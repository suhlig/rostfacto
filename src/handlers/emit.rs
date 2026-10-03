//! Writing durable events and surfacing their ids to clients.

use crate::events::EventType;
use axum::{http::HeaderValue, response::Response};

/// Attach the id of the event a mutation produced (if any) to its response,
/// so the client can ignore the matching SSE event and avoid double-applying
/// its own change. Callers pass `None` when the mutation emitted no event
/// (e.g. a no-op status change).
pub(super) fn attach_event_id_header(response: &mut Response, event_id: Option<i64>) {
    if let Some(id) = event_id {
        if let Ok(value) = HeaderValue::from_str(&id.to_string()) {
            response.headers_mut().insert("x-event-id", value);
        }
    }
}

/// Write one `events` row and wake every app instance's notifier, inside the
/// same transaction as the mutation it describes. The returned id is surfaced
/// as the `X-Event-Id` response header so the mutating client can ignore the
/// matching SSE event. Running the `pg_notify` in the transaction means the
/// notification is only delivered if the mutation commits.
pub(super) async fn emit_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    retro_id: i32,
    event_type: EventType,
    item_id: Option<i32>,
    payload: serde_json::Value,
) -> Result<i64, sqlx::Error> {
    let event_id = sqlx::query_scalar!(
        r#"INSERT INTO events (retro_id, event_type, item_id, payload)
           VALUES ($1, $2, $3, $4)
           RETURNING id"#,
        retro_id,
        event_type as EventType,
        item_id,
        payload
    )
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query!(
        "SELECT pg_notify('rostfacto_events', $1)",
        retro_id.to_string()
    )
    .execute(&mut **tx)
    .await?;

    Ok(event_id)
}
