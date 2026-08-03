# Plan: Cross-client sync via SSE with Postgres as the hub

## 1. Current architecture (what exists today)

- **All mutations are HTTP POST handlers** in `src/handlers.rs` that write to Postgres and return an HTML fragment (HTMX swaps it into the DOM). The board is rendered server-side by `show_retro`.
- **Card state lives only in Postgres** (`items.status`, `likes`, `archive_id`). The only client-side state is the **highlight timer**, which is purely local JS in `templates/retro.html` (a `Map` of `item id → end timestamp` with `setInterval`). Nothing about the timer touches the server today.
- **Auth** (`AuthUser` extractor) resolves admin/team membership from the session cache; `require_retro_access_by_id` gates every mutation.
- **Single app process** in `docker-compose.yml` (one `app` service), but the design should not assume a single instance — the story explicitly says "use Postgres as hub."

## 2. Core design decision: Postgres `LISTEN`/`NOTIFY` + an event log table

To satisfy "most events initiated in the database (or at least server-side) and distributed via SSE" and "no new infrastructure component," I propose:

- **A durable `events` table** written by **DB triggers** on `items`, `likes`, and `archives`. This makes the DB the single source of truth for "what happened," survives restarts, and works regardless of which app instance performed the mutation.
- **A background notifier task** in the app that `LISTEN`s on a Postgres channel, reads new rows from the `events` table, and fans them out to in-process SSE subscribers. This is the standard "Postgres as message bus" pattern and needs no extra infra.
- **An SSE endpoint** `GET /retro/{slug}/events` that streams events for that retro to connected browsers.

### Why triggers + event table rather than NOTIFY-only
`NOTIFY` payloads are ephemeral (lost if no listener is connected, e.g. during a deploy/restart) and capped at 8000 bytes. A durable `events` table lets a freshly connected client **replay missed events** and lets the notifier recover after a restart. Triggers guarantee every mutation emits an event even if a future handler forgets to.

### Why not LISTEN directly in each SSE handler
Each SSE connection would need its own Postgres connection + `LISTEN`, which doesn't scale and makes replay/ordering harder. A single notifier per process subscribed to one channel, forwarding to an in-memory broadcast, is simpler and keeps ordering.

---

## 3. The event model

### New migration `021_sse_events.sql`

> **PG 18 baseline:** the codebase already requires PostgreSQL 18 — migration `012` uses a virtual generated column (`display_name`), and `change_item_status` uses `OLD`/`NEW` in `RETURNING`. Docker Compose and CI already run `postgres:18`. The features below are therefore safe to use.

```sql
CREATE TYPE event_type AS ENUM (
    'ITEM_CREATED', 'ITEM_UPDATED', 'ITEM_STATUS_CHANGED',
    'ITEM_LIKED', 'ITEM_UNLIKED',
    'TIMER_STARTED', 'TIMER_EXTENDED', 'TIMER_CANCELLED', 'TIMER_ELAPSED',
    'RETRO_ARCHIVED'
);

CREATE TABLE events (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    retro_id   INTEGER NOT NULL REFERENCES retrospectives(id) ON DELETE CASCADE,
    event_type event_type NOT NULL,
    item_id    INTEGER,          -- NULL for retro-level events
    payload    JSONB NOT NULL,   -- structured event data (see below)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX events_retro_id_id_idx ON events(retro_id, id);
```

> **Retention (out of scope):** the `events` table grows unboundedly. It is the replay source for reconnecting clients, so this plan adds no cleanup; note retention/archival as future work.

**Payloads** (JSONB) — designed so the client can apply lightweight changes directly (like counts, timers, text) and knows which item to re-fetch for full card re-renders:

| Event | payload |
|-------|---------|
| `ITEM_CREATED` | `{item_id, retro_id, category, text, author_name, author_initials, likes_count, status}` |
| `ITEM_UPDATED` | `{item_id, text}` |
| `ITEM_STATUS_CHANGED` | `{item_id, old_status, new_status}` |
| `ITEM_LIKED` / `ITEM_UNLIKED` | `{item_id, likes_count}` |
| `TIMER_STARTED` | `{item_id, duration_seconds, started_at, ends_at}` |
| `TIMER_EXTENDED` | `{item_id, duration_seconds, started_at, ends_at}` |
| `TIMER_CANCELLED` | `{item_id}` |
| `TIMER_ELAPSED` | `{item_id}` |
| `RETRO_ARCHIVED` | `{retro_id}` |

### Triggers
- `AFTER INSERT ON items` → `ITEM_CREATED`
- `AFTER UPDATE ON items` → `ITEM_UPDATED` (text change), `ITEM_STATUS_CHANGED` (status change), or `TIMER_STARTED` / `TIMER_EXTENDED` / `TIMER_CANCELLED` / `TIMER_ELAPSED` (timer column changes) — the trigger function compares `OLD` vs `NEW` to decide
- `AFTER INSERT/DELETE ON likes` → `ITEM_LIKED` / `ITEM_UNLIKED` (with recomputed `likes_count`)
- `AFTER INSERT ON archives` → `RETRO_ARCHIVED` — fires exactly once per archive operation (the handler only creates an `archives` row when there is something to archive)

The `items` UPDATE trigger uses a `WHEN (OLD.archive_id IS NOT DISTINCT FROM NEW.archive_id)` clause so bulk archive updates don't spam per-item events. The trigger functions build the JSONB payload and `INSERT INTO events`, then `PERFORM pg_notify('rostfacto_events', retro_id::text)`.

> **Trigger precedence (decide in Step 4):** a single `UPDATE` can change both `status` and timer columns at once (e.g. cancel = status back to `CREATED` while clearing timer columns). The trigger must emit exactly one event — suggestion: `ITEM_STATUS_CHANGED` wins, and timer events fire only when timer columns change without a status change. Also decide whether `TIMER_CANCELLED` is ever emitted, or whether cancellation is always observed as `ITEM_STATUS_CHANGED`.

> **Note on timers:** The timer is currently 100% client-side. To sync it, the timer must become **server-authoritative**. This is the biggest behavioral change and is covered in §6.

---

## 4. Server-side changes

### New module `src/events.rs`
- `Event` struct (deserialize from `events` rows / JSONB payload).
- `EventHub` — an in-process broadcast:
  - `subscribe(retro_id) -> mpsc::UnboundedReceiver<Event>` (or a `tokio::sync::broadcast` keyed per retro).
  - `notifier_loop(pool)` — the background task: `LISTEN rostfacto_events`, on each notification read the latest `events` rows for that retro (or poll), and forward to subscribers.
  - `replay(retro_id, since_id) -> Vec<Event>` — used when a client connects to catch up on missed events.
- The notifier task is spawned in `main()` alongside the existing startup work.

### `AppState`
Add `pub events: EventHub` (or `Arc<EventHub>`). It's `Clone` so it flows through Axum state like `pool` does.

### New SSE handler in `handlers.rs` (or `events.rs`)
```rust
pub async fn retro_events(State(state), user: AuthUser, Path(slug): Path<String>) -> Response
```
- `require_retro_access` (reuses existing auth gating — non-members get 403, same as the board).
- Set `Content-Type: text/event-stream`, disable buffering.
- Replay events since the client's `Last-Event-ID` (or since now for a fresh connect).
- Subscribe to the hub and stream `event: <type>\ndata: <json>\n\n`, with periodic keep-alive comments.
- On disconnect, drop the subscription.

### Route registration in `main.rs`
```rust
.route("/retro/{slug}/events", get(events::retro_events))
```

### Wiring events into existing handlers
Because triggers write the events table, **most handlers need no changes** — the DB emits the event and the notifier distributes it. This is the key payoff of the DB-centric design. The handlers that change:

- **Timer endpoints** (new): `start`, `extend`, `cancel` — see §6.
- **Every mutating handler returns the event id its mutation produced** (confirmed decision, see §5 for why): after commit, look up the latest `events` row for that item/event type (`SELECT id FROM events WHERE item_id = $1 AND event_type = $2 ORDER BY id DESC LIMIT 1`) and send it in an `X-Event-Id` response header alongside the existing HTMX fragment. Each event type per item is produced by exactly one code path, so "latest by id" is the handler's own event.
- `archive_retro` stays as-is: the `archives` INSERT trigger emits `RETRO_ARCHIVED` exactly once.

---

## 5. Frontend changes

### New SSE client on the retro page (`templates/retro.html`)
- Open `new EventSource('/retro/{slug}/events')` (slug is available in the template).
- A small JS dispatcher maps event types to DOM updates:
  - `ITEM_CREATED` → fetch the server-rendered card via `GET /items/{id}` and prepend it into the correct column (`#good-items` / `#bad-items` / `#watch-items`). **Structured JSON in the events table; the client always fetches `/items/{id}` for full card re-renders** (confirmed decision) — this keeps the Askama templates the single rendering source and the triggers pure SQL.
  - `ITEM_STATUS_CHANGED` → fetch `GET /items/{id}` and swap `outerHTML` (status determines the card variant: created / highlighted / completed).
  - `ITEM_LIKED`/`ITEM_UNLIKED` → update the `.like-count` number in place (no full re-render).
  - `ITEM_UPDATED` → update `.card-text`.
  - `TIMER_*` → update the timer badge (see §6).
  - `RETRO_ARCHIVED` → clear all cards and stop all timers.
- **Deduplication** (confirmed decision): each SSE event carries its `events.id` as the SSE `id:` field; each mutation response returns the id of the event it caused in an `X-Event-Id` header. The client keeps a small, bounded set of "already applied" ids (from its own mutations) and **ignores SSE events whose id is in that set** — this is correct even when another client's event has a lower id (a simple `id <= last_local_id` cursor would wrongly skip those). EventSource's built-in `Last-Event-ID` reconnect handling stays independent of this set: on reconnect the server replays events with `id > Last-Event-ID`, and the client still filters out its own.

### HTMX vs SSE
HTMX's SSE extension swaps HTML on named events, but our events are structured JSON and some (like `ITEM_LIKED`) only update a counter. Custom JS gives finer control and avoids re-fetching whole cards. I recommend **plain `EventSource` + a small dispatcher** rather than the HTMX SSE extension.

---

## 6. The timer: moving from client-only to server-authoritative

This is the most invasive change and directly addresses the story's "timer was (re-)started (incl. duration), elapsed, or was cancelled."

### Current behavior
`retro.html` keeps `cardTimers` (id → end timestamp) entirely in the browser; `+2 min` just extends locally.

### New migration `022_item_timers.sql`

Timer state lives on the item itself; no separate table needed:

```sql
ALTER TABLE items
    ADD COLUMN timer_started_at TIMESTAMPTZ,
    ADD COLUMN timer_duration_seconds INTEGER,
    -- PG 18 virtual generated column: deadline is computed by the DB,
    -- so the sweep query and all clients derive the same value.
    ADD COLUMN timer_ends_at TIMESTAMPTZ GENERATED ALWAYS AS
        (timer_started_at + timer_duration_seconds * INTERVAL '1 second') VIRTUAL,
    ADD COLUMN timer_elapsed_at TIMESTAMPTZ;
```

- `timer_cancelled_at` is **not** needed: cancellation is just the status transition back to `CREATED`, which the `ITEM_STATUS_CHANGED` trigger already observes.
- Virtual generated columns can be read (incl. from trigger `NEW`) but not indexed; that's fine here because only the small set of `HIGHLIGHTED` items is ever swept.

### Required changes

1. **New server endpoints** (all write to DB → triggers emit `TIMER_*` events). Use PG 18's `OLD`/`NEW` in `RETURNING` (as `change_item_status` already does) so the handler returns the authoritative new deadline in one round trip, e.g. `UPDATE items SET timer_duration_seconds = timer_duration_seconds + 120 WHERE id = $1 RETURNING id, new.timer_ends_at;`:
   - `POST /items/{id}/timer/start` (duration)
   - `POST /items/{id}/timer/extend` (+2 min)
   - `POST /items/{id}/timer/cancel`
   - A server-side check for **elapsed** — a **periodic sweep task** (confirmed decision, e.g. every 1 s) spawned in `main()`. It runs `UPDATE items SET timer_elapsed_at = NOW() WHERE status = 'HIGHLIGHTED' AND timer_ends_at <= NOW() AND timer_elapsed_at IS NULL` (the `timer_ends_at` virtual generated column keeps the predicate trivial and consistent); the items UPDATE trigger emits `TIMER_ELAPSED`. The `timer_elapsed_at IS NULL` guard makes the sweep idempotent and safe if multiple instances ever run it concurrently (only one UPDATE wins; the other re-checks the predicate and updates 0 rows).
2. **Client renders the timer from server data** (`timer_ends_at` or `timer_started_at` + `timer_duration_seconds`), not from a local `Map`. The countdown still ticks client-side for smoothness, but the authoritative end-time comes from the DB, so all clients show the same countdown.
3. **`TIMER_ELAPSED`** fires when the sweep marks a timer elapsed → all clients see `0:00` and show the `+2 min` button simultaneously.

This means the existing `initTimers`/`extendTimer` JS in `retro.html` is rewritten to read server state and to POST to the new endpoints instead of mutating a local map.

---

## 7. "Retro was completed (all cards disappear and all timers stop)"

Two distinct triggers in the story:
- **All cards completed** → the existing "all-done" archive modal. With sync, when the last card is completed, all clients should see the modal (currently only the completing client does). This is naturally handled by the `ITEM_STATUS_CHANGED` event + a client-side check, or a dedicated event.
- **Retro archived** → `RETRO_ARCHIVED` event clears all cards and stops all timers on every client. The `archives` INSERT trigger fires exactly once (the handler only creates an `archives` row when there is something to archive); the client handles the event by emptying the board.

---

## 8. Files to change (summary)

| File | Change | Step |
|------|--------|------|
| `migrations/021_sse_events.sql` | New `events` table, `event_type` enum, triggers, `pg_notify` | 1 |
| `migrations/022_item_timers.sql` | Timer columns on `items` (`timer_started_at`, `timer_duration_seconds`, virtual generated `timer_ends_at`, `timer_elapsed_at`) | 4 |
| `src/events.rs` (new) | `Event` model, `EventHub`, notifier loop, SSE handler | 2 |
| `src/main.rs` | Spawn notifier, add `events` to `AppState`, register `/retro/{slug}/events` route, spawn sweep task | 2, 4 |
| `src/handlers.rs` | Minor changes to return event ids for dedup; new timer endpoints | 2, 4 |
| `src/models.rs` | Add timer fields to `Item` (and update all `Item` SELECTs in `handlers.rs`/`show_archive`) | 4 |
| `src/templates.rs` | New template structs if timer/event fragments are server-rendered | 5 |
| `templates/retro.html` | `EventSource` client, event dispatcher, rewrite timer JS to server-authoritative | 3, 5, 6 |
| `templates/item_card.html` | Render timer from server data | 5 |
| `templates/shared/macros.html` | Timer buttons post to new endpoints | 5 |
| `tests/integration_test.rs` | New tests: two clients, one adds a card → other sees it; like syncs; timer syncs; archive clears both | 2–6 |
| `tests/test_helpers.rs` | Helper to open a second `RetroPage`/SSE client | 3 |
| `.sqlx/` | Regenerate offline query cache (`scripts/sqlx-prepare.sh`) after schema/query changes | 2, 4, 7 |
| `README.markdown` | Remove the completed TODO item; document SSE + timer behavior | 7 |
| `CHANGELOG.md` | New entry | 7 |

---

## 9. Validation plan

- `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` (CI runs these with `SQLX_OFFLINE=true`).
- `scripts/sqlx-prepare.sh` to refresh `.sqlx` after adding the `events`/timer queries.
- `cargo test --bins` (unit tests).
- `cargo test --test integration_test` — add a multi-client test: open two `BrowserSession`s on the same retro, add a card in one and assert it appears in the other; like in one and assert the count updates in the other; start/extend/cancel a timer in one and assert the other's badge matches; archive in one and assert the other's board empties.

Each step of the implementation plan in §10 ends green — its new tests plus the full existing suite, `fmt`, and `clippy` — before the next step starts.

---

## 10. Implementation plan (TDD, step by step)

Work the plan top to bottom. Each step starts **red** (write the test first, watch it fail), ends **green** (test passes and the full existing suite still passes), and is independently shippable. DoD for every step: new tests pass, existing suite passes, `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` are clean.

- [ ] Step 1 — DB: `events` table + triggers (migration 021)
- [ ] Step 2 — Server: SSE endpoint + `X-Event-Id` headers
- [ ] Step 3 — Frontend: `EventSource` client + dispatcher (item events)
- [ ] Step 4 — Timer: server-authoritative (migration 022 + endpoints + sweep)
- [ ] Step 5 — Timer: client rendering + buttons
- [ ] Step 6 — Retro-level events (archived + all-done modal)
- [ ] Step 7 — Closeout (`.sqlx`, README, CHANGELOG)

**Ground rules**

- Migrations are immutable once merged: iterate freely on `021`/`022` inside their own branch, but any later schema change ships as a new migration.
- Server-side steps are tested with raw HTTP — reqwest is already a dependency and thirtyfour cannot read SSE streams. Client-side steps use two `BrowserSession`s on the same retro.
- Run `scripts/sqlx-prepare.sh` after every step that adds or changes queries.

### Step 1 — DB: `events` table + triggers (migration 021)

No app code changes.
- **Red:** migration test (existing `tests/migration_test.rs` pattern): inserting an item produces an `ITEM_CREATED` row with the right payload; text update → `ITEM_UPDATED`; status change → `ITEM_STATUS_CHANGED`; like/unlike → `ITEM_LIKED`/`ITEM_UNLIKED` with recomputed count; archive → exactly one `RETRO_ARCHIVED`; bulk archive emits no per-item events; `pg_notify` fires on a `LISTEN`ing connection.
- **Green:** migration applied; the existing integration suite still passes — that's the guard that triggers don't break current flows.
- **Files:** `migrations/021_sse_events.sql`, `tests/migration_test.rs`.

### Step 2 — Server: SSE endpoint + `X-Event-Id` headers

- **Red (raw HTTP):** open `GET /retro/{slug}/events` and mutate via HTTP → assert one SSE frame with the right type/payload; every mutating response carries `X-Event-Id` matching the latest `events` row for that item/type; non-members get 403; connecting with `Last-Event-ID` replays only newer events.
- **Green:** `src/events.rs` (`Event`, `EventHub`, notifier loop, replay), `AppState.events`, route registration, notifier spawn, header emission in handlers.
- **Files:** `src/events.rs` (new), `src/handlers.rs`, `src/main.rs`, `.sqlx/`.
- Optional finer split: SSE endpoint first, `X-Event-Id` headers second — both are small and HTTP-testable.

### Step 3 — Frontend: `EventSource` client + dispatcher (item events)

- **Red (two browsers):** A adds a card → B sees it, and **A shows exactly one card** (dedup works); likes, text edits, and status changes sync both ways; a client's own mutations never re-apply.
- **Green:** `EventSource('/retro/{slug}/events')` + dispatcher in `retro.html`; bounded "already applied" id set fed by `X-Event-Id`.
- **Files:** `templates/retro.html`, `tests/test_helpers.rs` (second-session helper).
- Recommended finer split: 3a = `ITEM_CREATED` + dedup only (the double-render risk), 3b = remaining event types.

### Step 4 — Timer: server-authoritative (migration 022 + endpoints + sweep)

- **Red:** migration test for the virtual generated `timer_ends_at`; HTTP tests: start/extend/cancel update the DB and emit `TIMER_STARTED`/`TIMER_EXTENDED`/`TIMER_CANCELLED`; the sweep marks a short (e.g. 2 s) timer elapsed exactly once and is idempotent when run twice.
- **Green:** migration `022`; timer fields on `Item` plus all SELECTs; `POST /items/{id}/timer/{start,extend,cancel}`; 1 s sweep task in `main()`.
- **Resolve here:** trigger precedence when one `UPDATE` changes both `status` and timer columns, and whether `TIMER_CANCELLED` is ever emitted (§3).
- **Files:** `migrations/022_item_timers.sql`, `src/models.rs`, `src/handlers.rs`, `src/main.rs`, `.sqlx/`.

### Step 5 — Timer: client rendering + buttons

- **Red (two browsers):** start in A → B shows the identical end time; extend and cancel sync; elapsed shows `0:00` + `+2 min` on both (use a short duration).
- **Green:** timer JS rewritten to render from server data (`timer_ends_at`); timer buttons POST to the new endpoints; `item_card.html`/`macros.html` render the timer from server fields.
- **Files:** `templates/retro.html`, `templates/item_card.html`, `templates/shared/macros.html`, `src/templates.rs` (if needed).

### Step 6 — Retro-level events (archived + all-done modal)

- **Red (two browsers):** archive in A → B's board empties and all timers stop; completing the last active card in A → **B** sees the all-done archive modal.
- **Green:** `RETRO_ARCHIVED` dispatcher case; all-done modal driven by `ITEM_STATUS_CHANGED` + a client-side check (or a dedicated event).
- **Files:** `templates/retro.html` (+ `src/handlers.rs` if a dedicated event is chosen).

### Step 7 — Closeout

- `scripts/sqlx-prepare.sh`; full `cargo test`; `fmt`/`clippy` as CI runs them.
- `README.markdown`: remove the completed TODO item, document SSE + timer behavior.
- `CHANGELOG.md`: new entry.

### Why this order

1. **DB first, no app code:** triggers are the foundation and are fully verifiable via migration tests; the existing suite guards against regressions before any new behavior layers on top.
2. **Server before frontend:** the whole protocol (stream, replay, headers, auth) is verifiable with plain HTTP tests, so a failing two-browser test later is a JS bug, not a wire-format bug.
3. **Dedup headers before dispatcher:** without `X-Event-Id`, the first two-browser test double-renders — a client receives its own `ITEM_CREATED` over SSE.
4. **Timer last, split server/client:** the most invasive slice (schema, `Item` + every SELECT, endpoints, sweep, JS rewrite) is isolated so its churn can't complicate earlier steps.
5. **Retro-level events last:** they reuse the dispatcher and need Step 4 for "timers stop" to be observable.

---

## 11. Confirmed decisions

1. **Event payloads:** structured JSON in the `events` table; the client fetches `GET /items/{id}` for full card re-renders. Keeps Askama the single rendering source and the triggers pure SQL.
2. **Timer elapsed detection:** a periodic sweep task (every ~1 s) marks elapsed timers via an idempotent `UPDATE ... WHERE timer_elapsed_at IS NULL`, and the items trigger emits `TIMER_ELAPSED`.
3. **Dedup:** every mutating handler returns the event id of its own event in an `X-Event-Id` header; the client ignores SSE events with those ids (bounded "already applied" set, not a cursor).
4. **Multi-instance:** single instance is the target today, but the Postgres-hub design (durable `events` table + per-process `LISTEN` notifier + idempotent sweep) is already multi-instance-ready; nothing in the implementation should rely on in-process-only state as the source of truth.

## 12. PostgreSQL features used (release notes review)

The app already runs on PostgreSQL 18 (`postgres:18` in Docker Compose and CI; `display_name` virtual generated column; `OLD`/`NEW` in `RETURNING`), so PG 18 features are fair game. From the release notes:

**Used in the plan:**
- **Virtual generated columns (PG 18)** — `timer_ends_at` computed in the DB. Previously used for `display_name`; PG 18 made `VIRTUAL` the default.
- **`OLD`/`NEW` in `RETURNING` (PG 18)** — already used by `change_item_status`; the new timer endpoints use it to return the authoritative deadline in one query.

**Considered and rejected:**
- **`uuidv7()` (PG 18)** for `events.id` — rejected: `BIGINT` identity is simpler, monotonic, and pairs directly with `Last-Event-ID` replay.
- **SQL/JSON constructors (`JSON()`, `JSON_SCALAR()`, PG 17)** for trigger payloads — rejected: `jsonb_build_object` already covers the small payloads.
- **`notify_buffers` tuning knob (PG 17)** — not needed at expected event volume; note it exists if NOTIFY queue pressure ever appears.
- **Temporal constraints `WITHOUT OVERLAPS` / `PERIOD` (PG 18)** — rejected: timers are per-item, and "one highlighted item per retro" is already enforced by the partial unique index.
- **Logical replication as an alternative hub** — rejected: heavier than needed, requires replication slots, and LISTEN/NOTIFY + the `events` table covers replay.
- **`MERGE` for status transitions (PG 17)** — optional cleanup only; the existing conditional `UPDATE` works.
