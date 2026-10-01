use crate::auth::AuthUser;
use crate::handlers::{
    database_error_response, log_database_error, not_found_response, require_retro_access,
};
use crate::presence::{ParticipantKey, PresenceHub, Roster};
use crate::AppState;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    types::Json,
    {FromRow, PgPool},
};
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

/// Event types written to the `events` table by DB triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "event_type", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventType {
    ItemCreated,
    ItemUpdated,
    ItemStatusChanged,
    ItemLiked,
    ItemUnliked,
    TimerStarted,
    TimerExtended,
    TimerCancelled,
    TimerElapsed,
    RetroArchived,
    ActionItemCreated,
    ActionItemUpdated,
    ActionItemCompleted,
    ActionItemDeleted,
}

impl Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            EventType::ItemCreated => "ITEM_CREATED",
            EventType::ItemUpdated => "ITEM_UPDATED",
            EventType::ItemStatusChanged => "ITEM_STATUS_CHANGED",
            EventType::ItemLiked => "ITEM_LIKED",
            EventType::ItemUnliked => "ITEM_UNLIKED",
            EventType::TimerStarted => "TIMER_STARTED",
            EventType::TimerExtended => "TIMER_EXTENDED",
            EventType::TimerCancelled => "TIMER_CANCELLED",
            EventType::TimerElapsed => "TIMER_ELAPSED",
            EventType::RetroArchived => "RETRO_ARCHIVED",
            EventType::ActionItemCreated => "ACTION_ITEM_CREATED",
            EventType::ActionItemUpdated => "ACTION_ITEM_UPDATED",
            EventType::ActionItemCompleted => "ACTION_ITEM_COMPLETED",
            EventType::ActionItemDeleted => "ACTION_ITEM_DELETED",
        };
        write!(f, "{}", name)
    }
}

/// A single event from the `events` table, ready to be streamed to clients.
#[derive(Debug, Clone)]
pub struct Event {
    pub id: i64,
    pub retro_id: i32,
    pub event_type: EventType,
    pub item_id: Option<i32>,
    pub payload: Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(FromRow)]
struct EventRow {
    id: i64,
    retro_id: i32,
    event_type: EventType,
    item_id: Option<i32>,
    payload: sqlx::types::Json<Value>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<EventRow> for Event {
    fn from(row: EventRow) -> Self {
        Event {
            id: row.id,
            retro_id: row.retro_id,
            event_type: row.event_type,
            item_id: row.item_id,
            payload: row.payload.0,
            created_at: row.created_at,
        }
    }
}

/// Load events for one retro with `after_id < id <= up_to_id`, in id order.
async fn load_events(
    pool: &PgPool,
    retro_id: i32,
    after_id: i64,
    up_to_id: i64,
) -> Result<Vec<Event>, sqlx::Error> {
    let rows = sqlx::query_as!(
        EventRow,
        r#"SELECT id as "id!", retro_id as "retro_id!", event_type as "event_type: _",
                  item_id as "item_id: _", payload as "payload: Json<Value>",
                  created_at as "created_at!"
           FROM events
           WHERE retro_id = $1 AND id > $2 AND id <= $3
           ORDER BY id"#,
        retro_id,
        after_id,
        up_to_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Event::from).collect())
}

/// In-process fan-out of events to SSE subscribers, keyed by retro.
///
/// The notifier task is the only writer; each SSE handler subscribes for its
/// retro and filters out events it already replayed from the DB (see
/// `retro_events`).
#[derive(Clone, Default)]
pub struct EventHub {
    inner: Arc<EventHubInner>,
}

#[derive(Default)]
struct EventHubInner {
    subscribers: Mutex<HashMap<i32, Vec<mpsc::UnboundedSender<Event>>>>,
}

impl EventHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to all events for one retro. The receiver yields every event
    /// published after the subscription; the caller is responsible for
    /// ignoring anything it already replayed.
    pub async fn subscribe(&self, retro_id: i32) -> mpsc::UnboundedReceiver<Event> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.inner
            .subscribers
            .lock()
            .await
            .entry(retro_id)
            .or_default()
            .push(sender);
        receiver
    }

    /// Deliver one event to all subscribers of its retro, pruning senders
    /// whose clients disconnected.
    async fn publish(&self, event: &Event) {
        let mut subscribers = self.inner.subscribers.lock().await;
        let Some(list) = subscribers.get_mut(&event.retro_id) else {
            return;
        };
        list.retain(|sender| sender.send(event.clone()).is_ok());
        if list.is_empty() {
            subscribers.remove(&event.retro_id);
        }
    }
}

/// Background task: LISTENs on the Postgres channel and forwards new `events`
/// rows to the in-process hub. Spawned once per app process.
pub async fn notifier_loop(pool: PgPool, hub: EventHub) {
    let mut last_id = match sqlx::query_scalar!("SELECT COALESCE(MAX(id), 0) FROM events")
        .fetch_one(&pool)
        .await
    {
        Ok(id) => id.unwrap_or(0),
        Err(error) => {
            tracing::error!(error = %error, "failed to read initial event cursor; notifier disabled");
            return;
        }
    };

    loop {
        if let Err(error) = notify_once(&pool, &hub, &mut last_id).await {
            tracing::error!(
                error = %error,
                "event notifier lost its LISTEN connection; reconnecting"
            );
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

async fn notify_once(pool: &PgPool, hub: &EventHub, last_id: &mut i64) -> Result<(), sqlx::Error> {
    let mut listener = sqlx::postgres::PgListener::connect_with(pool).await?;
    listener.listen("rostfacto_events").await?;
    loop {
        let notification = listener.recv().await?;
        let Ok(retro_id) = notification.payload().parse::<i32>() else {
            continue;
        };
        // Reading from the durable events table (rather than trusting the
        // notification alone) also catches up on events that were committed
        // while this task was disconnected.
        let events = load_events(pool, retro_id, *last_id, i64::MAX).await?;
        for event in events {
            hub.publish(&event).await;
            *last_id = (*last_id).max(event.id);
        }
    }
}

fn sse_frame(event: &Event) -> Result<Bytes, std::convert::Infallible> {
    let data = serde_json::to_string(&event.payload).expect("event payload should serialize");
    Ok(Bytes::from(format!(
        "event: {}\nid: {}\ndata: {}\n\n",
        event.event_type, event.id, data
    )))
}

/// Format a roster snapshot as a `PARTICIPANTS` SSE frame.
///
/// Deliberately has **no `id:` line**: presence is ephemeral, so the frame
/// must not advance the client's `Last-Event-ID` replay cursor.
fn presence_frame(roster: &Roster) -> Result<Bytes, std::convert::Infallible> {
    let data = serde_json::to_string(&serde_json::json!({ "participants": roster }))
        .expect("roster should serialize");
    Ok(Bytes::from(format!(
        "event: PARTICIPANTS\ndata: {}\n\n",
        data
    )))
}

/// Query parameters of the SSE endpoint.
#[derive(Debug, Deserialize)]
pub struct PresenceParams {
    /// Client-generated participant id (demo mode only; ignored when
    /// authenticated so it can never be used to impersonate another user).
    participant: Option<String>,
}

/// Releases one presence connection when the SSE stream is dropped, i.e. when
/// the client closes the connection, navigates away, or a keep-alive write
/// fails. The participant's `presence` row then stops being heartbeated and
/// ages out after the grace period.
struct ConnectionGuard {
    presence: PresenceHub,
    retro_id: i32,
    key: ParticipantKey,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.presence.leave(self.retro_id, self.key.clone());
    }
}

/// Who is connecting: dedup key plus display data (name, avatar).
async fn resolve_participant(
    state: &AppState,
    user: &AuthUser,
    params: &PresenceParams,
) -> Result<(ParticipantKey, String, Option<String>), Box<Response>> {
    if state.config.demo_mode() {
        let id = params
            .participant
            .as_deref()
            .and_then(|value| Uuid::parse_str(value).ok())
            .unwrap_or_else(Uuid::new_v4);
        // Name and avatar are ignored for guests; the hub assigns "Guest N".
        return Ok((ParticipantKey::Guest(id), String::new(), None));
    }

    let row = sqlx::query!(
        r#"SELECT display_name as "display_name?", avatar_url FROM users WHERE id = $1"#,
        user.user_id
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| {
        log_database_error("sse_participant_profile", &error);
        Box::new(database_error_response())
    })?;

    let (name, avatar_url) = match row {
        Some(row) => (row.display_name, row.avatar_url),
        None => (None, None),
    };
    let name = name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            if user.full_name.trim().is_empty() {
                user.username.clone()
            } else {
                user.full_name.clone()
            }
        });
    Ok((ParticipantKey::User(user.user_id), name, avatar_url))
}

/// `GET /retro/{slug}/events` — SSE stream of events for one retro.
///
/// Replays events newer than the client's `Last-Event-ID` (bounded by the
/// newest event at connect time) and then streams live events, with periodic
/// keep-alive comments. Also streams `PARTICIPANTS` roster snapshots (see
/// `presence`), which carry no event id.
pub async fn retro_events(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(params): Query<PresenceParams>,
) -> Response {
    let retro = match require_retro_access(&state, &user, &slug).await {
        Ok(Some(retro)) => retro,
        Ok(None) => return not_found_response(&state, &slug),
        Err(response) => return response.into_response(),
    };

    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());

    // Subscribe before computing the replay bound so that no event can fall
    // between the replay query and the subscription.
    let mut receiver = state.events.subscribe(retro.id).await;
    let max_id = match sqlx::query_scalar!(
        "SELECT COALESCE(MAX(id), 0) FROM events WHERE retro_id = $1",
        retro.id
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(id) => id.unwrap_or(0),
        Err(error) => {
            log_database_error("sse_max_event_id", &error);
            return database_error_response();
        }
    };

    let replay = match last_event_id {
        Some(since) => match load_events(&state.pool, retro.id, since, max_id).await {
            Ok(events) => events,
            Err(error) => {
                log_database_error("sse_replay_events", &error);
                return database_error_response();
            }
        },
        None => Vec::new(),
    };

    let (key, name, avatar_url) = match resolve_participant(&state, &user, &params).await {
        Ok(participant) => participant,
        Err(response) => return *response,
    };

    if let Err(error) = state
        .presence
        .join(retro.id, key.clone(), name, avatar_url)
        .await
    {
        log_database_error("sse_presence_join", &error);
        return database_error_response();
    }
    // From here on the guard owns the matching `leave`, so a failing
    // `subscribe` below cannot leak a connection.
    let guard = ConnectionGuard {
        presence: state.presence.clone(),
        retro_id: retro.id,
        key,
    };
    // The first message is the current roster, including this client.
    let mut presence_rx = match state.presence.subscribe(retro.id).await {
        Ok(receiver) => receiver,
        Err(error) => {
            log_database_error("sse_presence_subscribe", &error);
            return database_error_response();
        }
    };

    let stream = async_stream::stream! {
        let _guard = guard;
        // Ends the stream when the process shuts down, so axum's graceful
        // shutdown does not wait forever for this connection to close.
        let mut shutdown = state.shutdown.clone();
        for event in replay {
            yield sse_frame(&event);
        }
        let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tokio::select! {
                received = receiver.recv() => {
                    match received {
                        Some(event) if event.id > max_id => yield sse_frame(&event),
                        Some(_) => {} // already covered by the replay
                        None => break,
                    }
                }
                roster = presence_rx.recv() => {
                    match roster {
                        Some(roster) => yield presence_frame(&roster),
                        None => break,
                    }
                }
                _ = shutdown.changed() => break,
                _ = keepalive.tick() => {
                    yield Ok::<Bytes, std::convert::Infallible>(
                        Bytes::from_static(b": keep-alive\n\n"),
                    );
                }
            }
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(stream))
        .expect("SSE response should build")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presence::ParticipantView;

    #[test]
    fn presence_frame_has_no_event_id() {
        let roster = vec![ParticipantView {
            key: "guest:00000000-0000-0000-0000-000000000000".into(),
            name: "Guest 1".into(),
            avatar_url: None,
            initials: "G1".into(),
            guest: true,
            ready: false,
        }];

        let frame = presence_frame(&roster).unwrap();

        assert_eq!(
            std::str::from_utf8(&frame).unwrap(),
            "event: PARTICIPANTS\ndata: {\"participants\":[{\"key\":\"guest:00000000-0000-0000-0000-000000000000\",\"name\":\"Guest 1\",\"avatar_url\":null,\"initials\":\"G1\",\"guest\":true,\"ready\":false}]}\n\n"
        );
    }
}
