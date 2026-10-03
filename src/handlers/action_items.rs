//! Handlers for action items: create, edit, complete, and delete.

use super::access::{load_action_item, require_retro_access_by_id};
use super::emit::{attach_event_id_header, emit_event};
use super::error::{not_found_page, not_found_response, validation_error};
use super::{database_error_response, log_database_error, HandlerError, MAX_ITEM_TEXT_LENGTH};
use crate::auth::AuthUser;
use crate::events::EventType;
use crate::templates::{ActionItemEditTemplate, ActionItemTemplate};
use crate::AppState;
use askama::Template;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use serde::Deserialize;
use serde_json::json;

pub async fn add_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(retro_id): Path<i32>,
    headers: HeaderMap,
    Form(form): Form<NewActionItem>,
) -> Result<Response, HandlerError> {
    match require_retro_access_by_id(&state, &user, retro_id).await? {
        Some(_) => {}
        None => return Err(not_found_response(&state, "").into()),
    }

    let text = form.text.trim();
    if text.is_empty() {
        return Err(validation_error(&state, &headers, "Action item text is required").into());
    }
    if text.chars().count() > MAX_ITEM_TEXT_LENGTH {
        return Err(validation_error(
            &state,
            &headers,
            &format!("Action item text must be {MAX_ITEM_TEXT_LENGTH} characters or less"),
        )
        .into());
    }

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("add_action_item_begin_transaction", &error);
        database_error_response()
    })?;

    let action_item = action_item_query_as!(
        "INSERT INTO action_items (retro_id, text) VALUES ($1, $2) RETURNING ",
        "",
        retro_id,
        text
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| {
        log_database_error("add_action_item", &error);
        database_error_response()
    })?;

    // The new action item gets exactly one ACTION_ITEM_CREATED event, written
    // in the same transaction as the INSERT so every client syncs it.
    let event_id = emit_event(
        &mut tx,
        retro_id,
        EventType::ActionItemCreated as EventType,
        Some(action_item.id),
        json!({
            "action_item_id": action_item.id,
            "retro_id": retro_id,
            "text": action_item.text,
            "completed": false,
        }),
    )
    .await
    .map_err(|error| {
        log_database_error("add_action_item_emit_event", &error);
        database_error_response()
    })?;

    tx.commit().await.map_err(|error| {
        log_database_error("add_action_item_commit_transaction", &error);
        database_error_response()
    })?;

    let mut response = Html(ActionItemTemplate { action_item }.render().unwrap()).into_response();
    attach_event_id_header(&mut response, Some(event_id));
    Ok(response)
}

pub async fn show_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(action_item_id): Path<i32>,
) -> Result<Html<String>, HandlerError> {
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
) -> Result<Html<String>, HandlerError> {
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
    headers: HeaderMap,
    Form(form): Form<NewActionItem>,
) -> Result<Response, HandlerError> {
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
        return Err(validation_error(&state, &headers, "Action item text is required").into());
    }
    if text.chars().count() > MAX_ITEM_TEXT_LENGTH {
        return Err(validation_error(
            &state,
            &headers,
            &format!("Action item text must be {MAX_ITEM_TEXT_LENGTH} characters or less"),
        )
        .into());
    }

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("update_action_item_begin_transaction", &error);
        database_error_response()
    })?;

    sqlx::query!(
        "UPDATE action_items SET text = $1 WHERE id = $2",
        text,
        action_item_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        log_database_error("update_action_item", &error);
        database_error_response()
    })?;

    // Only a real text change emits ACTION_ITEM_UPDATED (matching the header,
    // which must not suppress a future event with a stale id).
    let event_id = if existing.text != text {
        Some(
            emit_event(
                &mut tx,
                existing.retro_id,
                EventType::ActionItemUpdated as EventType,
                Some(action_item_id),
                json!({"action_item_id": action_item_id, "text": text}),
            )
            .await
            .map_err(|error| {
                log_database_error("update_action_item_emit_event", &error);
                database_error_response()
            })?,
        )
    } else {
        None
    };

    tx.commit().await.map_err(|error| {
        log_database_error("update_action_item_commit_transaction", &error);
        database_error_response()
    })?;

    let action_item = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|_| database_error_response())?;

    let mut response = Html(ActionItemTemplate { action_item }.render().unwrap()).into_response();
    attach_event_id_header(&mut response, event_id);
    Ok(response)
}

pub async fn complete_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(action_item_id): Path<i32>,
) -> Result<Response, HandlerError> {
    let existing = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => database_error_response(),
        })?;
    require_retro_access_by_id(&state, &user, existing.retro_id)
        .await?
        .ok_or_else(|| not_found_page(&state))?;

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("complete_action_item_begin_transaction", &error);
        database_error_response()
    })?;

    sqlx::query!(
        "UPDATE action_items SET completed_at = COALESCE(completed_at, NOW()) WHERE id = $1",
        action_item_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        log_database_error("complete_action_item", &error);
        database_error_response()
    })?;

    // Completing an already-completed item is a no-op, so it emits nothing.
    let event_id = if existing.completed_at.is_none() {
        Some(
            emit_event(
                &mut tx,
                existing.retro_id,
                EventType::ActionItemCompleted as EventType,
                Some(action_item_id),
                json!({"action_item_id": action_item_id, "completed": true}),
            )
            .await
            .map_err(|error| {
                log_database_error("complete_action_item_emit_event", &error);
                database_error_response()
            })?,
        )
    } else {
        None
    };

    tx.commit().await.map_err(|error| {
        log_database_error("complete_action_item_commit_transaction", &error);
        database_error_response()
    })?;

    let action_item = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|_| database_error_response())?;

    let mut response = Html(ActionItemTemplate { action_item }.render().unwrap()).into_response();
    attach_event_id_header(&mut response, event_id);
    Ok(response)
}

pub async fn delete_action_item(
    State(state): State<AppState>,
    user: AuthUser,
    Path(action_item_id): Path<i32>,
) -> Result<Response, HandlerError> {
    let existing = load_action_item(&state.pool, action_item_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => not_found_page(&state),
            _ => database_error_response(),
        })?;
    require_retro_access_by_id(&state, &user, existing.retro_id)
        .await?
        .ok_or_else(|| not_found_page(&state))?;

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("delete_action_item_begin_transaction", &error);
        database_error_response()
    })?;

    sqlx::query!("DELETE FROM action_items WHERE id = $1", action_item_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            log_database_error("delete_action_item", &error);
            database_error_response()
        })?;

    let event_id = emit_event(
        &mut tx,
        existing.retro_id,
        EventType::ActionItemDeleted as EventType,
        Some(action_item_id),
        json!({"action_item_id": action_item_id}),
    )
    .await
    .map_err(|error| {
        log_database_error("delete_action_item_emit_event", &error);
        database_error_response()
    })?;

    tx.commit().await.map_err(|error| {
        log_database_error("delete_action_item_commit_transaction", &error);
        database_error_response()
    })?;

    let mut response = StatusCode::OK.into_response();
    attach_event_id_header(&mut response, Some(event_id));
    Ok(response)
}

#[derive(Deserialize)]
pub struct NewActionItem {
    text: String,
}
