//! Handlers for retrospectives: list, create, show, archive, delete, and the
//! archive snapshots.

use super::access::{require_retro_access, require_retro_access_by_id};
use super::emit::emit_event;
use super::error::{forbidden, not_found_page, not_found_response};
use super::{database_error_response, log_database_error, HandlerError, MAX_RETRO_TITLE_LENGTH};
use crate::auth::{
    read_cookie, AuthUser, MaybeAuthUser, ADMIN_REAUTH_MAX_AGE_SECONDS, SESSION_COOKIE,
};
use crate::events::EventType;
use crate::models::{apply_author_initials, Archive, Retrospective};
use crate::templates::{
    ArchiveListEntry, ArchiveTemplate, ArchivesTemplate, GitHubTeam, HomeTemplate,
    NewRetroTemplate, RetroTemplate, RetrosTemplate,
};
use crate::AppState;
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::{request::Parts, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;

pub async fn home(State(state): State<AppState>, maybe_user: MaybeAuthUser) -> Html<String> {
    let template = HomeTemplate {
        user: maybe_user.0,
        demo_mode: state.config.demo_mode(),
    };
    Html(template.render().unwrap())
}

pub async fn list_retros(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Html<String>, HandlerError> {
    let retros = if user.is_admin {
        sqlx::query_as!(
            Retrospective,
            "SELECT * FROM retrospectives ORDER BY created_at DESC"
        )
        .fetch_all(&state.pool)
        .await
    } else {
        // Qualified team slugs ("org/team") also match retros created before
        // multi-org support, which store the bare team slug.
        let mut team_slugs = user.team_slugs.clone();
        team_slugs.extend(
            user.team_slugs
                .iter()
                .filter_map(|s| s.rsplit_once('/').map(|(_, bare)| bare.to_string())),
        );
        sqlx::query_as!(
            Retrospective,
            "SELECT * FROM retrospectives WHERE team_slug = ANY($1) ORDER BY created_at DESC",
            &team_slugs
        )
        .fetch_all(&state.pool)
        .await
    }
    .map_err(|error| {
        log_database_error("list_retros", &error);
        database_error_response()
    })?;

    let template = RetrosTemplate {
        retros,
        is_admin: user.is_admin,
        user: Some(user),
        demo_mode: state.config.demo_mode(),
    };
    Ok(Html(template.render().unwrap()))
}

/// Build the new-retro form template. Shared by the initial GET and the
/// re-render after a rejected submission, so the user's input is preserved
/// instead of being replaced by a generic error page.
fn new_retro_template(
    state: &AppState,
    user: AuthUser,
    error_message: Option<String>,
    title_value: String,
    slug_value: String,
    team_slug_value: String,
) -> NewRetroTemplate {
    let teams = user
        .teams
        .iter()
        .map(|t| GitHubTeam {
            slug: t.slug.clone(),
        })
        .collect();

    NewRetroTemplate {
        is_admin: user.is_admin,
        teams,
        team_listing_errors: user.team_listing_errors.clone(),
        applications_url: state.config.applications_url(),
        app_owner: state.config.github_app_owner.clone().unwrap_or_default(),
        demo_mode: state.config.demo_mode(),
        user: Some(user),
        error_message,
        title_value,
        slug_value,
        team_slug_value,
    }
}

/// Re-render the new-retro form with a validation error and the submitted
/// values, so a rejected submission stays on the form (with the error shown)
/// instead of navigating to a standalone error page.
fn new_retro_error_response(
    state: &AppState,
    user: AuthUser,
    status: StatusCode,
    message: &str,
    title: &str,
    slug: &str,
    team_slug: &str,
) -> Response {
    let template = new_retro_template(
        state,
        user,
        Some(message.to_string()),
        title.to_string(),
        slug.to_string(),
        team_slug.to_string(),
    );
    (status, Html(template.render().unwrap())).into_response()
}

pub async fn new_retro(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Html<String>, HandlerError> {
    if !user.is_admin {
        return Err(forbidden(&state, "Only admins can create retrospectives").into());
    }

    let template = new_retro_template(
        &state,
        user,
        None,
        String::new(),
        String::new(),
        String::new(),
    );
    Ok(Html(template.render().unwrap()))
}

/// Availability check for the new-retro form's slug field. Returns an HTML
/// fragment (empty when the slug is free) for htmx to swap in next to the
/// field, so a clash is flagged while the user types rather than only on
/// submit.
pub async fn slug_check(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<SlugCheck>,
) -> Response {
    if !user.is_admin {
        return forbidden(&state, "Only admins can create retrospectives");
    }

    let slug = query.slug.as_deref().unwrap_or("").trim();
    if slug.is_empty() {
        return Html(String::new()).into_response();
    }

    let taken = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM retrospectives WHERE slug = $1)",
        slug
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("slug_check", &error);
        database_error_response()
    });

    match taken {
        Ok(Some(true)) => {
            Html(r#"<span class="slug-warning">Slug is already in use</span>"#.to_string())
                .into_response()
        }
        Ok(_) => Html(String::new()).into_response(),
        Err(response) => response,
    }
}

pub async fn create_retro(
    State(state): State<AppState>,
    user: AuthUser,
    Form(form): Form<NewRetro>,
) -> impl IntoResponse {
    if !user.is_admin {
        return forbidden(&state, "Only admins can create retrospectives");
    }

    // Echoed back when a submission is rejected so the form is not cleared.
    let submitted_title = form.title.clone();
    let submitted_slug = form.slug.clone();
    let submitted_team_slug = form.team_slug.clone().unwrap_or_default();

    if form.slug.is_empty() {
        return new_retro_error_response(
            &state,
            user,
            StatusCode::BAD_REQUEST,
            "Slug is required",
            &submitted_title,
            &submitted_slug,
            &submitted_team_slug,
        );
    }
    if form.slug.len() > 255 {
        return new_retro_error_response(
            &state,
            user,
            StatusCode::BAD_REQUEST,
            "Slug must be 255 characters or less",
            &submitted_title,
            &submitted_slug,
            &submitted_team_slug,
        );
    }
    if !form
        .slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return new_retro_error_response(
            &state,
            user,
            StatusCode::BAD_REQUEST,
            "Slug can only contain lowercase letters, numbers, and dashes",
            &submitted_title,
            &submitted_slug,
            &submitted_team_slug,
        );
    }

    let title = form.title.trim();
    if title.is_empty() {
        return new_retro_error_response(
            &state,
            user,
            StatusCode::BAD_REQUEST,
            "Title is required",
            &submitted_title,
            &submitted_slug,
            &submitted_team_slug,
        );
    }
    if title.chars().count() > MAX_RETRO_TITLE_LENGTH {
        return new_retro_error_response(
            &state,
            user,
            StatusCode::BAD_REQUEST,
            &format!("Title must be {MAX_RETRO_TITLE_LENGTH} characters or less"),
            &submitted_title,
            &submitted_slug,
            &submitted_team_slug,
        );
    }

    let team_slug = if state.config.demo_mode() {
        form.team_slug.clone().unwrap_or_else(|| "demo".to_string())
    } else {
        match form.team_slug.clone() {
            Some(s) if !s.is_empty() => s,
            _ => {
                return new_retro_error_response(
                    &state,
                    user,
                    StatusCode::BAD_REQUEST,
                    "Team is required",
                    &submitted_title,
                    &submitted_slug,
                    &submitted_team_slug,
                )
            }
        }
    };

    let retro = match sqlx::query_as!(
        Retrospective,
        "INSERT INTO retrospectives (title, slug, team_slug, created_by) VALUES ($1, $2, $3, $4) RETURNING *",
        title,
        form.slug,
        team_slug,
        user.user_id
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(retro) => retro,
        Err(error) => {
            if error
                .as_database_error()
                .and_then(|database_error| database_error.constraint())
                == Some("retrospectives_slug_key")
            {
                return new_retro_error_response(
                    &state,
                    user,
                    StatusCode::CONFLICT,
                    "Slug is already in use",
                    &submitted_title,
                    &submitted_slug,
                    &submitted_team_slug,
                );
            }
            log_database_error("create_retro", &error);
            return database_error_response();
        }
    };

    tracing::info!(
        retro_id = retro.id,
        user_id = user.user_id,
        "retrospective created"
    );

    (
        StatusCode::SEE_OTHER,
        [("Location", format!("/retro/{}", retro.slug))],
    )
        .into_response()
}

pub async fn show_retro(
    State(state): State<AppState>,
    user: AuthUser,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, HandlerError> {
    let retro = match require_retro_access(&state, &user, &slug).await? {
        Some(r) => r,
        None => return Ok(not_found_response(&state, &slug)),
    };

    let mut good_items = item_query_as!(
        r#"i.retro_id = $1
           AND i.category = 'GOOD'
           AND i.archive_id IS NULL
           ORDER BY i.created_at ASC"#,
        retro.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_retro_good_items", &error);
        database_error_response()
    })?;

    let mut bad_items = item_query_as!(
        r#"i.retro_id = $1
           AND i.category = 'BAD'
           AND i.archive_id IS NULL
           ORDER BY i.created_at ASC"#,
        retro.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_retro_bad_items", &error);
        database_error_response()
    })?;

    let mut watch_items = item_query_as!(
        r#"i.retro_id = $1
           AND i.category = 'WATCH'
           AND i.archive_id IS NULL
           ORDER BY i.created_at ASC"#,
        retro.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_retro_watch_items", &error);
        database_error_response()
    })?;

    let action_items = action_item_query_as!(
        "SELECT ",
        " FROM action_items WHERE retro_id = $1 AND archive_id IS NULL ORDER BY created_at ASC",
        retro.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_retro_action_items", &error);
        database_error_response()
    })?;

    apply_author_initials(&mut [&mut good_items, &mut bad_items, &mut watch_items]);

    let all_completed = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM items
            WHERE retro_id = $1
            AND archive_id IS NULL
        )
        AND NOT EXISTS (
            SELECT 1 FROM items
            WHERE retro_id = $1
            AND archive_id IS NULL
            AND status != 'COMPLETED'::status
        )
        "#,
        retro.id
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_retro_completion_status", &error);
        database_error_response()
    })?
    .unwrap_or(false);

    let can_archive = !good_items.is_empty()
        || !bad_items.is_empty()
        || !watch_items.is_empty()
        || !action_items.is_empty();

    // In demo mode the participant id is client-generated and only known to the
    // browser, so the key is left empty and derived client-side.
    let participant_key = if state.config.demo_mode() {
        String::new()
    } else {
        format!("user:{}", user.user_id)
    };

    let template = RetroTemplate {
        retro,
        good_items,
        bad_items,
        watch_items,
        action_items,
        show_archive_modal: all_completed,
        is_admin: user.is_admin,
        user: Some(user),
        demo_mode: state.config.demo_mode(),
        error_message: None,
        can_archive,
        participant_key,
    };

    Ok(Html(template.render().unwrap()).into_response())
}

pub async fn archive_retro(
    State(state): State<AppState>,
    user: AuthUser,
    Path(retro_id): Path<i32>,
) -> Result<impl IntoResponse, HandlerError> {
    let retro = match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(r) => r,
        None => {
            return Ok(not_found_response(&state, ""));
        }
    };

    let active_items_count = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM items WHERE retro_id = $1 AND archive_id IS NULL",
        retro_id
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("archive_retro_count_active_items", &error);
        database_error_response()
    })?
    .unwrap_or(0);
    let active_action_items_count = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM action_items WHERE retro_id = $1 AND archive_id IS NULL",
        retro_id
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("archive_retro_count_action_items", &error);
        database_error_response()
    })?
    .unwrap_or(0);

    if active_items_count > 0 || active_action_items_count > 0 {
        let mut tx = state.pool.begin().await.map_err(|error| {
            log_database_error("archive_retro_begin_transaction", &error);
            database_error_response()
        })?;
        let archive_id = sqlx::query_scalar!(
            "INSERT INTO archives (retro_id) VALUES ($1) RETURNING id",
            retro_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| {
            log_database_error("archive_retro_create_snapshot", &error);
            database_error_response()
        })?;
        sqlx::query!(
            "UPDATE items SET status = 'ARCHIVED'::status, archive_id = $1, archived_at = NOW()
             WHERE retro_id = $2 AND archive_id IS NULL",
            archive_id,
            retro_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            log_database_error("archive_retro_items", &error);
            database_error_response()
        })?;
        sqlx::query!(
            "UPDATE action_items SET archive_id = $1, archived_at = NOW()
             WHERE retro_id = $2 AND archive_id IS NULL",
            archive_id,
            retro_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            log_database_error("archive_retro_action_items", &error);
            database_error_response()
        })?;
        emit_event(
            &mut tx,
            retro_id,
            EventType::RetroArchived as EventType,
            None,
            json!({"retro_id": retro_id}),
        )
        .await
        .map_err(|error| {
            log_database_error("archive_retro_emit_event", &error);
            database_error_response()
        })?;
        tx.commit().await.map_err(|error| {
            log_database_error("archive_retro_commit_transaction", &error);
            database_error_response()
        })?;

        tracing::info!(
            retro_id,
            user_id = user.user_id,
            archived_items = active_items_count,
            archived_action_items = active_action_items_count,
            "retrospective archived"
        );
    } else {
        tracing::info!(
            retro_id,
            user_id = user.user_id,
            "retro has no active items to archive"
        );
    }

    Ok((
        StatusCode::SEE_OTHER,
        [("Location", format!("/retro/{}", retro.slug))],
    )
        .into_response())
}

pub async fn delete_retro(
    State(state): State<AppState>,
    user: AuthUser,
    Path(slug): Path<String>,
    parts: Parts,
) -> impl IntoResponse {
    if !user.is_admin {
        return forbidden(&state, "Only admins can delete retrospectives");
    }

    // Step-up re-authentication: deleting a retro is destructive, so require
    // a login that is at most ADMIN_REAUTH_MAX_AGE_SECONDS old. Older admins
    // are sent back through the OAuth flow, which creates a fresh session.
    if !state.config.demo_mode() {
        let created_at = match read_cookie(&parts, SESSION_COOKIE) {
            Some(session_id) => {
                sqlx::query_scalar!("SELECT created_at FROM sessions WHERE id = $1", session_id)
                    .fetch_optional(&state.pool)
                    .await
                    .ok()
                    .flatten()
            }
            None => None,
        };
        let fresh_login = created_at.is_some_and(|created_at| {
            Utc::now() - created_at
                < chrono::Duration::try_seconds(ADMIN_REAUTH_MAX_AGE_SECONDS).unwrap()
        });

        if !fresh_login {
            tracing::info!(
                user_id = user.user_id,
                "admin re-authentication required before deleting a retro"
            );
            return (
                StatusCode::FORBIDDEN,
                [("HX-Redirect", "/auth/login"), ("Location", "/auth/login")],
                "Re-authentication required to delete a retro",
            )
                .into_response();
        }
    }

    let retro = match sqlx::query_as!(
        Retrospective,
        "DELETE FROM retrospectives WHERE slug = $1 RETURNING *",
        slug
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(retro) => retro,
        Err(sqlx::Error::RowNotFound) => return not_found_page(&state),
        Err(error) => {
            log_database_error("delete_retro", &error);
            return database_error_response();
        }
    };

    tracing::info!(
        retro_id = retro.id,
        user_id = user.user_id,
        "retrospective deleted"
    );
    StatusCode::OK.into_response()
}

pub async fn list_archives(
    State(state): State<AppState>,
    user: AuthUser,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, HandlerError> {
    let retro = match require_retro_access(&state, &user, &slug).await? {
        Some(r) => r,
        None => return Ok(not_found_response(&state, &slug)),
    };

    let archives = sqlx::query_as!(
        Archive,
        r#"
        SELECT id, retro_id, created_at
        FROM archives
        WHERE retro_id = $1
        ORDER BY created_at DESC
        "#,
        retro.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("list_archives", &error);
        database_error_response()
    })?;

    let mut archive_entries = Vec::with_capacity(archives.len());
    for archive in archives {
        let items_count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM items WHERE archive_id = $1",
            archive.id
        )
        .fetch_one(&state.pool)
        .await
        .map_err(|error| {
            log_database_error("list_archives_items_count", &error);
            database_error_response()
        })?
        .unwrap_or(0);

        let action_items_count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM action_items WHERE archive_id = $1",
            archive.id
        )
        .fetch_one(&state.pool)
        .await
        .map_err(|error| {
            log_database_error("list_archives_action_items_count", &error);
            database_error_response()
        })?
        .unwrap_or(0);

        archive_entries.push(ArchiveListEntry {
            archive,
            items_count,
            action_items_count,
        });
    }

    let can_archive = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM items WHERE retro_id = $1 AND archive_id IS NULL)
         OR EXISTS(SELECT 1 FROM action_items WHERE retro_id = $1 AND archive_id IS NULL)",
        retro.id
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("list_archives_can_archive", &error);
        database_error_response()
    })?
    .unwrap_or(false);

    Ok(Html(
        ArchivesTemplate {
            retro,
            archives: archive_entries,
            is_admin: user.is_admin,
            user: Some(user),
            demo_mode: state.config.demo_mode(),
            can_archive,
        }
        .render()
        .unwrap(),
    )
    .into_response())
}

pub async fn show_archive(
    State(state): State<AppState>,
    user: AuthUser,
    Path((slug, archive_id)): Path<(String, i32)>,
) -> Result<impl IntoResponse, HandlerError> {
    let retro = match require_retro_access(&state, &user, &slug).await? {
        Some(r) => r,
        None => return Ok(not_found_response(&state, &slug)),
    };

    let archive = match sqlx::query_as!(
        Archive,
        r#"
        SELECT id, retro_id, created_at
        FROM archives
        WHERE id = $1 AND retro_id = $2
        "#,
        archive_id,
        retro.id
    )
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(a)) => a,
        Ok(None) => return Err(not_found_page(&state).into()),
        Err(error) => {
            log_database_error("show_archive", &error);
            return Err(database_error_response().into());
        }
    };

    let mut good_items = item_query_as!(
        r#"i.archive_id = $1
           AND i.category = 'GOOD'
           ORDER BY i.created_at ASC"#,
        archive.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_archive_good_items", &error);
        database_error_response()
    })?;

    let mut bad_items = item_query_as!(
        r#"i.archive_id = $1
           AND i.category = 'BAD'
           ORDER BY i.created_at ASC"#,
        archive.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_archive_bad_items", &error);
        database_error_response()
    })?;

    let mut watch_items = item_query_as!(
        r#"i.archive_id = $1
           AND i.category = 'WATCH'
           ORDER BY i.created_at ASC"#,
        archive.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_archive_watch_items", &error);
        database_error_response()
    })?;

    apply_author_initials(&mut [&mut good_items, &mut bad_items, &mut watch_items]);

    let action_items = action_item_query_as!(
        "SELECT ",
        " FROM action_items WHERE archive_id = $1 ORDER BY created_at ASC",
        archive.id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_archive_action_items", &error);
        database_error_response()
    })?;

    let can_archive = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM items WHERE retro_id = $1 AND archive_id IS NULL)
         OR EXISTS(SELECT 1 FROM action_items WHERE retro_id = $1 AND archive_id IS NULL)",
        retro.id
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("show_archive_can_archive", &error);
        database_error_response()
    })?
    .unwrap_or(false);

    Ok(Html(
        ArchiveTemplate {
            retro,
            archive,
            good_items,
            bad_items,
            watch_items,
            action_items,
            is_admin: user.is_admin,
            user: Some(user),
            demo_mode: state.config.demo_mode(),
            can_archive,
        }
        .render()
        .unwrap(),
    )
    .into_response())
}

#[derive(Deserialize)]
pub struct NewRetro {
    title: String,
    slug: String,
    team_slug: Option<String>,
}

#[derive(Deserialize)]
pub struct SlugCheck {
    slug: Option<String>,
}
