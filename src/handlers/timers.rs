//! The server-authoritative highlight timer: start, extend, and the sweep
//! that marks elapsed timers.

use super::access::{load_item_with_initials, require_retro_access_by_id};
use super::emit::{attach_event_id_header, emit_event};
use super::error::{forbidden, not_found_response};
use super::{database_error_response, log_database_error, HandlerError};
use crate::auth::AuthUser;
use crate::events::EventType;
use crate::templates::ItemCardTemplate;
use crate::AppState;
use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
    Form,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct TimerStartForm {
    pub duration: Option<i32>,
}

/// Timer columns read back from a timer UPDATE's RETURNING clause. The
/// deadline is derived exactly like the DB's `timer_ends_at` generated column
/// (`timer_started_at + timer_duration_seconds`), so event payloads and card
/// renders always agree.
#[derive(sqlx::FromRow)]
struct TimerRow {
    timer_started_at: chrono::DateTime<chrono::Utc>,
    timer_duration_seconds: i32,
}

impl TimerRow {
    fn ends_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.timer_started_at + chrono::Duration::seconds(self.timer_duration_seconds as i64)
    }
}

/// Verify the item exists and the user has access to its retro.
async fn require_timer_access(
    state: &AppState,
    user: &AuthUser,
    item_id: i32,
) -> Result<i32, HandlerError> {
    let retro_id = match sqlx::query_scalar!("SELECT retro_id FROM items WHERE id = $1", item_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(id)) => id,
        Ok(None) => return Err(not_found_response(state, "").into()),
        Err(error) => {
            log_database_error("load_item_retro_id_for_timer", &error);
            return Err(database_error_response().into());
        }
    };

    match require_retro_access_by_id(state, user, retro_id).await? {
        Some(_) => Ok(retro_id),
        None => Err(forbidden(state, "You do not have access to this retrospective").into()),
    }
}

/// Start the highlight timer for an item. The deadline is computed by the DB
/// (`timer_ends_at`), so every client shows the same countdown.
pub async fn start_item_timer(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
    Form(form): Form<TimerStartForm>,
) -> Result<Response, HandlerError> {
    let retro_id = require_timer_access(&state, &user, item_id).await?;

    let duration = form.duration.unwrap_or(300).clamp(1, 3600);

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("start_timer_begin_transaction", &error);
        database_error_response()
    })?;

    let started = sqlx::query_as!(
        TimerRow,
        r#"UPDATE items
           SET timer_started_at = NOW(),
               timer_duration_seconds = $2,
               timer_elapsed_at = NULL
           WHERE id = $1 AND status = 'HIGHLIGHTED'::status
           RETURNING timer_started_at as "timer_started_at!", timer_duration_seconds as "timer_duration_seconds!""#,
        item_id,
        duration
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| {
        log_database_error("start_item_timer", &error);
        database_error_response()
    })?;

    let event_id = match started {
        Some(row) => Some(
            emit_event(
                &mut tx,
                retro_id,
                EventType::TimerStarted as EventType,
                Some(item_id),
                json!({
                    "item_id": item_id,
                    "duration_seconds": row.timer_duration_seconds,
                    "started_at": row.timer_started_at,
                    "ends_at": row.ends_at(),
                }),
            )
            .await
            .map_err(|error| {
                log_database_error("start_timer_emit_event", &error);
                database_error_response()
            })?,
        ),
        None => None,
    };

    let item = load_item_with_initials(&mut tx, item_id)
        .await
        .map_err(|error| {
            log_database_error("load_item_after_timer_start", &error);
            database_error_response()
        })?;

    tx.commit().await.map_err(|error| {
        log_database_error("start_timer_commit_transaction", &error);
        database_error_response()
    })?;

    tracing::debug!(item_id, duration, user_id = user.user_id, "timer started");

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

/// Extend a running timer by two minutes (restarting it if it already
/// elapsed). The new deadline is `GREATEST(old_deadline, NOW()) + 2 minutes`,
/// so the added time always counts from the moment the button is pressed: an
/// extend of a timer that has been over for longer than two minutes yields a
/// fresh countdown instead of leaving the deadline in the past (which the
/// sweep would immediately mark elapsed again).
pub async fn extend_item_timer(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<i32>,
) -> Result<Response, HandlerError> {
    let retro_id = require_timer_access(&state, &user, item_id).await?;

    let mut tx = state.pool.begin().await.map_err(|error| {
        log_database_error("extend_timer_begin_transaction", &error);
        database_error_response()
    })?;

    let extended = sqlx::query_as!(
        TimerRow,
        // `timer_ends_at` is generated as `timer_started_at +
        // timer_duration_seconds`, so a plain `duration + 120` cannot move the
        // deadline past NOW() once the timer is more than two minutes overdue
        // (the sweep re-marks it elapsed at once). Compare the derived deadline
        // against NOW() and, when it has already passed, restart the countdown
        // from NOW(): the added time runs from the button press, never from a
        // deadline that is already in the past. A still-running timer keeps its
        // start (and accumulates its duration), so the two behave identically on
        // the wire (the deadline is `GREATEST(old_deadline, NOW()) + 2 min`).
        r#"UPDATE items
           SET timer_started_at = CASE
                   WHEN timer_started_at + (timer_duration_seconds * INTERVAL '1 second') <= NOW()
                       THEN NOW()
                   ELSE timer_started_at
               END,
               timer_duration_seconds = CASE
                   WHEN timer_started_at + (timer_duration_seconds * INTERVAL '1 second') <= NOW()
                       THEN 120
                   ELSE timer_duration_seconds + 120
               END,
               timer_elapsed_at = NULL
           WHERE id = $1 AND timer_started_at IS NOT NULL
           RETURNING timer_started_at as "timer_started_at!", timer_duration_seconds as "timer_duration_seconds!""#,
        item_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|error| {
        log_database_error("extend_item_timer", &error);
        database_error_response()
    })?;

    let event_id = match extended {
        Some(row) => Some(
            emit_event(
                &mut tx,
                retro_id,
                EventType::TimerExtended as EventType,
                Some(item_id),
                json!({
                    "item_id": item_id,
                    "duration_seconds": row.timer_duration_seconds,
                    "started_at": row.timer_started_at,
                    "ends_at": row.ends_at(),
                }),
            )
            .await
            .map_err(|error| {
                log_database_error("extend_timer_emit_event", &error);
                database_error_response()
            })?,
        ),
        None => None,
    };

    let item = load_item_with_initials(&mut tx, item_id)
        .await
        .map_err(|error| {
            log_database_error("load_item_after_timer_extend", &error);
            database_error_response()
        })?;

    tx.commit().await.map_err(|error| {
        log_database_error("extend_timer_commit_transaction", &error);
        database_error_response()
    })?;

    tracing::debug!(item_id, user_id = user.user_id, "timer extended");

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

/// Background task: marks highlight timers as elapsed once their deadline
/// passes, so all clients see 0:00 and the +2 min button at the same time.
/// Runs every second; the idempotent UPDATE makes concurrent sweeps (e.g.
/// multiple app instances) safe.
pub async fn timer_sweep_loop(pool: PgPool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        if let Err(error) = sweep_elapsed_timers(&pool).await {
            log_database_error("timer_sweep", &error);
        }
    }
}

/// Mark overdue highlight timers elapsed and emit one TIMER_ELAPSED event per
/// item, all in one transaction so the mutation and its events commit
/// together. Idempotent: a second run (e.g. another app instance) matches no
/// rows because the first run set `timer_elapsed_at`.
async fn sweep_elapsed_timers(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let swept = sqlx::query_as!(
        SweptTimer,
        r#"UPDATE items
           SET timer_elapsed_at = NOW()
           WHERE status = 'HIGHLIGHTED'::status
             AND timer_ends_at <= NOW()
             AND timer_elapsed_at IS NULL
           RETURNING id as "id!", retro_id as "retro_id!""#
    )
    .fetch_all(&mut *tx)
    .await?;

    for row in &swept {
        emit_event(
            &mut tx,
            row.retro_id,
            EventType::TimerElapsed as EventType,
            Some(row.id),
            json!({"item_id": row.id}),
        )
        .await?;
    }

    if !swept.is_empty() {
        tracing::debug!(count = swept.len(), "highlight timers marked elapsed");
    }

    tx.commit().await?;
    Ok(())
}

/// A row swept by the timer sweep; carries the retro for the TIMER_ELAPSED
/// event's NOTIFY.
#[derive(sqlx::FromRow)]
struct SweptTimer {
    id: i32,
    retro_id: i32,
}
