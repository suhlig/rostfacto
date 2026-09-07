# 0001 — Keep PostgreSQL as the database

- Status: Accepted
- Date: 2026-09-07
- Deciders: Project maintainers
- Related: the app-emitted `events` refactor (migration 025)

## Context

Rostfacto needs a shared store for boards, sessions, and its cross-client
synchronization. The live-sync feature is the load-bearing requirement: every
mutation must reach every browser with an open board, ideally as a push, and a
client that reconnects must be able to replay what it missed.

For a while we considered switching to SQLite to make deployment and local
development simpler (no database service to run, a single file instead of a
server). The evaluation surfaced three ways Postgres is load-bearing, not just
preferred:

1. **SSE fan-out is push-based.** Mutations write a row to the durable `events`
   table and `NOTIFY` the `rostfacto_events` channel inside the same
   transaction; a per-process notifier task (`events::notifier_loop`) listens
   and fans the event out to SSE subscribers. `LISTEN`/`NOTIFY` is a
   database-wide broadcast, so this works with **multiple app instances**: an
   instance that did not perform the mutation still receives the event and
   delivers it to its own SSE clients. SQLite has no equivalent channel and no
   network protocol, so this whole mechanism would have to be rebuilt as
   polling plus an out-of-process bus.
2. **Multi-instance operation is already designed in.** Sessions live in the
   DB; the timer sweep is idempotent; the partial unique index
   (`single_highlighted_item_per_retro`) enforces the one-highlight rule
   retro-wide regardless of which instance handled the request. Horizontal
   scaling today is "run more instances behind a load balancer". SQLite is
   single-writer and single-host; sharing the file over a network filesystem is
   unsafe, and multi-host topologies (LiteFS, rqlite, …) reintroduce the
   infrastructure SQLite was meant to remove.
3. **Compile-time checked queries.** The ~100 `sqlx::query!`-family macros are
   verified against a live database (or the committed `.sqlx` offline cache).
   These macros are per-database: they cannot be shared between Postgres and
   SQLite, so a pluggable backend would mean either duplicated query sets or
   giving up the checking the project deliberately relies on.

The schema also leans on Postgres-specific features (enums, JSONB, identity
columns, PG 18 virtual generated columns and `uuidv7()`), all of which would
need rewriting for SQLite.

As part of this decision we also simplified the event machinery: events are now
written by the application inside the same transaction as each mutation
(`emit_event` in `src/handlers.rs`) instead of by database triggers, and
migration 025 drops the trigger functions. This removed the most intricate
plpgsql in the codebase and the duplicate "what will the trigger emit"
reasoning in the handlers, while keeping the same observable SSE contract.

## Decision

We will keep PostgreSQL (PG 18) as the only supported database for the
foreseeable future. Specifically:

- Do **not** introduce SQLite, and do **not** build a database-abstraction
  layer (a `Db` trait, `sqlx::Any`, or feature-flagged query duplication).
- Keep using `sqlx::query!`-family macros checked against a live Postgres
  database (or the offline `.sqlx` cache) so query correctness stays a compile
  time concern.
- Keep `LISTEN`/`NOTIFY` + the per-process notifier as the SSE fan-out
  mechanism; keep multi-instance operation as the deployment model.
- Events are emitted by the app in the mutation's transaction
  (`emit_event`), not by database triggers. This is a *prerequisite*, not a
  step away from Postgres: it removes the trigger complexity that would have
  been the hardest part of any future SQLite port, should that decision ever
  be revisited.

## Consequences

### Positive

- Push-based SSE with sub-second cross-instance sync out of the box.
- Horizontal scaling by adding instances; sessions, the single-highlight
  invariant, and timer expiry all stay correct without shared in-memory state.
- Compile-time query verification (the `.sqlx` cache keeps CI hermetic).
- Event payloads are now built in Rust where the values are already in scope,
  and PG 18 quirks (generated columns reading NULL inside triggers) are gone.

### Negative

- Operations requires a PostgreSQL 18 server (service dependency in Docker /
  CI; `DATABASE_URL` required to build).
- Event production is now a matter of app discipline rather than DB
  enforcement: every new mutation path must go through `emit_event` or boards
  will silently stop syncing. The `events_test.rs` HTTP-level tests are the
  regression net. Raw SQL writes (e.g. ad-hoc support queries) no longer
  produce events, and migration-level contract tests for them were removed.
- Deploy ordering matters when removing/adding event emission: dropping the
  triggers and shipping the emitting code in one release leaves a short window
  where the old binary writes no events. Acceptable for this project's deploy
  cadence; a two-phase rollout would be overkill.
- Single-node/hosted-SQLite deployments (Litestream, Fly LiteFS, …) are not
  on the roadmap, so the "no database server" selling point is explicitly
  traded away.

## Alternatives considered

- **SQLite, single node.** Viable on data volume and latency, but drops
  multi-instance operation and turns SSE push into polling (or per-instance
  in-process publishing), and would require rewriting the schema and ~100
  checked queries. Rejected: it solves a problem (ops simplicity) we do not
  currently have while breaking the horizontal-scaling path we rely on.
- **`sqlx::Any` with runtime queries.** One code path for both databases, but
  loses compile-time query checking — the property that makes this codebase
  safe to refactor. Rejected.
- **Feature-flagged Postgres/SQLite query modules.** Keeps checking but
  duplicates every query and the enum/JSON mapping; high ongoing cost for a
  feature nobody needs. Rejected.

## Note on the event-emission refactor

The switch from triggers to app-emitted events (migration 025) keeps the
`events` table, the `event_type` enum, the `NOTIFY` channel, the notifier, and
the SSE replay/dedup contract unchanged. It removes `emit_item_event`,
`emit_like_event`, `emit_archive_event` and their five triggers. Handlers now
receive the event id directly from `INSERT … RETURNING id`, which also removes
the read-back `SELECT … ORDER BY id DESC LIMIT 1` queries and their mirror
logic for the `X-Event-Id` header.
