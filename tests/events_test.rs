mod test_helpers;

use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use sqlx::PgPool;
use test_helpers::*;

/// One parsed SSE frame (event name, id, JSON data).
#[derive(Debug)]
struct SseFrame {
    event: String,
    id: Option<i64>,
    data: Value,
}

async fn next_sse_frame(
    stream: &mut (impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Unpin),
    buffer: &mut String,
) -> Option<SseFrame> {
    loop {
        if let Some(frame) = parse_sse_frame(buffer) {
            return Some(frame);
        }
        match stream.next().await {
            Some(Ok(chunk)) => buffer.push_str(&String::from_utf8_lossy(&chunk)),
            Some(Err(error)) => panic!("SSE stream error: {}", error),
            None => return None,
        }
    }
}

fn parse_sse_frame(buffer: &mut String) -> Option<SseFrame> {
    let end = buffer.find("\n\n")?;
    let raw = buffer[..end].to_string();
    buffer.drain(..end + 2);

    let mut event = String::new();
    let mut id = None;
    let mut data = String::new();
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("id:") {
            id = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("data:") {
            data = value.trim().to_string();
        }
    }
    if event.is_empty() {
        return None; // keep-alive comment
    }
    Some(SseFrame {
        event,
        id,
        data: serde_json::from_str(&data).expect("SSE data should be JSON"),
    })
}

/// Read frames until one with the expected event name appears (skipping
/// keep-alives and unrelated events).
async fn wait_for_sse_event(
    stream: &mut (impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Unpin),
    buffer: &mut String,
    expected_event: &str,
) -> SseFrame {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
    loop {
        let frame = next_sse_frame(stream, buffer)
            .await
            .expect("SSE stream ended before expected event");
        if frame.event == expected_event {
            return frame;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {} event",
            expected_event
        );
    }
}

struct TestContext {
    _db: TestDb,
    _server: TestServer,
    client: Client,
    pool: PgPool,
    base_url: String,
}

async fn setup() -> TestContext {
    let db = TestDb::new().await;
    let server = TestServer::start(&db.database_url).await;
    let base_url = server.base_url();
    let pool = PgPool::connect(&db.database_url)
        .await
        .expect("Failed to connect to test DB");
    TestContext {
        _db: db,
        _server: server,
        client: Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("Failed to build HTTP client"),
        pool,
        base_url,
    }
}

async fn create_retro(ctx: &TestContext, slug: &str) -> i32 {
    let response = ctx
        .client
        .post(format!("{}/retros", ctx.base_url))
        .form(&[("title", "Events Test"), ("slug", slug)])
        .send()
        .await
        .expect("Failed to create retro");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::SEE_OTHER,
        "creating a retro should redirect"
    );
    sqlx::query_scalar!("SELECT id FROM retrospectives WHERE slug = $1", slug)
        .fetch_one(&ctx.pool)
        .await
        .expect("Created retro should exist")
}

fn parse_item_id(html: &str) -> i32 {
    let marker = "data-item-id=\"";
    let start = html.find(marker).expect("card should carry data-item-id") + marker.len();
    let end = html[start..].find('"').expect("item id should be quoted") + start;
    html[start..end].parse().expect("item id should be numeric")
}

async fn add_item(
    ctx: &TestContext,
    category: &str,
    retro_id: i32,
    text: &str,
) -> (i32, Option<i64>) {
    let response = ctx
        .client
        .post(format!("{}/items/{}/{}", ctx.base_url, category, retro_id))
        .form(&[("text", text)])
        .send()
        .await
        .expect("Failed to add item");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "adding an item should succeed"
    );
    let event_id = response
        .headers()
        .get("x-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    let html = response.text().await.expect("Item response should be HTML");
    (parse_item_id(&html), event_id)
}

async fn latest_event_id(ctx: &TestContext, item_id: i32, event_type: &str) -> i64 {
    sqlx::query_scalar!(
        "SELECT id FROM events WHERE item_id = $1 AND event_type::text = $2 \
         ORDER BY id DESC LIMIT 1",
        item_id,
        event_type
    )
    .fetch_one(&ctx.pool)
    .await
    .expect("Expected an events row")
}

#[tokio::test]
async fn sse_streams_live_events_to_connected_clients() {
    let ctx = setup().await;
    let retro_id = create_retro(&ctx, "sse-live").await;

    let response = ctx
        .client
        .get(format!("{}/retro/sse-live/events", ctx.base_url))
        .send()
        .await
        .expect("Failed to open SSE stream");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .expect("SSE response should carry a content type")
        .to_str()
        .unwrap();
    assert!(
        content_type.starts_with("text/event-stream"),
        "SSE content type should be text/event-stream, got: {}",
        content_type
    );

    let (item_id, _) = add_item(&ctx, "Good", retro_id, "Hello sync").await;

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let frame = wait_for_sse_event(&mut stream, &mut buffer, "ITEM_CREATED").await;
    assert_eq!(
        frame.id.expect("events should carry their id"),
        1,
        "first event id should be 1"
    );
    assert_eq!(frame.data["item_id"].as_i64(), Some(item_id as i64));
    assert_eq!(frame.data["retro_id"].as_i64(), Some(retro_id as i64));
    assert_eq!(frame.data["text"].as_str(), Some("Hello sync"));
}

#[tokio::test]
async fn sse_replays_events_since_last_event_id() {
    let ctx = setup().await;
    let retro_id = create_retro(&ctx, "sse-replay").await;
    let (_, created_event_id) = add_item(&ctx, "Good", retro_id, "First card").await;
    let created_event_id = created_event_id.expect("add_item should return X-Event-Id");

    // A fresh connection with Last-Event-ID: 0 replays the existing event.
    let response = ctx
        .client
        .get(format!("{}/retro/sse-replay/events", ctx.base_url))
        .header("Last-Event-ID", "0")
        .send()
        .await
        .expect("Failed to open SSE stream with replay");
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let frame = wait_for_sse_event(&mut stream, &mut buffer, "ITEM_CREATED").await;
    assert_eq!(
        frame.id,
        Some(created_event_id),
        "replay should deliver the missed event"
    );

    // Connecting with the latest event id replays nothing; the first event
    // received must be the new one.
    let response = ctx
        .client
        .get(format!("{}/retro/sse-replay/events", ctx.base_url))
        .header("Last-Event-ID", created_event_id.to_string())
        .send()
        .await
        .expect("Failed to open SSE stream after catch-up");
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    add_item(&ctx, "Bad", retro_id, "Second card").await;
    let frame = wait_for_sse_event(&mut stream, &mut buffer, "ITEM_CREATED").await;
    assert_eq!(
        frame.data["text"].as_str(),
        Some("Second card"),
        "events older than Last-Event-ID must not be replayed"
    );
}

#[tokio::test]
async fn mutation_responses_carry_x_event_id() {
    let ctx = setup().await;
    let retro_id = create_retro(&ctx, "sse-headers").await;

    let (item_id, created_event_id) = add_item(&ctx, "Good", retro_id, "Header test").await;
    let created_event_id = created_event_id.expect("add_item response should carry X-Event-Id");
    assert_eq!(
        created_event_id,
        latest_event_id(&ctx, item_id, "ITEM_CREATED").await,
        "X-Event-Id should match the latest ITEM_CREATED event"
    );

    let response = ctx
        .client
        .post(format!(
            "{}/items/{}/status?action=highlight",
            ctx.base_url, item_id
        ))
        .send()
        .await
        .expect("Failed to highlight item");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let event_id: i64 = response
        .headers()
        .get("x-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .expect("status change response should carry X-Event-Id");
    assert_eq!(
        event_id,
        latest_event_id(&ctx, item_id, "ITEM_STATUS_CHANGED").await
    );

    let response = ctx
        .client
        .post(format!("{}/items/{}/like", ctx.base_url, item_id))
        .send()
        .await
        .expect("Failed to like item");
    let event_id: i64 = response
        .headers()
        .get("x-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .expect("like response should carry X-Event-Id");
    assert_eq!(event_id, latest_event_id(&ctx, item_id, "ITEM_LIKED").await);

    let response = ctx
        .client
        .post(format!("{}/items/{}", ctx.base_url, item_id))
        .form(&[("text", "Edited text")])
        .send()
        .await
        .expect("Failed to update item");
    let event_id: i64 = response
        .headers()
        .get("x-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .expect("update response should carry X-Event-Id");
    assert_eq!(
        event_id,
        latest_event_id(&ctx, item_id, "ITEM_UPDATED").await
    );

    // A no-op status change (already highlighted) emits no event and thus no header.
    let response = ctx
        .client
        .post(format!(
            "{}/items/{}/status?action=highlight",
            ctx.base_url, item_id
        ))
        .send()
        .await
        .expect("Failed to run no-op status change");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert!(
        response.headers().get("x-event-id").is_none(),
        "a no-op status change should not carry X-Event-Id"
    );
}

#[tokio::test]
async fn sse_for_missing_retro_returns_404() {
    let ctx = setup().await;
    let response = ctx
        .client
        .get(format!("{}/retro/does-not-exist/events", ctx.base_url))
        .send()
        .await
        .expect("Failed to request SSE for missing retro");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn timer_endpoints_update_the_db_and_emit_events() {
    let ctx = setup().await;
    let retro_id = create_retro(&ctx, "timer-http").await;
    let (item_id, _) = add_item(&ctx, "Good", retro_id, "Timed card").await;

    let response = ctx
        .client
        .get(format!("{}/retro/timer-http/events", ctx.base_url))
        .send()
        .await
        .expect("Failed to open SSE stream");
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    ctx.client
        .post(format!(
            "{}/items/{}/status?action=highlight",
            ctx.base_url, item_id
        ))
        .send()
        .await
        .expect("Failed to highlight item");
    wait_for_sse_event(&mut stream, &mut buffer, "ITEM_STATUS_CHANGED").await;

    // Start: sets the timer columns and emits TIMER_STARTED.
    let response = ctx
        .client
        .post(format!("{}/items/{}/timer/start", ctx.base_url, item_id))
        .form(&[("duration", "300")])
        .send()
        .await
        .expect("Failed to start timer");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let event_id: i64 = response
        .headers()
        .get("x-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .expect("timer start response should carry X-Event-Id");

    let frame = wait_for_sse_event(&mut stream, &mut buffer, "TIMER_STARTED").await;
    assert_eq!(
        frame.id,
        Some(event_id),
        "SSE event id should match X-Event-Id"
    );
    assert_eq!(frame.data["item_id"].as_i64(), Some(item_id as i64));
    assert_eq!(frame.data["duration_seconds"].as_i64(), Some(300));

    let row = sqlx::query!(
        "SELECT timer_started_at as \"started_at\", timer_duration_seconds as \"duration\", \
         timer_ends_at as \"ends_at\" FROM items WHERE id = $1",
        item_id
    )
    .fetch_one(&ctx.pool)
    .await
    .expect("Failed to read timer columns");
    let started_at = row.started_at.expect("timer_started_at should be set");
    assert_eq!(row.duration, Some(300));
    let ends_at = row.ends_at.expect("timer_ends_at should be computed");
    assert_eq!(ends_at, started_at + chrono::Duration::seconds(300));

    // Extend: adds 2 minutes and emits TIMER_EXTENDED.
    let response = ctx
        .client
        .post(format!("{}/items/{}/timer/extend", ctx.base_url, item_id))
        .send()
        .await
        .expect("Failed to extend timer");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let event_id: i64 = response
        .headers()
        .get("x-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .expect("timer extend response should carry X-Event-Id");
    let frame = wait_for_sse_event(&mut stream, &mut buffer, "TIMER_EXTENDED").await;
    assert_eq!(frame.id, Some(event_id));
    assert_eq!(frame.data["duration_seconds"].as_i64(), Some(420));

    let duration: Option<i32> = sqlx::query_scalar!(
        "SELECT timer_duration_seconds FROM items WHERE id = $1",
        item_id
    )
    .fetch_one(&ctx.pool)
    .await
    .expect("Failed to read timer duration");
    assert_eq!(duration, Some(420));

    // Cancel (the existing status action): resets the timer columns and emits
    // ITEM_STATUS_CHANGED, never TIMER_CANCELLED.
    ctx.client
        .post(format!(
            "{}/items/{}/status?action=cancel",
            ctx.base_url, item_id
        ))
        .send()
        .await
        .expect("Failed to cancel highlight");
    let frame = wait_for_sse_event(&mut stream, &mut buffer, "ITEM_STATUS_CHANGED").await;
    assert_eq!(frame.data["new_status"].as_str(), Some("CREATED"));

    let row = sqlx::query!(
        "SELECT timer_started_at, timer_duration_seconds, timer_elapsed_at FROM items WHERE id = $1",
        item_id
    )
    .fetch_one(&ctx.pool)
    .await
    .expect("Failed to read timer columns after cancel");
    assert!(
        row.timer_started_at.is_none(),
        "cancel should clear timer_started_at"
    );
    assert!(
        row.timer_duration_seconds.is_none(),
        "cancel should clear duration"
    );
    assert!(
        row.timer_elapsed_at.is_none(),
        "cancel should clear elapsed_at"
    );
}

#[tokio::test]
async fn timer_sweep_marks_short_timers_elapsed() {
    let ctx = setup().await;
    let retro_id = create_retro(&ctx, "timer-sweep-http").await;
    let (item_id, _) = add_item(&ctx, "Good", retro_id, "Short timer").await;

    let response = ctx
        .client
        .get(format!("{}/retro/timer-sweep-http/events", ctx.base_url))
        .send()
        .await
        .expect("Failed to open SSE stream");
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    ctx.client
        .post(format!(
            "{}/items/{}/status?action=highlight",
            ctx.base_url, item_id
        ))
        .send()
        .await
        .expect("Failed to highlight item");
    wait_for_sse_event(&mut stream, &mut buffer, "ITEM_STATUS_CHANGED").await;

    ctx.client
        .post(format!("{}/items/{}/timer/start", ctx.base_url, item_id))
        .form(&[("duration", "2")])
        .send()
        .await
        .expect("Failed to start short timer");
    wait_for_sse_event(&mut stream, &mut buffer, "TIMER_STARTED").await;

    // The 1-second sweep should mark the timer elapsed within a few seconds.
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
    loop {
        let elapsed: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar!("SELECT timer_elapsed_at FROM items WHERE id = $1", item_id)
                .fetch_one(&ctx.pool)
                .await
                .expect("Failed to read elapsed_at");
        if elapsed.is_some() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for the sweep to mark the timer elapsed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    let frame = wait_for_sse_event(&mut stream, &mut buffer, "TIMER_ELAPSED").await;
    assert_eq!(frame.data["item_id"].as_i64(), Some(item_id as i64));

    // Extending an elapsed timer restarts it (clears the elapsed marker).
    ctx.client
        .post(format!("{}/items/{}/timer/extend", ctx.base_url, item_id))
        .send()
        .await
        .expect("Failed to extend elapsed timer");
    let frame = wait_for_sse_event(&mut stream, &mut buffer, "TIMER_EXTENDED").await;
    assert_eq!(frame.data["duration_seconds"].as_i64(), Some(122));

    let elapsed: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar!("SELECT timer_elapsed_at FROM items WHERE id = $1", item_id)
            .fetch_one(&ctx.pool)
            .await
            .expect("Failed to read elapsed_at after extend");
    assert!(
        elapsed.is_none(),
        "extending should clear the elapsed marker"
    );
}

#[tokio::test]
async fn archive_emits_single_retro_archived_event() {
    let ctx = setup().await;
    let retro_id = create_retro(&ctx, "archive-event").await;
    add_item(&ctx, "Good", retro_id, "To archive").await;

    let response = ctx
        .client
        .get(format!("{}/retro/archive-event/events", ctx.base_url))
        .send()
        .await
        .expect("Failed to open SSE stream");
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    ctx.client
        .post(format!("{}/retro/{}/archive", ctx.base_url, retro_id))
        .send()
        .await
        .expect("Failed to archive retro");

    let frame = wait_for_sse_event(&mut stream, &mut buffer, "RETRO_ARCHIVED").await;
    assert_eq!(frame.data["retro_id"].as_i64(), Some(retro_id as i64));

    // Archiving again emits nothing (no active items left).
    ctx.client
        .post(format!("{}/retro/{}/archive", ctx.base_url, retro_id))
        .send()
        .await
        .expect("Failed to archive empty retro");
    let nothing = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        next_sse_frame(&mut stream, &mut buffer),
    )
    .await;
    assert!(
        nothing.is_err(),
        "archiving an empty retro should not emit another RETRO_ARCHIVED"
    );
}
