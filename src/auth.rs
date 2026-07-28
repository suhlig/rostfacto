use crate::github::{get_user, is_team_member, list_org_teams, GitHubUser};
use axum::{
    extract::{FromRef, FromRequestParts, Query, State},
    http::{header::SET_COOKIE, request::Parts, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::fmt::Write;

pub const SESSION_COOKIE: &str = "rostfacto_session";
const OAUTH_STATE_COOKIE: &str = "rostfacto_oauth_state";
const SESSION_MAX_AGE_SECONDS: i64 = 60 * 60 * 24 * 7; // 7 days
const OAUTH_STATE_MAX_AGE_SECONDS: i64 = 60 * 10; // 10 minutes

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTeam {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i32,
    pub github_id: i64,
    pub username: String,
    pub full_name: String,
    pub access_token: String,
    pub is_admin: bool,
    pub team_slugs: Vec<String>,
    pub teams: Vec<CachedTeam>,
}

impl AuthUser {
    pub fn is_member_of_team(&self, team_slug: &str) -> bool {
        self.is_admin || self.team_slugs.iter().any(|s| s == team_slug)
    }
}

/// Placeholder used when authentication is disabled (demo mode). In this mode
/// all requests are treated as a synthetic admin user. The demo user is created
/// in the database at startup so that foreign keys remain valid.
const DEMO_GITHUB_ID: i64 = 0;

#[derive(sqlx::FromRow)]
struct UserRow {
    id: i32,
    username: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct SessionRow {
    user_id: i32,
    github_id: i64,
    username: String,
    full_name: Option<String>,
    access_token: String,
    is_admin: bool,
    teams: sqlx::types::Json<Vec<CachedTeam>>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    bytes.iter().fold(String::new(), |mut acc, b| {
        let _ = write!(acc, "{:02x}", b);
        acc
    })
}

fn set_cookie(name: &str, value: &str, max_age_seconds: i64) -> String {
    format!(
        "{}={}; Max-Age={}; Path=/; HttpOnly; SameSite=Lax",
        name, value, max_age_seconds
    )
}

fn clear_cookie(name: &str) -> String {
    format!("{}=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax", name)
}

pub async fn ensure_demo_user(pool: &PgPool) -> Result<i32, sqlx::Error> {
    let user = sqlx::query_scalar!(
        r#"
        INSERT INTO users (github_id, username, full_name, avatar_url, access_token)
        VALUES ($1, 'demo', 'Demo User', NULL, 'demo')
        ON CONFLICT (github_id) DO UPDATE SET
            username = 'demo', full_name = 'Demo User', access_token = 'demo'
        RETURNING id
        "#,
        DEMO_GITHUB_ID
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub(crate) async fn load_session(
    pool: &PgPool,
    session_id: &str,
) -> Result<Option<SessionRow>, sqlx::Error> {
    sqlx::query_as!(
        SessionRow,
        r#"
        SELECT
            u.id as user_id,
            u.github_id,
            u.username,
            u.full_name,
            u.access_token,
            s.is_admin,
            s.teams as "teams: _"
        FROM users u
        JOIN sessions s ON s.user_id = u.id
        WHERE s.id = $1 AND s.expires_at > NOW()
        "#,
        session_id
    )
    .fetch_optional(pool)
    .await
}

async fn upsert_user(
    pool: &PgPool,
    github_user: &GitHubUser,
    access_token: &str,
) -> Result<UserRow, sqlx::Error> {
    sqlx::query_as!(
        UserRow,
        r#"
        INSERT INTO users (github_id, username, full_name, avatar_url, access_token)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (github_id) DO UPDATE SET
            username = EXCLUDED.username,
            full_name = EXCLUDED.full_name,
            avatar_url = EXCLUDED.avatar_url,
            access_token = EXCLUDED.access_token
        RETURNING id, username
        "#,
        github_user.id,
        github_user.login,
        github_user.name,
        github_user.avatar_url,
        access_token
    )
    .fetch_one(pool)
    .await
}

pub(crate) async fn create_session(
    pool: &PgPool,
    user_id: i32,
    is_admin: bool,
    teams: &[CachedTeam],
) -> Result<String, sqlx::Error> {
    let session_id = generate_token();
    let expires_at = Utc::now() + chrono::Duration::try_seconds(SESSION_MAX_AGE_SECONDS).unwrap();
    let teams_json = serde_json::to_value(teams).unwrap();
    sqlx::query!(
        "INSERT INTO sessions (id, user_id, expires_at, is_admin, teams) VALUES ($1, $2, $3, $4, $5)",
        session_id,
        user_id,
        expires_at,
        is_admin,
        teams_json
    )
    .execute(pool)
    .await?;
    Ok(session_id)
}

pub(crate) fn auth_user_from_session(session: SessionRow) -> AuthUser {
    let team_slugs = session.teams.iter().map(|t| t.slug.clone()).collect();
    let full_name = session
        .full_name
        .unwrap_or_else(|| session.username.clone());

    tracing::debug!(
        user_id = session.user_id,
        is_admin = session.is_admin,
        "auth user loaded from session"
    );

    AuthUser {
        user_id: session.user_id,
        github_id: session.github_id,
        username: session.username,
        full_name,
        access_token: session.access_token,
        is_admin: session.is_admin,
        team_slugs,
        teams: session.teams.into_inner(),
    }
}

fn session_cookie(session_id: &str) -> String {
    set_cookie(SESSION_COOKIE, session_id, SESSION_MAX_AGE_SECONDS)
}

fn clear_session_cookie() -> String {
    clear_cookie(SESSION_COOKIE)
}

fn read_cookie(parts: &Parts, name: &str) -> Option<String> {
    parts
        .headers
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .map(|s| s.trim())
        .find_map(|s| {
            let (key, value) = s.split_once('=')?;
            if key == name {
                Some(value.to_string())
            } else {
                None
            }
        })
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    crate::AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = crate::AppState::from_ref(state);

        if state.config.demo_mode() {
            let user_id = state.demo_user_id.ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Demo user not initialized",
                )
                    .into_response()
            })?;
            return Ok(AuthUser {
                user_id,
                github_id: DEMO_GITHUB_ID,
                username: "demo".to_string(),
                full_name: "Demo User".to_string(),
                access_token: "demo".to_string(),
                is_admin: true,
                team_slugs: vec!["demo".to_string()],
                teams: vec![CachedTeam {
                    slug: "demo".to_string(),
                    name: "Demo Team".to_string(),
                }],
            });
        }

        let session_id = match read_cookie(parts, SESSION_COOKIE) {
            Some(id) => id,
            None => {
                tracing::debug!("no session cookie present; redirecting to login");
                return Err((
                    StatusCode::SEE_OTHER,
                    [("Location", "/auth/login")],
                    "Redirecting to login",
                )
                    .into_response());
            }
        };

        let session = match load_session(&state.pool, &session_id).await {
            Ok(Some(session)) => session,
            Ok(None) => {
                tracing::debug!("session not found or expired; redirecting to login");
                return Err((
                    StatusCode::SEE_OTHER,
                    [
                        ("Location", "/auth/login"),
                        ("Set-Cookie", &clear_session_cookie()),
                    ],
                    "Redirecting to login",
                )
                    .into_response());
            }
            Err(error) => {
                tracing::error!(error_type = "session_lookup", "session lookup failed");
                tracing::debug!(error = %error, "session lookup failure details");
                return Err(
                    (StatusCode::INTERNAL_SERVER_ERROR, "Session lookup failed").into_response()
                );
            }
        };

        Ok(auth_user_from_session(session))
    }
}

pub struct MaybeAuthUser(pub Option<AuthUser>);

impl<S> FromRequestParts<S> for MaybeAuthUser
where
    S: Send + Sync,
    crate::AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = crate::AppState::from_ref(state);

        if state.config.demo_mode() {
            let user_id = state.demo_user_id.ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Demo user not initialized",
                )
                    .into_response()
            })?;
            return Ok(MaybeAuthUser(Some(AuthUser {
                user_id,
                github_id: DEMO_GITHUB_ID,
                username: "demo".to_string(),
                full_name: "Demo User".to_string(),
                access_token: "demo".to_string(),
                is_admin: true,
                team_slugs: vec!["demo".to_string()],
                teams: vec![CachedTeam {
                    slug: "demo".to_string(),
                    name: "Demo Team".to_string(),
                }],
            })));
        }

        let session_id = match read_cookie(parts, SESSION_COOKIE) {
            Some(id) => id,
            None => return Ok(MaybeAuthUser(None)),
        };

        let session = match load_session(&state.pool, &session_id).await {
            Ok(Some(session)) => session,
            Ok(None) => return Ok(MaybeAuthUser(None)),
            Err(error) => {
                tracing::error!(error_type = "session_lookup", "session lookup failed");
                tracing::debug!(error = %error, "session lookup failure details");
                return Err(
                    (StatusCode::INTERNAL_SERVER_ERROR, "Session lookup failed").into_response()
                );
            }
        };

        Ok(MaybeAuthUser(Some(auth_user_from_session(session))))
    }
}

pub async fn login(State(state): State<crate::AppState>) -> impl IntoResponse {
    if state.config.demo_mode() {
        tracing::debug!("demo mode: /auth/login redirects to /");
        return Redirect::to("/").into_response();
    }

    let state_token = generate_token();
    tracing::debug!("starting OAuth flow");
    let redirect_uri = format!("{}/auth/callback", state.config.public_url);

    let mut url = url::Url::parse(&crate::github::oauth_authorize_url(&state.config))
        .expect("invalid GitHub authorize URL");
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("client_id", &state.config.github_client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", "read:user read:org")
            .append_pair("state", &state_token)
            .append_pair("response_type", "code");
    }

    (
        StatusCode::SEE_OTHER,
        [
            ("Location", url.to_string()),
            (
                "Set-Cookie",
                set_cookie(
                    OAUTH_STATE_COOKIE,
                    &state_token,
                    OAUTH_STATE_MAX_AGE_SECONDS,
                ),
            ),
        ],
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

pub async fn callback(
    State(state): State<crate::AppState>,
    Query(params): Query<CallbackQuery>,
    parts: Parts,
) -> impl IntoResponse {
    if state.config.demo_mode() {
        return Redirect::to("/").into_response();
    }

    let stored_state = read_cookie(&parts, OAUTH_STATE_COOKIE);
    let expected_state = match stored_state {
        Some(s) => s,
        None => {
            tracing::warn!("OAuth callback: missing state cookie");
            return (StatusCode::BAD_REQUEST, "Missing OAuth state cookie").into_response();
        }
    };

    if params.state != expected_state {
        tracing::warn!("OAuth callback: state mismatch");
        return (StatusCode::BAD_REQUEST, "Invalid OAuth state").into_response();
    }

    tracing::debug!("OAuth callback: state verified, exchanging code for token");

    let client = reqwest::Client::new();
    let redirect_uri = format!("{}/auth/callback", state.config.public_url);
    let token_response = client
        .post(crate::github::oauth_token_url(&state.config))
        .form(&[
            ("client_id", state.config.github_client_id.as_str()),
            ("client_secret", state.config.github_client_secret.as_str()),
            ("code", params.code.as_str()),
            ("redirect_uri", &redirect_uri),
            ("grant_type", "authorization_code"),
        ])
        .header("Accept", "application/json")
        .send()
        .await;

    let token = match token_response {
        Ok(response) => {
            let status = response.status();
            match response.json::<TokenResponse>().await {
                Ok(token) => token,
                Err(e) => {
                    tracing::error!(status = %status, "OAuth token response could not be parsed");
                    tracing::debug!(error = %e, "OAuth token response parse failure details");
                    return (
                        StatusCode::BAD_REQUEST,
                        "Failed to parse OAuth token response",
                    )
                        .into_response();
                }
            }
        }
        Err(error) => {
            tracing::error!(operation = "oauth_token_exchange", "GitHub request failed");
            tracing::debug!(error = %error, "OAuth token exchange failure details");
            return (StatusCode::BAD_REQUEST, "Failed to exchange OAuth code").into_response();
        }
    };

    tracing::debug!("OAuth token acquired, fetching GitHub user profile");

    let github_user = match get_user(&token.access_token, &state.config).await {
        Ok(user) => user,
        Err(error) => {
            tracing::error!(operation = "github_get_user", "GitHub request failed");
            tracing::debug!(error = %error, "GitHub user request failure details");
            return (StatusCode::BAD_REQUEST, "Failed to fetch GitHub user").into_response();
        }
    };

    tracing::debug!(
        username = %github_user.login,
        github_id = github_user.id,
        "GitHub identity authenticated"
    );

    let user = match upsert_user(&state.pool, &github_user, &token.access_token).await {
        Ok(user) => user,
        Err(error) => {
            tracing::error!(operation = "persist_user", "database operation failed");
            tracing::debug!(error = %error, "user persistence failure details");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to persist user").into_response();
        }
    };

    // Resolve team membership once at login and cache it in the session. Subsequent
    // requests read the cached values and do not call the GitHub API.
    let is_admin = if let Some((org, team)) = state.config.admin_team() {
        match is_team_member(
            org,
            team,
            &user.username,
            &token.access_token,
            &state.config,
        )
        .await
        {
            Ok(is_member) => is_member,
            Err(e) => {
                tracing::warn!(org, team, "admin team membership check failed");
                tracing::debug!(username = %user.username, error = %e, "admin team membership check failure details");
                false
            }
        }
    } else {
        true
    };

    let teams: Vec<CachedTeam> = if let Some(org) = state.config.github_user_org.as_deref() {
        match list_org_teams(org, &token.access_token, &state.config).await {
            Ok(teams) => teams
                .into_iter()
                .map(|t| CachedTeam {
                    slug: t.slug,
                    name: t.name,
                })
                .collect(),
            Err(e) => {
                tracing::warn!(org, "failed to list teams for authenticated user");
                tracing::debug!(username = %user.username, error = %e, "team listing failure details");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    tracing::info!(
        user_id = user.id,
        is_admin,
        team_count = teams.len(),
        "user authenticated"
    );

    let session_id = match create_session(&state.pool, user.id, is_admin, &teams).await {
        Ok(id) => id,
        Err(error) => {
            tracing::error!(operation = "create_session", "database operation failed");
            tracing::debug!(error = %error, "session creation failure details");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create session",
            )
                .into_response();
        }
    };

    tracing::debug!("session created for user '{}'", github_user.login);

    let mut response = (StatusCode::SEE_OTHER, [("Location", "/")]).into_response();
    let headers = response.headers_mut();
    headers.append(SET_COOKIE, session_cookie(&session_id).parse().unwrap());
    headers.append(
        SET_COOKIE,
        clear_cookie(OAUTH_STATE_COOKIE).parse().unwrap(),
    );
    response
}

pub async fn logout(
    State(state): State<crate::AppState>,
    _user: AuthUser,
    parts: Parts,
) -> impl IntoResponse {
    if let Some(session_id) = read_cookie(&parts, SESSION_COOKIE) {
        if let Err(error) = sqlx::query!("DELETE FROM sessions WHERE id = $1", session_id)
            .execute(&state.pool)
            .await
        {
            tracing::error!(
                error_type = "session_delete",
                "failed to delete session during logout"
            );
            tracing::debug!(error = %error, "session deletion failure details");
        }
    }

    tracing::info!("user signed out");

    (
        StatusCode::SEE_OTHER,
        [
            ("Location", "/".to_string()),
            ("Set-Cookie", clear_session_cookie()),
        ],
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    #[tokio::test]
    async fn session_round_trip_caches_teams_and_admin_status() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");
        let pool = PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to database");

        let user_id = ensure_demo_user(&pool)
            .await
            .expect("Failed to ensure demo user");

        let teams = vec![
            CachedTeam {
                slug: "team-a".to_string(),
                name: "Team A".to_string(),
            },
            CachedTeam {
                slug: "team-b".to_string(),
                name: "Team B".to_string(),
            },
        ];

        let session_id = create_session(&pool, user_id, true, &teams)
            .await
            .expect("Failed to create session");

        let session = load_session(&pool, &session_id)
            .await
            .expect("Failed to load session")
            .expect("Session should exist");

        let user = auth_user_from_session(session);

        assert!(user.is_admin, "cached admin status should be true");
        assert_eq!(
            user.team_slugs,
            vec!["team-a".to_string(), "team-b".to_string()]
        );
        assert_eq!(user.teams.len(), 2);
        assert_eq!(user.teams[0].slug, "team-a");
        assert_eq!(user.teams[1].name, "Team B");

        // Clean up the test session.
        sqlx::query!("DELETE FROM sessions WHERE id = $1", session_id)
            .execute(&pool)
            .await
            .expect("Failed to delete test session");
    }
}
