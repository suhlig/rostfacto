//! Handlers for item cards: create, edit, like, and status changes.

use super::access::{load_item_with_initials, require_retro_access_by_id};
use super::emit::{attach_event_id_header, emit_event};
use super::error::{forbidden, not_found_page, not_found_response, validation_error};
use super::{database_error_response, log_database_error, HandlerError, MAX_ITEM_TEXT_LENGTH};
use crate::auth::AuthUser;
use crate::events::EventType;
use crate::models::{Category, Status};
use crate::templates::{ArchiveModalTemplate, ItemCardTemplate, ItemEditTemplate};
use crate::AppState;
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::{header::HeaderName, HeaderMap, HeaderValue},
    response::{Html, IntoResponse, Response},
    Form,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

pub async fn add_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path((category, retro_id)): Path<(Category, i32)>,
    headers: HeaderMap,
    Form(form): Form<NewItem>,
) -> Result<Response, HandlerError> {
    match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(_) => {}
        None => {
            return Err(not_found_response(&state, "").into());
        }
    }

    let text = form.text.trim();
    if text.is_empty() {
        return Err(validation_error(&state, &headers, "Card text is required").into());
    }
    if text.chars().count() > MAX_ITEM_TEXT_LENGTH {
        return Err(validation_error(
            &state,
            &headers,
            &format!("Card text must be {MAX_ITEM_TEXT_LENGTH} characters or less"),
        )
        .into());
    }

    // Rendered before the INSERT so `category` can be moved into the query;
    // the payload carries the same uppercase label the DB enum uses.
    let category_label = category.to_string();

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("add_item_begin_transaction", &error);
        database_error_response()
    })?;

    let item_id = sqlx::query_scalar!(
        r#"INSERT INTO items (retro_id, text, category, status, created_by)
           VALUES ($1, $2, $3, 'CREATED'::status, $4)
           RETURNING id"#,
        retro_id,
        text,
        category as Category,
        user.user_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| {
        log_database_error("add_item", &error);
        database_error_response()
    })?;

    // The new card gets exactly one ITEM_CREATED event, written like every
    // mutation's event in the same transaction as the INSERT.
    let event_id = emit_event(
        &mut tx,
        retro_id,
        EventType::ItemCreated as EventType,
        Some(item_id),
        json!({
            "item_id": item_id,
            "retro_id": retro_id,
            "category": category_label,
            "text": text,
            "status": "CREATED",
            "likes_count": 0,
            "author_name": user.full_name,
        }),
    )
    .await
    .map_err(|error| {
        log_database_error("add_item_emit_event", &error);
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

    let mut response = if needs_initials_refresh {
        (
            [(
                HeaderName::from_static("hx-refresh"),
                HeaderValue::from_static("true"),
            )],
            html,
        )
            .into_response()
    } else {
        html.into_response()
    };
    attach_event_id_header(&mut response, Some(event_id));
    Ok(response)
}

pub async fn change_item_status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, HandlerError> {
    // Verify the item exists and the user has access to its retro before mutating.
    let retro_id = match sqlx::query_scalar!("SELECT retro_id FROM items WHERE id = $1", item_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(id)) => id,
        Ok(None) => return Err(not_found_response(&state, "").into()),
        Err(error) => {
            log_database_error("load_item_retro_id", &error);
            return Err(database_error_response().into());
        }
    };

    match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(_) => {}
        None => {
            return Err(forbidden(&state, "You do not have access to this retrospective").into());
        }
    }

    #[derive(sqlx::FromRow)]
    struct StatusChange {
        id: i32,
        old_status: Status,
        new_status: Status,
    }

    let action = params.get("action").map(|s| s.as_str());
    // Wrap the UPDATE and the events lookup in one transaction so the header
    // reflects the event this mutation produced (or nothing for a no-op
    // status change).
    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("change_item_status_begin_transaction", &error);
        database_error_response()
    })?;
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
        END,
        -- Completing or cancelling a highlight ends its timer: reset the timer
        -- columns in the same UPDATE (the handler emits ITEM_STATUS_CHANGED,
        -- never TIMER_CANCELLED). The condition on the old status keeps a
        -- no-op status change from touching the timer.
        timer_started_at = CASE
            WHEN $2 IN ('cancel', 'complete') AND status = 'HIGHLIGHTED'::status THEN NULL
            ELSE timer_started_at
        END,
        timer_duration_seconds = CASE
            WHEN $2 IN ('cancel', 'complete') AND status = 'HIGHLIGHTED'::status THEN NULL
            ELSE timer_duration_seconds
        END,
        timer_elapsed_at = CASE
            WHEN $2 IN ('cancel', 'complete') AND status = 'HIGHLIGHTED'::status THEN NULL
            ELSE timer_elapsed_at
        END
        WHERE id = $1
        RETURNING id, old.status as "old_status: _", new.status as "new_status: _"
        "#,
        item_id,
        action
    )
    .fetch_one(&mut *tx)
    .await
    {
        Ok(row) => row,
        Err(e) => {
            if e.as_database_error()
                .and_then(|de| de.constraint())
                .is_some_and(|c| c.contains("single_highlighted_item_per_retro"))
            {
                // Fetch the original item so we can re-render it with the error message
                drop(tx);
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
                )
                .into_response());
            }
            log_database_error("change_item_status", &e);
            return Err(database_error_response().into());
        }
    };

    // Only an actual status change emits an event. A no-op stays quiet so no
    // X-Event-Id is attached: attaching a stale id would wrongly suppress a
    // future SSE update for this item.
    let event_id = if status_change.old_status != status_change.new_status {
        Some(
            emit_event(
                &mut tx,
                retro_id,
                EventType::ItemStatusChanged as EventType,
                Some(status_change.id),
                json!({
                    "item_id": status_change.id,
                    "old_status": status_change.old_status.to_string(),
                    "new_status": status_change.new_status.to_string(),
                }),
            )
            .await
            .map_err(|error| {
                log_database_error("change_item_status_emit_event", &error);
                database_error_response()
            })?,
        )
    } else {
        None
    };

    tx.commit().await.map_err(|error| {
        log_database_error("change_item_status_commit_transaction", &error);
        database_error_response()
    })?;

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

    let mut response = Html(template).into_response();
    attach_event_id_header(&mut response, event_id);
    Ok(response)
}

pub async fn show_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
) -> Result<Html<String>, HandlerError> {
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
        None => return Err(not_found_page(&state).into()),
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
) -> Result<Html<String>, HandlerError> {
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
        None => return Err(not_found_page(&state).into()),
    }

    Ok(Html(ItemEditTemplate { item }.render().unwrap()))
}

pub async fn update_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
    headers: HeaderMap,
    Form(form): Form<NewItem>,
) -> Result<Response, HandlerError> {
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
        None => return Err(not_found_page(&state).into()),
    }

    let text = form.text.trim();
    if text.is_empty() {
        return Err(validation_error(&state, &headers, "Card text is required").into());
    }
    if text.chars().count() > MAX_ITEM_TEXT_LENGTH {
        return Err(validation_error(
            &state,
            &headers,
            &format!("Card text must be {MAX_ITEM_TEXT_LENGTH} characters or less"),
        )
        .into());
    }
    let old_text = item.text.clone();

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

    // Only a real text change emits ITEM_UPDATED (matching the header, which
    // must not suppress a future event with a stale id).
    let event_id = if old_text != text {
        Some(
            emit_event(
                &mut tx,
                item.retro_id,
                EventType::ItemUpdated as EventType,
                Some(item_id),
                json!({"item_id": item_id, "text": text}),
            )
            .await
            .map_err(|error| {
                log_database_error("update_item_emit_event", &error);
                database_error_response()
            })?,
        )
    } else {
        None
    };

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

    let mut response = Html(
        ItemCardTemplate {
            item,
            error_message: None,
        }
        .render()
        .unwrap(),
    )
    .into_response();
    attach_event_id_header(&mut response, event_id);
    Ok(response)
}

pub async fn like_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
) -> Result<Response, HandlerError> {
    let retro_id = match sqlx::query_scalar!("SELECT retro_id FROM items WHERE id = $1", item_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(id)) => id,
        Ok(None) => return Err(not_found_response(&state, "").into()),
        Err(error) => {
            log_database_error("load_item_retro_id_for_like", &error);
            return Err(database_error_response().into());
        }
    };

    match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(_) => {}
        None => {
            return Err(forbidden(&state, "You do not have access to this retrospective").into())
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

    // Recompute the like count after the toggle, in the same transaction,
    // mirroring the count the trigger used to send in the payload.
    let event_type = if already_liked {
        EventType::ItemUnliked
    } else {
        EventType::ItemLiked
    };
    let likes_count = sqlx::query_scalar!("SELECT COUNT(*) FROM likes WHERE item_id = $1", item_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| {
            log_database_error("like_item_count", &error);
            database_error_response()
        })?
        .unwrap_or(0);
    let event_id = emit_event(
        &mut tx,
        retro_id,
        event_type as EventType,
        Some(item_id),
        json!({"item_id": item_id, "likes_count": likes_count}),
    )
    .await
    .map_err(|error| {
        log_database_error("like_item_emit_event", &error);
        database_error_response()
    })?;

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

    let mut response = Html(
        ItemCardTemplate {
            item,
            error_message: None,
        }
        .render()
        .unwrap(),
    )
    .into_response();
    attach_event_id_header(&mut response, Some(event_id));
    Ok(response)
}

#[derive(Deserialize)]
pub struct NewItem {
    text: String,
}
