# Rostfacto — Agent Notes

## What this app is

A Rust web app for running team retrospectives (inspired by the archived Postfacto). Users create a retro board, add cards in three columns (Good, Bad, Watch), highlight one card at a time to discuss it, mark it completed, and optionally archive all cards.

## Tech stack

- Rust 2021 edition, Tokio async runtime.
- Web framework: Axum 0.8.
- DB access: sqlx 0.9 with compile-time checked queries against PostgreSQL.
- Templating: Askama 0.16 (Jinja-like HTML templates under `templates/`).
- Frontend: HTMX 2.0.10 for partial updates, custom CSS (`static/custom.css`) for styling.
- CLI args: clap.
- Tests: `thirtyfour` 0.37 (WebDriver/Selenium) integration tests that drive Firefox via geckodriver.

## Running the app

Requires a local PostgreSQL database and `DATABASE_URL` set:

```bash
createuser --createdb rostfacto
createdb -O rostfacto rostfacto-dev
export DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev
cargo install sqlx-cli
sqlx migrate run
# The app fails closed without auth config: set DEMO_MODE=1 for unsecured
# local development, or configure the GitHub env vars (see below).
export DEMO_MODE=1
cargo run
```

Default bind address is `0.0.0.0:3000` (can override with `--bind-address`).

## Validation prerequisites

`cargo check`, `cargo test`, and `cargo run` all require sqlx to verify queries against a live PostgreSQL database.

```bash
export DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev
```

Ensure Postgres is running, the `rostfacto-dev` database exists, and migrations have been applied (`sqlx migrate run`). Without this, compilation fails with "set `DATABASE_URL` to use query macros online".

If the database is not available, you can also use `cargo sqlx prepare` to update the offline query cache and compile in offline mode.

## Project structure

- `src/main.rs` — Axum router setup, routes, middleware (CSRF, security headers, body limit), and startup.
- `src/handlers.rs` — HTTP handlers for retros, items, and archive/delete.
- `src/auth.rs` — GitHub OAuth login/logout, session management (sliding expiry, revocation on re-login), and Axum extractors (`AuthUser`, `MaybeAuthUser`).
- `src/config.rs` — Environment-based configuration (`Config::from_env`); fails closed when GitHub auth is configured incompletely.
- `src/csrf.rs` — Origin/Referer check middleware for state-changing requests.
- `src/security_headers.rs` — CSP, HSTS, and other security headers on every response.
- `src/github.rs` — GitHub API helpers (get user, check team membership, list org teams).
- `src/models.rs` — `Retrospective`, `Item`, `Category`, `Status` and author-initials logic.
- `src/templates.rs` — Askama template structs for each page.
- `templates/` — Askama HTML templates (no inline scripts or event handlers; CSP forbids them).
- `static/js/` — `site.js` (dialogs, account menu, delete-dialog close), `retro.js` (board sync, timers, keyboard shortcuts), `home.js` (carousel).
- `migrations/` — sqlx migrations (PostgreSQL enum types, tables, constraints).
- `static/` — CSS, SVG icons, favicon.
- `tests/` — WebDriver integration tests plus migration tests and shared helpers.

## Authentication & authorization

- **Demo mode**: set `DEMO_MODE=1` to run without authentication (every request is treated as an admin; a red banner warns that the instance is unsecured). Without it, the app **fails closed**: `Config::from_env` panics unless the GitHub auth env vars (including `PUBLIC_URL`) are set. This mode is used by the integration tests.
- **GitHub OAuth**: when `GITHUB_CLIENT_ID` and `GITHUB_CLIENT_SECRET` are set, users must sign in via GitHub (or GitHub Enterprise Server if `GITHUB_ENTERPRISE_URL` is configured).
- **Admins**: users who are members of the configured admin team (`GITHUB_ADMIN_ORG` / `GITHUB_ADMIN_TEAM_SLUG`). Only admins can create or delete retros.
- **Retro access**: each retro is assigned a team at creation time (`team_slug`), stored org-qualified (`org/team-slug`); the access checks match both qualified and legacy bare slugs. Non-admin users can only view or change retros whose team they belong to (checked against the cached session teams). Admins can see and manage all retros.
- **Sessions**: stored in Postgres (`sessions` table) and referenced by a `rostfacto_session` cookie. Admin status and team membership are resolved once at login and cached in the session (`is_admin`, `teams` JSONB); subsequent requests read the cache. Orgs whose team list could not be fetched at login are cached in `team_listing_errors` and surfaced as a warning on the retro creation form, which links to the OAuth app authorization page (`GitHub → Settings → Applications`, enterprise-adapted).
- **403 vs 404**: non-existent retros return 404; existing retros the user is not authorized to access return 403.

### Required environment variables for production

| Variable | Purpose |
|----------|---------|
| `DATABASE_URL` | PostgreSQL connection string |
| `PUBLIC_URL` | Public base URL of the app (used for OAuth callback); required when auth is enabled |
| `DEMO_MODE` | Set to `1` to run without authentication (never in production) |
| `GITHUB_CLIENT_ID` | GitHub OAuth app client ID |
| `GITHUB_CLIENT_SECRET` | GitHub OAuth app client secret |
| `GITHUB_ADMIN_ORG` | GitHub organization containing the admin team |
| `GITHUB_ADMIN_TEAM_SLUG` | Team slug of the admin team |
| `GITHUB_USER_ORG` | Colon-separated list of organizations whose teams can be assigned to retros, e.g. `org-a:org-b` (optional) |
| `GITHUB_APP_OWNER` | Name or email of the person to contact when an org's teams cannot be listed (e.g. SAML SSO authorization missing); shown on the retro creation form (optional) |
| `GITHUB_ENTERPRISE_URL` | Base URL of a GitHub Enterprise Server instance (optional) |

`Config::from_env` panics on any missing required variable: a deployment with incomplete GitHub auth configuration will not start instead of silently running unsecured or broken. There is no `SESSION_SECRET` anymore — sessions are opaque random DB tokens, not signed cookies.

## Key routes

| Route | Method | Purpose |
|-------|--------|---------|
| `/` | GET | Home page |
| `/retros` | GET | List retros the current user has access to (admins see all) |
| `/retros/new` | GET | Form to create a retro (admin only) |
| `/retros` | POST | Create a retro (title, slug, team_slug) — admin only |
| `/retro/{slug}` | GET | Show a retro board |
| `/retro/{slug}/events` | GET | SSE stream of events for the retro (replays `Last-Event-ID` catch-up, then live events) |
| `/retro/{slug}/delete` | DELETE | Delete a retro and its items (admin only) |
| `/retro/{retro_id}/archive` | POST | Archive all items in the retro |
| `/items/{category}/{retro_id}` | POST | Add a new item card |
| `/items/{id}` | GET | Render a single item card |
| `/items/{id}` | POST | Update item text |
| `/items/{id}/edit` | GET | Show inline edit form for an item |
| `/items/{id}/status` | POST | Change item status (highlight/complete/cancel) |
| `/items/{id}/like` | POST | Toggle a like on the item |
| `/items/{id}/timer/start` | POST | Start the highlight timer (form field `duration`, default 300 s) |
| `/items/{id}/timer/extend` | POST | Extend the running timer by 2 minutes |
| `/auth/login` | GET | Start GitHub OAuth login |
| `/auth/callback` | GET | GitHub OAuth callback |
| `/auth/logout` | POST | Sign out and clear session |
| `/static/*` | GET | Static files |

## Database schema

- `retrospectives(id, title, slug, created_at, updated_at, team_slug, created_by)` — unique slug is the public URL key; `team_slug` controls access.
- `items(id, retro_id, text, category, created_at, updated_at, status, created_by, author_id, author_name, author_initials, likes_count, timer_started_at, timer_duration_seconds, timer_ends_at, timer_elapsed_at)` — FK to `retrospectives` with `ON DELETE CASCADE`, FK to `users` with `ON DELETE RESTRICT`. The timer columns are the server-authoritative highlight timer; `timer_ends_at` is a PG 18 virtual generated column (`timer_started_at + timer_duration_seconds`).
- `users(id, github_id, username, full_name, display_name, avatar_url, created_at, updated_at)` — GitHub users who have logged in. `display_name` is a virtual column (`COALESCE(full_name, username)`).
- `sessions(id, user_id, expires_at, created_at, updated_at, is_admin, teams, team_listing_errors)` — server-side sessions. `id` is a UUIDv7 text token; `teams` is a JSONB cache of team slugs/names; `team_listing_errors` is a JSONB list of configured user orgs whose teams could not be listed at login. Expiry slides on activity (7-day idle window, 30-day absolute cap); re-login revokes all previous sessions of the user.
- `likes(item_id, user_id)` — toggled likes on items.
- `events(id, retro_id, event_type, item_id, payload, created_at)` — durable event log written by DB triggers on `items`/`likes`/`archives`; the notifier task and SSE replay read from it. `event_type` is an enum (`ITEM_CREATED`, `ITEM_UPDATED`, `ITEM_STATUS_CHANGED`, `ITEM_LIKED`, `ITEM_UNLIKED`, `TIMER_STARTED`, `TIMER_EXTENDED`, `TIMER_CANCELLED` — reserved, never emitted, `TIMER_ELAPSED`, `RETRO_ARCHIVED`).
- Enums:
  - `category` = `GOOD`, `BAD`, `WATCH`
  - `status` = `CREATED`, `HIGHLIGHTED`, `COMPLETED`, `ARCHIVED`
- Indexes/constraints:
  - Partial unique index: only one `HIGHLIGHTED` item per retro (`single_highlighted_item_per_retro`).
  - `items_retro_category_status_idx` covering index on `(retro_id, category, status)`.
  - CHECK constraints on slug format, non-empty item text, non-empty username, and length caps (`items.text`/`action_items.text` ≤ 5000 chars, `retrospectives.title` ≤ 200 chars — migration 024, mirrored by the handler validation).
  - Integer identity columns use `GENERATED ALWAYS AS IDENTITY`.

## Core domain logic

- **Card lifecycle**: `CREATED` → `HIGHLIGHTED` → `COMPLETED`. `ARCHIVED` is a terminal state used for all items at once. Completed cards cannot be re-highlighted.
- **Highlighting**: clicking a created card marks it highlighted. Because of the DB index, only one item per retro can be highlighted; an attempt to highlight a second card renders an error on that card.
- **Completing**: a highlighted card shows "Done" and "Cancel" buttons. Completing sets it to `COMPLETED`. Cancel returns it to `CREATED`.
- **Timer**: highlighted cards show a 5-minute countdown timer with a +2 minute extend button. The timer is **server-authoritative**: the client that highlights a card starts it via `POST /items/{id}/timer/start`, the deadline is computed by the DB (`timer_ends_at`), a 1 s background sweep marks elapsed timers (`timer_elapsed_at`), and `TIMER_*` events keep every client's countdown in sync. Completing or cancelling a highlight resets the timer columns in the same UPDATE (observed as `ITEM_STATUS_CHANGED`, never `TIMER_CANCELLED`). The auto-start normally fires from `htmx:afterRequest`, but htmx 2.0.10 drops that event when the SSE re-fetch for the same status change replaces the request element before the highlight response lands; a fallback armed in `htmx:beforeRequest` starts the timer if the badge still has no deadline (guarded against double starts by an in-flight POST set).
- **Likes**: any card can be liked; likes are per-user and toggle on/off.
- **Editing**: item text can be edited inline.
- **All-done prompt**: when the last active item is completed, the server returns a modal asking whether to archive all cards. Declining keeps them visible as completed.
- **Cross-client sync**: every mutation writes an `events` row via DB triggers and `NOTIFY`s the `rostfacto_events` channel; a per-process notifier task (`events::notifier_loop`) fans events out to SSE subscribers (`GET /retro/{slug}/events`). Mutating handlers return their event id in an `X-Event-Id` header so clients can ignore the matching SSE event (dedup).
- **Slug rules**: lowercase letters, numbers, dashes only, max 255 chars, unique.

## How the frontend works

- Templates are server-rendered Askama HTML.
- HTMX attributes are used for inline updates:
  - Adding cards swaps the result into `#good-items`, `#bad-items`, or `#watch-items`.
  - Status changes, likes, and text edits replace the nearest `.card`.
  - Delete buttons target the closest table row.
- The retro page opens an `EventSource('/retro/{slug}/events')` and applies events to the DOM: full card re-renders via `GET /items/{id}`, in-place updates for likes/text/timers, board clear on `RETRO_ARCHIVED`. Mutations deduplicate their own SSE events via the `X-Event-Id` response header; cards inserted or replaced outside an HTMX swap are handed to `htmx.process()` because htmx 2.0 binds trigger handlers directly on elements.
- All JavaScript lives in `static/js/` (`site.js`, `retro.js`, `home.js`): the pages run under a strict CSP (`script-src 'self'` + the SRI-pinned htmx CDN), so **no inline scripts, `onclick`/`hx-on` attributes, or `style=` attributes may be added**. Behavior that used inline handlers now uses delegated listeners: `data-open-dialog`/`data-close-dialog` buttons, a `htmx:afterRequest` handler that resets forms and closes delete dialogs, and keydown handling for Cmd/Ctrl+Enter and Escape on card textareas.
- Two-browser sync tests must take a `two_browser_permit()` (serializes against the Firefox session limit).

## Testing

- Integration tests are in `tests/integration_test.rs` using `thirtyfour`.
- Migration tests are in `tests/migration_test.rs`.
- `tests/test_helpers.rs` starts a geckodriver instance and provides page objects (`HomePage`, `RetrosPage`, `RetroPage`).
- Each integration test starts its own app instance on a random port with a fresh PostgreSQL database copied from a migrated template; you do not need to start a server beforehand and it will not conflict with a dev server on port 3000. `TestServer::start` runs the pre-built binary via `CARGO_BIN_EXE_rostfacto` (never `cargo run`) — spawning cargo from inside tests rebuilds the app and its shared dependencies on every run, serializing the suite on the build lock and starving the browsers under CI load.
- Test databases are dropped when a test finishes (or panics), but a killed test process (Ctrl+C, timeout) cannot clean up after itself. Each test run therefore starts by dropping leftover `rostfacto_test_*` databases that have no active connections.
- The template database (`rostfacto_test_template`) is created automatically on the first test run, so `rostfacto-dev` is no longer polluted by test data.
- The tests rely on **demo mode** (`DEMO_MODE=1`, no `GITHUB_ADMIN_ORG` set) so they run without real GitHub credentials. `TestServer::start` also sets `PUBLIC_URL` to the random test port.
- You need Firefox and geckodriver installed. On macOS:
  ```bash
  brew install geckodriver
  brew install --cask firefox
  ```
- Run the tests with:
  ```bash
  export DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev
  cargo test --test integration_test
  ```
- Set `SHOW_BROWSER` to run Firefox visibly:
  ```bash
  SHOW_BROWSER=1 cargo test --test integration_test
  ```

## Important quirks for agents

- `sqlx` macros are compile-time checked against a live database. Any schema change must be reflected in the DB or in `sqlx` prepare data; otherwise compilation fails with "set `DATABASE_URL` to use query macros online". Use `DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev`.
- Askama templates are embedded and type-checked. The struct fields in `src/templates.rs` must match the template variables. Template-only changes do not trigger a rebuild by themselves — `touch src/main.rs` (or `cargo clean -p rostfacto`) before `cargo build`/`cargo run` if only templates changed.
- Askama blocks cannot be nested: `{% block %}` overrides must sit at the top level of the template (a block inside `{% block content %}` renders empty).
- `models.rs` implements `Display` for `Category` so that `to_string()` returns uppercase (`GOOD`/`BAD`/`WATCH`) to match the DB enum. The same file also defines `url_segment()`, `display_label()`, `column_class()`, `icon()`, and `items_container_id()` helpers.
- The `AuthUser` extractor reads cached admin/team data from the session; it does **not** call the GitHub API on every request. Live API calls happen only during the OAuth callback.
- OAuth callbacks use `PUBLIC_URL` to build the redirect URI, so it must match the GitHub OAuth app settings.
- `main()` spawns two background tasks: `events::notifier_loop` (`LISTEN` on `rostfacto_events`, fans events out to SSE subscribers) and `handlers::timer_sweep_loop` (marks elapsed highlight timers every second). Both are idempotent/multi-instance-safe; the durable `events` table is the source of truth for replay.
- Sessions slide on activity (7-day idle, 30-day cap) and are revoked on re-login; deleting a retro requires an admin session at most 24 h old (step-up re-auth, redirects to `/auth/login`).
- CSRF protection is an Origin/Referer match check for state-changing requests (mismatch → 403); Firefox sends neither header on same-origin form POSTs, so a missing header is accepted — SameSite=Lax cookies are the primary defense.
- Timer events: `TIMER_CANCELLED` is never emitted — cancelling a highlight is always observed as `ITEM_STATUS_CHANGED` (the trigger's status branch wins over timer changes).
- The dev database must stay consistent with the recorded migrations: `sqlx migrate run` only applies *new* migrations, so manually dropping triggers/functions on an already-migrated DB leaves it broken while `_sqlx_migrations` still claims everything is applied. When iterating on unmerged migrations, drop the `_sqlx_migrations` rows for them and re-apply, or recreate the dev DB.
