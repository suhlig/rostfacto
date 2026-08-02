use crate::auth::{AuthUser, MaybeAuthUser};
use crate::models::{
    apply_author_initials, ActionItem, Archive, Category, Item, Retrospective, Status,
};
use crate::templates::{
    ActionItemEditTemplate, ActionItemTemplate, ArchiveListEntry, ArchiveModalTemplate,
    ArchiveTemplate, ArchivesTemplate, ErrorTemplate, GitHubTeam, HomeTemplate, ItemCardTemplate,
    ItemEditTemplate, NewRetroTemplate, RetroTemplate, RetrosTemplate,
};
use crate::AppState;
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::{header::HeaderName, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;

fn log_database_error(operation: &'static str, error: &sqlx::Error) {
    if let Some(database_error) = error.as_database_error() {
        tracing::error!(
            operation,
            database_code = database_error.code().as_deref(),
            constraint = database_error.constraint(),
            "database operation failed"
        );
    } else {
        tracing::error!(operation, error_type = "sqlx", "database operation failed");
    }
    tracing::debug!(operation, error = %error, "database operation failure details");
}

fn database_error_response() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
}

async fn load_item_with_initials(
    conn: &mut sqlx::PgConnection,
    item_id: i32,
) -> Result<Item, sqlx::Error> {
    let mut items = sqlx::query_as!(
        Item,
        r#"SELECT i.id as "id!", i.retro_id as "retro_id!", i.text as "text!",
                  i.category as "category: _", i.created_at as "created_at!", i.status as "status: _",
                  i.created_by as "author_id!", u.display_name as "author_name!",
                  ''::text as "author_initials!",
                  (SELECT COUNT(*) FROM likes WHERE item_id = i.id) as "likes_count!",
                  i.archive_id as "archive_id: _", i.archived_at as "archived_at: _"
           FROM items i
           JOIN users u ON u.id = i.created_by
           WHERE i.retro_id = (SELECT retro_id FROM items WHERE id = $1)"#,
        item_id
    )
    .fetch_all(&mut *conn)
    .await?;

    apply_author_initials(&mut [&mut items]);
    items
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or(sqlx::Error::RowNotFound)
}

async fn load_action_item(pool: &PgPool, action_item_id: i32) -> Result<ActionItem, sqlx::Error> {
    sqlx::query_as!(
        ActionItem,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!", created_at as "created_at!",
                  completed_at as "completed_at: _", archive_id as "archive_id: _", archived_at as "archived_at: _"
           FROM action_items WHERE id = $1"#,
        action_item_id
    )
    .fetch_one(pool)
    .await
}

fn forbidden(state: &AppState, message: &str) -> Response {
    let template = ErrorTemplate {
        code: "403",
        message: message.to_string(),
        demo_mode: state.config.demo_mode(),
    };
    (StatusCode::FORBIDDEN, Html(template.render().unwrap())).into_response()
}

fn not_found_response(state: &AppState, slug: &str) -> Response {
    let template = ErrorTemplate {
        code: "404",
        message: format!("No retrospective with slug '{}' found", slug),
        demo_mode: state.config.demo_mode(),
    };
    (StatusCode::NOT_FOUND, Html(template.render().unwrap())).into_response()
}

fn not_found_page(state: &AppState) -> Response {
    let template = ErrorTemplate {
        code: "404",
        message: "Page not found".to_string(),
        demo_mode: state.config.demo_mode(),
    };
    (StatusCode::NOT_FOUND, Html(template.render().unwrap())).into_response()
}

fn bad_request(state: &AppState, message: &str) -> Response {
    let template = ErrorTemplate {
        code: "400",
        message: message.to_string(),
        demo_mode: state.config.demo_mode(),
    };
    (StatusCode::BAD_REQUEST, Html(template.render().unwrap())).into_response()
}

async fn load_retro(pool: &PgPool, slug: &str) -> Result<Option<Retrospective>, sqlx::Error> {
    sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE slug = $1",
        slug
    )
    .fetch_optional(pool)
    .await
}

async fn require_retro_access(
    state: &AppState,
    user: &AuthUser,
    slug: &str,
) -> Result<Option<Retrospective>, Response> {
    let retro = match load_retro(&state.pool, slug).await {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(None),
        Err(error) => {
            log_database_error("load_retro_by_slug", &error);
            return Err(database_error_response());
        }
    };

    if user.is_admin || user.is_member_of_team(&retro.team_slug) {
        Ok(Some(retro))
    } else {
        Err(forbidden(
            state,
            "You do not have access to this retrospective",
        ))
    }
}

async fn require_retro_access_by_id(
    state: &AppState,
    user: &AuthUser,
    retro_id: i32,
) -> Result<Option<Retrospective>, Response> {
    let retro = match sqlx::query_as!(
        Retrospective,
        "SELECT * FROM retrospectives WHERE id = $1",
        retro_id
    )
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return Ok(None),
        Err(error) => {
            log_database_error("load_retro_by_id", &error);
            return Err(database_error_response());
        }
    };

    if user.is_admin || user.is_member_of_team(&retro.team_slug) {
        Ok(Some(retro))
    } else {
        Err(forbidden(
            state,
            "You do not have access to this retrospective",
        ))
    }
}

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
) -> Result<Html<String>, Response> {
    let retros = if user.is_admin {
        sqlx::query_as!(
            Retrospective,
            "SELECT * FROM retrospectives ORDER BY created_at DESC"
        )
        .fetch_all(&state.pool)
        .await
    } else {
        sqlx::query_as!(
            Retrospective,
            "SELECT * FROM retrospectives WHERE team_slug = ANY($1) ORDER BY created_at DESC",
            &user.team_slugs
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

pub async fn new_retro(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Html<String>, Response> {
    if !user.is_admin {
        return Err(forbidden(&state, "Only admins can create retrospectives"));
    }

    let teams = user
        .teams
        .iter()
        .map(|t| GitHubTeam {
            slug: t.slug.clone(),
            name: t.name.clone(),
        })
        .collect();

    let template = NewRetroTemplate {
        is_admin: user.is_admin,
        teams,
        demo_mode: state.config.demo_mode(),
        user: Some(user),
    };
    Ok(Html(template.render().unwrap()))
}

pub async fn create_retro(
    State(state): State<AppState>,
    user: AuthUser,
    Form(form): Form<NewRetro>,
) -> impl IntoResponse {
    if !user.is_admin {
        return forbidden(&state, "Only admins can create retrospectives");
    }

    if form.slug.is_empty() {
        return bad_request(&state, "Slug is required");
    }
    if form.slug.len() > 255 {
        return bad_request(&state, "Slug must be 255 characters or less");
    }
    if !form
        .slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return bad_request(
            &state,
            "Slug can only contain lowercase letters, numbers, and dashes",
        );
    }

    let team_slug = if state.config.demo_mode() {
        form.team_slug.unwrap_or_else(|| "demo".to_string())
    } else {
        match form.team_slug {
            Some(s) if !s.is_empty() => s,
            _ => return bad_request(&state, "Team is required"),
        }
    };

    let retro = match sqlx::query_as!(
        Retrospective,
        "INSERT INTO retrospectives (title, slug, team_slug, created_by) VALUES ($1, $2, $3, $4) RETURNING *",
        form.title,
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
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(
                        ErrorTemplate {
                            code: "500",
                            message: "Slug is already in use".to_string(),
                            demo_mode: state.config.demo_mode(),
                        }
                        .render()
                        .unwrap(),
                    ),
                )
                    .into_response();
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
) -> Result<impl IntoResponse, Response> {
    let retro = match require_retro_access(&state, &user, &slug).await? {
        Some(r) => r,
        None => return Ok(not_found_response(&state, &slug)),
    };

    let mut good_items = sqlx::query_as!(
        Item,
        r#"SELECT i.id as "id!", i.retro_id as "retro_id!", i.text as "text!",
                  i.category as "category: _", i.created_at as "created_at!", i.status as "status: _",
                  i.created_by as "author_id!", u.display_name as "author_name!",
                  ''::text as "author_initials!",
                  (SELECT COUNT(*) FROM likes WHERE item_id = i.id) as "likes_count!",
                  i.archive_id as "archive_id: _", i.archived_at as "archived_at: _"
           FROM items i
           JOIN users u ON u.id = i.created_by
           WHERE i.retro_id = $1
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

    let mut bad_items = sqlx::query_as!(
        Item,
        r#"SELECT i.id as "id!", i.retro_id as "retro_id!", i.text as "text!",
                  i.category as "category: _", i.created_at as "created_at!", i.status as "status: _",
                  i.created_by as "author_id!", u.display_name as "author_name!",
                  ''::text as "author_initials!",
                  (SELECT COUNT(*) FROM likes WHERE item_id = i.id) as "likes_count!",
                  i.archive_id as "archive_id: _", i.archived_at as "archived_at: _"
           FROM items i
           JOIN users u ON u.id = i.created_by
           WHERE i.retro_id = $1
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

    let mut watch_items = sqlx::query_as!(
        Item,
        r#"SELECT i.id as "id!", i.retro_id as "retro_id!", i.text as "text!",
                  i.category as "category: _", i.created_at as "created_at!", i.status as "status: _",
                  i.created_by as "author_id!", u.display_name as "author_name!",
                  ''::text as "author_initials!",
                  (SELECT COUNT(*) FROM likes WHERE item_id = i.id) as "likes_count!",
                  i.archive_id as "archive_id: _", i.archived_at as "archived_at: _"
           FROM items i
           JOIN users u ON u.id = i.created_by
           WHERE i.retro_id = $1
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

    let action_items = sqlx::query_as!(
        ActionItem,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!", created_at as "created_at!",
                  completed_at as "completed_at: _", archive_id as "archive_id: _", archived_at as "archived_at: _"
           FROM action_items
           WHERE retro_id = $1 AND archive_id IS NULL
           ORDER BY created_at ASC"#,
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
    };

    Ok(Html(template.render().unwrap()).into_response())
}

pub async fn add_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path((category, retro_id)): Path<(Category, i32)>,
    Form(form): Form<NewItem>,
) -> Result<Response, Response> {
    match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(_) => {}
        None => {
            return Err(not_found_response(&state, ""));
        }
    }

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("add_item_begin_transaction", &error);
        database_error_response()
    })?;

    let item_id = sqlx::query_scalar!(
        r#"INSERT INTO items (retro_id, text, category, status, created_by)
           VALUES ($1, $2, $3, 'CREATED'::status, $4)
           RETURNING id"#,
        retro_id,
        form.text,
        category as Category,
        user.user_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| {
        log_database_error("add_item", &error);
        database_error_response()
    })?;

    let item = load_item_with_initials(&mut tx, item_id)
        .await
        .map_err(|error| {
            log_database_error("load_added_item", &error);
            database_error_response()
        })?;

    tx.commit().await.map_err(|error| {
        log_database_error("add_item_commit_transaction", &error);
        database_error_response()
    })?;

    tracing::debug!(
        item_id = item.id,
        retro_id = item.retro_id,
        category = %item.category.to_string(),
        "item created"
    );

    let needs_initials_refresh = item.author_initials.chars().count() > 2;
    let template = ItemCardTemplate {
        item,
        error_message: None,
    };
    let html = Html(template.render().unwrap());

    if needs_initials_refresh {
        Ok((
            [(
                HeaderName::from_static("hx-refresh"),
                HeaderValue::from_static("true"),
            )],
            html,
        )
            .into_response())
    } else {
        Ok(html.into_response())
    }
}

pub async fn change_item_status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Html<String>, Response> {
    // Verify the item exists and the user has access to its retro before mutating.
    let retro_id = match sqlx::query_scalar!("SELECT retro_id FROM items WHERE id = $1", item_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(id)) => id,
        Ok(None) => return Err(not_found_response(&state, "")),
        Err(error) => {
            log_database_error("load_item_retro_id", &error);
            return Err(database_error_response());
        }
    };

    match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(_) => {}
        None => {
            return Err(forbidden(
                &state,
                "You do not have access to this retrospective",
            ));
        }
    }

    #[derive(sqlx::FromRow)]
    struct StatusChange {
        id: i32,
        old_status: Status,
        new_status: Status,
    }

    let action = params.get("action").map(|s| s.as_str());
    let status_change = match sqlx::query_as!(
        StatusChange,
        r#"
        UPDATE items
        SET status = CASE
            WHEN status = 'COMPLETED'::status THEN 'COMPLETED'::status
            WHEN status = 'CREATED'::status AND $2 = 'highlight' THEN 'HIGHLIGHTED'::status
            WHEN status = 'HIGHLIGHTED'::status AND $2 = 'complete' THEN 'COMPLETED'::status
            WHEN status = 'HIGHLIGHTED'::status AND $2 = 'cancel' THEN 'CREATED'::status
            ELSE status
        END
        WHERE id = $1
        RETURNING id, old.status as "old_status: _", new.status as "new_status: _"
        "#,
        item_id,
        action
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(row) => row,
        Err(e) => {
            if e.as_database_error()
                .and_then(|de| de.constraint())
                .is_some_and(|c| c.contains("single_highlighted_item_per_retro"))
            {
                // Fetch the original item so we can re-render it with the error message
                let mut conn = state.pool.acquire().await.map_err(|error| {
                    log_database_error("reload_item_after_highlight_conflict_acquire", &error);
                    database_error_response()
                })?;
                let original =
                    load_item_with_initials(&mut conn, item_id)
                        .await
                        .map_err(|error| {
                            log_database_error("reload_item_after_highlight_conflict", &error);
                            database_error_response()
                        })?;
                tracing::debug!(item_id, retro_id, "item highlight conflict");
                return Ok(Html(
                    ItemCardTemplate {
                        item: original,
                        error_message: Some(
                            "Only one item can be highlighted at a time".to_string(),
                        ),
                    }
                    .render()
                    .unwrap(),
                ));
            }
            log_database_error("change_item_status", &e);
            return Err(database_error_response());
        }
    };

    let mut conn = state.pool.acquire().await.map_err(|error| {
        log_database_error("load_updated_item_acquire", &error);
        database_error_response()
    })?;
    let item = load_item_with_initials(&mut conn, status_change.id)
        .await
        .map_err(|error| {
            log_database_error("load_updated_item", &error);
            database_error_response()
        })?;

    tracing::debug!(
        item_id = status_change.id,
        old_status = ?status_change.old_status,
        new_status = ?status_change.new_status,
        action = action.unwrap_or("missing"),
        "item status changed"
    );

    // Check if all items in this retro are completed
    let all_completed = sqlx::query_scalar!(
        r#"
        SELECT NOT EXISTS (
            SELECT 1 FROM items
            WHERE retro_id = $1
            AND archive_id IS NULL
            AND status != 'COMPLETED'::status
        )
        "#,
        item.retro_id
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("change_item_status_completion_check", &error);
        database_error_response()
    })?;

    tracing::debug!(
        item_id = item.id,
        retro_id = item.retro_id,
        action = action.unwrap_or("missing"),
        user_id = user.user_id,
        "item status change processed"
    );

    let template = if all_completed.unwrap_or(false) {
        ArchiveModalTemplate {
            item,
            error_message: None,
        }
        .render()
        .unwrap()
    } else {
        ItemCardTemplate {
            item,
            error_message: None,
        }
        .render()
        .unwrap()
    };

    Ok(Html(template))
}

pub async fn show_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
) -> Result<Html<String>, Response> {
    let mut conn = state.pool.acquire().await.map_err(|error| {
        log_database_error("load_item_acquire", &error);
        database_error_response()
    })?;
    let item = load_item_with_initials(&mut conn, item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => {
                log_database_error("load_item", &error);
                database_error_response()
            }
        })?;

    match require_retro_access_by_id(&state, &user, item.retro_id).await? {
        Some(_) => {}
        None => return Err(not_found_page(&state)),
    }

    Ok(Html(
        ItemCardTemplate {
            item,
            error_message: None,
        }
        .render()
        .unwrap(),
    ))
}

pub async fn edit_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
) -> Result<Html<String>, Response> {
    let mut conn = state.pool.acquire().await.map_err(|error| {
        log_database_error("load_item_for_edit_acquire", &error);
        database_error_response()
    })?;
    let item = load_item_with_initials(&mut conn, item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => {
                log_database_error("load_item_for_edit", &error);
                database_error_response()
            }
        })?;

    match require_retro_access_by_id(&state, &user, item.retro_id).await? {
        Some(_) => {}
        None => return Err(not_found_page(&state)),
    }

    Ok(Html(ItemEditTemplate { item }.render().unwrap()))
}

pub async fn update_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
    Form(form): Form<NewItem>,
) -> Result<Html<String>, Response> {
    let mut conn = state.pool.acquire().await.map_err(|error| {
        log_database_error("load_item_for_update_acquire", &error);
        database_error_response()
    })?;
    let item = load_item_with_initials(&mut conn, item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => {
                log_database_error("load_item_for_update", &error);
                database_error_response()
            }
        })?;

    match require_retro_access_by_id(&state, &user, item.retro_id).await? {
        Some(_) => {}
        None => return Err(not_found_page(&state)),
    }

    let text = form.text.trim();
    if text.is_empty() {
        return Err(bad_request(&state, "Card text is required"));
    }

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("update_item_begin_transaction", &error);
        database_error_response()
    })?;

    sqlx::query!("UPDATE items SET text = $1 WHERE id = $2", text, item_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            log_database_error("update_item", &error);
            database_error_response()
        })?;

    let item = load_item_with_initials(&mut tx, item_id)
        .await
        .map_err(|error| {
            log_database_error("load_updated_item_text", &error);
            database_error_response()
        })?;

    tx.commit().await.map_err(|error| {
        log_database_error("update_item_commit_transaction", &error);
        database_error_response()
    })?;

    tracing::debug!(item_id, user_id = user.user_id, "item text updated");

    Ok(Html(
        ItemCardTemplate {
            item,
            error_message: None,
        }
        .render()
        .unwrap(),
    ))
}

pub async fn like_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
) -> Result<Html<String>, Response> {
    let retro_id = match sqlx::query_scalar!("SELECT retro_id FROM items WHERE id = $1", item_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(id)) => id,
        Ok(None) => return Err(not_found_response(&state, "")),
        Err(error) => {
            log_database_error("load_item_retro_id_for_like", &error);
            return Err(database_error_response());
        }
    };

    match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(_) => {}
        None => {
            return Err(forbidden(
                &state,
                "You do not have access to this retrospective",
            ))
        }
    }

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("like_item_begin_transaction", &error);
        database_error_response()
    })?;

    let already_liked = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM likes WHERE item_id = $1 AND user_id = $2)"#,
        item_id,
        user.user_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| {
        log_database_error("check_existing_like", &error);
        database_error_response()
    })?
    .unwrap_or(false);

    if already_liked {
        sqlx::query!(
            r#"DELETE FROM likes WHERE item_id = $1 AND user_id = $2"#,
            item_id,
            user.user_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            log_database_error("delete_like", &error);
            database_error_response()
        })?;
    } else {
        sqlx::query!(
            r#"INSERT INTO likes (item_id, user_id) VALUES ($1, $2)"#,
            item_id,
            user.user_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            log_database_error("insert_like", &error);
            database_error_response()
        })?;
    }

    let item = load_item_with_initials(&mut tx, item_id)
        .await
        .map_err(|error| {
            log_database_error("load_item_after_like", &error);
            database_error_response()
        })?;

    tx.commit().await.map_err(|error| {
        log_database_error("like_item_commit_transaction", &error);
        database_error_response()
    })?;

    tracing::debug!(
        item_id,
        retro_id,
        user_id = user.user_id,
        liked = !already_liked,
        "item like toggled"
    );

    Ok(Html(
        ItemCardTemplate {
            item,
            error_message: None,
        }
        .render()
        .unwrap(),
    ))
}

pub async fn add_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(retro_id): Path<i32>,
    Form(form): Form<NewActionItem>,
) -> Result<Html<String>, Response> {
    match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(_) => {}
        None => return Err(not_found_response(&state, "")),
    }

    let text = form.text.trim();
    if text.is_empty() {
        return Err(bad_request(&state, "Action item text is required"));
    }

    let action_item = sqlx::query_as!(
        ActionItem,
        r#"INSERT INTO action_items (retro_id, text)
           VALUES ($1, $2)
           RETURNING id as "id!", retro_id as "retro_id!", text as "text!", created_at as "created_at!",
                     completed_at as "completed_at: _", archive_id as "archive_id: _", archived_at as "archived_at: _""#,
        retro_id,
        text
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("add_action_item", &error);
        database_error_response()
    })?;

    Ok(Html(ActionItemTemplate { action_item }.render().unwrap()))
}

pub async fn show_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(action_item_id): Path<i32>,
) -> Result<Html<String>, Response> {
    let action_item = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => database_error_response(),
        })?;
    require_retro_access_by_id(&state, &user, action_item.retro_id)
        .await?
        .ok_or_else(|| not_found_page(&state))?;
    Ok(Html(ActionItemTemplate { action_item }.render().unwrap()))
}

pub async fn edit_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(action_item_id): Path<i32>,
) -> Result<Html<String>, Response> {
    let action_item = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => database_error_response(),
        })?;
    require_retro_access_by_id(&state, &user, action_item.retro_id)
        .await?
        .ok_or_else(|| not_found_page(&state))?;
    Ok(Html(
        ActionItemEditTemplate { action_item }.render().unwrap(),
    ))
}

pub async fn update_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(action_item_id): Path<i32>,
    Form(form): Form<NewActionItem>,
) -> Result<Html<String>, Response> {
    let existing = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => database_error_response(),
        })?;
    require_retro_access_by_id(&state, &user, existing.retro_id)
        .await?
        .ok_or_else(|| not_found_page(&state))?;

    let text = form.text.trim();
    if text.is_empty() {
        return Err(bad_request(&state, "Action item text is required"));
    }
    sqlx::query!(
        "UPDATE action_items SET text = $1 WHERE id = $2",
        text,
        action_item_id
    )
    .execute(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("update_action_item", &error);
        database_error_response()
    })?;
    let action_item = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|_| database_error_response())?;
    Ok(Html(ActionItemTemplate { action_item }.render().unwrap()))
}

pub async fn complete_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(action_item_id): Path<i32>,
) -> Result<Html<String>, Response> {
    let existing = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => database_error_response(),
        })?;
    require_retro_access_by_id(&state, &user, existing.retro_id)
        .await?
        .ok_or_else(|| not_found_page(&state))?;
    sqlx::query!(
        "UPDATE action_items SET completed_at = COALESCE(completed_at, NOW()) WHERE id = $1",
        action_item_id
    )
    .execute(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("complete_action_item", &error);
        database_error_response()
    })?;
    let action_item = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|_| database_error_response())?;
    Ok(Html(ActionItemTemplate { action_item }.render().unwrap()))
}

pub async fn delete_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(action_item_id): Path<i32>,
) -> Result<StatusCode, Response> {
    let existing = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => database_error_response(),
        })?;
    require_retro_access_by_id(&state, &user, existing.retro_id)
        .await?
        .ok_or_else(|| not_found_page(&state))?;
    sqlx::query!("DELETE FROM action_items WHERE id = $1", action_item_id)
        .execute(&state.pool)
        .await
        .map_err(|error| {
            log_database_error("delete_action_item", &error);
            database_error_response()
        })?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn archive_retro(
    State(state): State<AppState>,
    user: AuthUser,
    Path(retro_id): Path<i32>,
) -> Result<impl IntoResponse, Response> {
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
) -> impl IntoResponse {
    if !user.is_admin {
        return forbidden(&state, "Only admins can delete retrospectives");
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
) -> Result<impl IntoResponse, Response> {
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
) -> Result<impl IntoResponse, Response> {
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
        Ok(None) => return Err(not_found_page(&state)),
        Err(error) => {
            log_database_error("show_archive", &error);
            return Err(database_error_response());
        }
    };

    let mut good_items = sqlx::query_as!(
        Item,
        r#"SELECT i.id as "id!", i.retro_id as "retro_id!", i.text as "text!",
                  i.category as "category: _", i.created_at as "created_at!", i.status as "status: _",
                  i.created_by as "author_id!", u.display_name as "author_name!",
                  ''::text as "author_initials!",
                  (SELECT COUNT(*) FROM likes WHERE item_id = i.id) as "likes_count!",
                  i.archive_id as "archive_id: _", i.archived_at as "archived_at: _"
           FROM items i
           JOIN users u ON u.id = i.created_by
           WHERE i.archive_id = $1
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

    let mut bad_items = sqlx::query_as!(
        Item,
        r#"SELECT i.id as "id!", i.retro_id as "retro_id!", i.text as "text!",
                  i.category as "category: _", i.created_at as "created_at!", i.status as "status: _",
                  i.created_by as "author_id!", u.display_name as "author_name!",
                  ''::text as "author_initials!",
                  (SELECT COUNT(*) FROM likes WHERE item_id = i.id) as "likes_count!",
                  i.archive_id as "archive_id: _", i.archived_at as "archived_at: _"
           FROM items i
           JOIN users u ON u.id = i.created_by
           WHERE i.archive_id = $1
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

    let mut watch_items = sqlx::query_as!(
        Item,
        r#"SELECT i.id as "id!", i.retro_id as "retro_id!", i.text as "text!",
                  i.category as "category: _", i.created_at as "created_at!", i.status as "status: _",
                  i.created_by as "author_id!", u.display_name as "author_name!",
                  ''::text as "author_initials!",
                  (SELECT COUNT(*) FROM likes WHERE item_id = i.id) as "likes_count!",
                  i.archive_id as "archive_id: _", i.archived_at as "archived_at: _"
           FROM items i
           JOIN users u ON u.id = i.created_by
           WHERE i.archive_id = $1
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

    let action_items = sqlx::query_as!(
        ActionItem,
        r#"SELECT id as "id!", retro_id as "retro_id!", text as "text!", created_at as "created_at!",
                  completed_at as "completed_at: _", archive_id as "archive_id: _", archived_at as "archived_at: _"
           FROM action_items WHERE archive_id = $1 ORDER BY created_at ASC"#,
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

pub async fn not_found(State(state): State<AppState>, _user: MaybeAuthUser) -> impl IntoResponse {
    not_found_page(&state)
}

#[derive(Deserialize)]
pub struct NewRetro {
    title: String,
    slug: String,
    team_slug: Option<String>,
}

#[derive(Deserialize)]
pub struct NewItem {
    text: String,
}

#[derive(Deserialize)]
pub struct NewActionItem {
    text: String,
}
