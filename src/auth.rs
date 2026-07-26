use crate::config::Config;
use crate::github::{get_user, is_team_member, list_org_teams, GitHubUser};
use axum::{
    extract::{FromRef, FromRequestParts, Query, State},
    http::{header::SET_COOKIE, request::Parts, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use chrono::Utc;
use rand::Rng;
use serde::Deserialize;
use sqlx::PgPool;
use std::fmt::Write;
use std::future::Future;

pub const SESSION_COOKIE: &str = "rostfacto_session";
const OAUTH_STATE_COOKIE: &str = "rostfacto_oauth_state";
const SESSION_MAX_AGE_SECONDS: i64 = 60 * 60 * 24 * 7; // 7 days
const OAUTH_STATE_MAX_AGE_SECONDS: i64 = 60 * 10; // 10 minutes

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i32,
    pub github_id: i64,
    pub username: String,
    pub access_token: String,
    pub is_admin: bool,
    pub team_slugs: Vec<String>,
}

impl AuthUser {
    pub async fn is_member_of_team(&self, team_slug: &str, config: &Config) -> bool {
        if self.is_admin {
            return true;
        }
        if self.team_slugs.iter().any(|s| s == team_slug) {
            return true;
        }
        // Fallback to a live check if the team is not in the cached list.
        if let Some(org) = config.github_user_org.as_deref() {
            is_team_member(org, team_slug, &self.username, &self.access_token, config)
                .await
                .unwrap_or(false)
        } else {
            false
        }
    }
}

/// Placeholder used when authentication is disabled (demo mode). In this mode
/// all requests are treated as a synthetic admin user. The demo user is created
/// in the database at startup so that foreign keys remain valid.
const DEMO_GITHUB_ID: i64 = 0;

#[derive(sqlx::FromRow)]
struct UserRow {
    id: i32,
    github_id: i64,
    username: String,
    access_token: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
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
        INSERT INTO users (github_id, username, avatar_url, access_token)
        VALUES ($1, 'demo', NULL, 'demo')
        ON CONFLICT (github_id) DO UPDATE SET username = 'demo', access_token = 'demo'
        RETURNING id
        "#,
        DEMO_GITHUB_ID
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

async fn load_user_by_session(
    pool: &PgPool,
    session_id: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as!(
        UserRow,
        r#"
        SELECT u.id, u.github_id, u.username, u.access_token
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
        INSERT INTO users (github_id, username, avatar_url, access_token)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (github_id) DO UPDATE SET
            username = EXCLUDED.username,
            avatar_url = EXCLUDED.avatar_url,
            access_token = EXCLUDED.access_token
        RETURNING id, github_id, username, access_token
        "#,
        github_user.id,
        github_user.login,
        github_user.avatar_url,
        access_token
    )
    .fetch_one(pool)
    .await
}

async fn create_session(pool: &PgPool, user_id: i32) -> Result<String, sqlx::Error> {
    let session_id = generate_token();
    let expires_at = Utc::now() + chrono::Duration::try_seconds(SESSION_MAX_AGE_SECONDS).unwrap();
    sqlx::query!(
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, $3)",
        session_id,
        user_id,
        expires_at
    )
    .execute(pool)
    .await?;
    Ok(session_id)
}

async fn build_auth_user(user: UserRow, config: &Config) -> AuthUser {
    let is_admin = if let Some((org, team)) = config.admin_team() {
        match is_team_member(org, team, &user.username, &user.access_token, config).await {
            Ok(is_member) => is_member,
            Err(e) => {
                tracing::warn!(
                    "admin team check failed for user '{}' in {}/{}: {}",
                    user.username,
                    org,
                    team,
                    e
                );
                false
            }
        }
    } else {
        true
    };

    let team_slugs = if let Some(org) = config.github_user_org.as_deref() {
        match list_org_teams(org, &user.access_token, config).await {
            Ok(teams) => teams.into_iter().map(|t| t.slug).collect(),
            Err(e) => {
                tracing::warn!(
                    "failed to list teams in org '{}' for user '{}': {}",
                    org,
                    user.username,
                    e
                );
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    tracing::debug!(
        "user '{}': admin={}, visible teams={:?}",
        user.username,
        is_admin,
        team_slugs
    );

    AuthUser {
        user_id: user.id,
        github_id: user.github_id,
        username: user.username,
        access_token: user.access_token,
        is_admin,
        team_slugs,
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
            let mut kv = s.splitn(2, '=');
            let key = kv.next()?;
            let value = kv.next()?;
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

    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
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
                    access_token: "demo".to_string(),
                    is_admin: true,
                    team_slugs: vec!["demo".to_string()],
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

            let user = match load_user_by_session(&state.pool, &session_id).await {
                Ok(Some(user)) => user,
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
                Err(e) => {
                    tracing::error!("session lookup failed: {}", e);
                    return Err((StatusCode::INTERNAL_SERVER_ERROR, "Session lookup failed")
                        .into_response());
                }
            };

            Ok(build_auth_user(user, &state.config).await)
        }
    }
}

pub struct MaybeAuthUser(pub Option<AuthUser>);

impl<S> FromRequestParts<S> for MaybeAuthUser
where
    S: Send + Sync,
    crate::AppState: FromRef<S>,
{
    type Rejection = Response;

    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
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
                    access_token: "demo".to_string(),
                    is_admin: true,
                    team_slugs: vec!["demo".to_string()],
                })));
            }

            let session_id = match read_cookie(parts, SESSION_COOKIE) {
                Some(id) => id,
                None => return Ok(MaybeAuthUser(None)),
            };

            let user = match load_user_by_session(&state.pool, &session_id).await {
                Ok(Some(user)) => user,
                Ok(None) => return Ok(MaybeAuthUser(None)),
                Err(_) => {
                    return Err((StatusCode::INTERNAL_SERVER_ERROR, "Session lookup failed")
                        .into_response());
                }
            };

            Ok(MaybeAuthUser(Some(
                build_auth_user(user, &state.config).await,
            )))
        }
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
                    tracing::error!(
                        "OAuth token exchange returned {} but could not be parsed: {}",
                        status,
                        e
                    );
                    return (
                        StatusCode::BAD_REQUEST,
                        "Failed to parse OAuth token response",
                    )
                        .into_response();
                }
            }
        }
        Err(e) => {
            tracing::error!("OAuth token exchange request failed: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to exchange OAuth code").into_response();
        }
    };

    tracing::debug!("OAuth token acquired, fetching GitHub user profile");

    let github_user = match get_user(&token.access_token, &state.config).await {
        Ok(user) => user,
        Err(e) => {
            tracing::error!(
                "failed to fetch GitHub user from {}: {}",
                crate::github::api_base_url(&state.config),
                e
            );
            return (StatusCode::BAD_REQUEST, "Failed to fetch GitHub user").into_response();
        }
    };

    tracing::info!(
        "GitHub user '{}' (id {}) authenticated",
        github_user.login,
        github_user.id
    );

    let user = match upsert_user(&state.pool, &github_user, &token.access_token).await {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("failed to persist user: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to persist user").into_response();
        }
    };

    let session_id = match create_session(&state.pool, user.id).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("failed to create session: {}", e);
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
        let _ = sqlx::query!("DELETE FROM sessions WHERE id = $1", session_id)
            .execute(&state.pool)
            .await;
    }

    tracing::debug!("user signed out");

    (
        StatusCode::SEE_OTHER,
        [
            ("Location", "/".to_string()),
            ("Set-Cookie", clear_session_cookie()),
        ],
    )
        .into_response()
}
