# Rostfacto — Agent Notes

## What this app is

A Rust web app for running team retrospectives (inspired by the archived Postfacto). Users create a retro board, add cards in three columns (Good, Bad, Watch), highlight one card at a time to discuss it, mark it completed, and optionally archive all cards.

## Tech stack

- Rust 2021 edition, Tokio async runtime.
- Web framework: Axum 0.8.
- DB access: sqlx 0.7 with compile-time checked queries against PostgreSQL.
- Templating: Askama (Jinja-like HTML templates under `templates/`).
- Frontend: HTMX 2.0.4 for partial updates, Pico CSS for styling.
- CLI args: clap.
- Tests: `thirtyfour` (WebDriver/Selenium) integration tests that drive Firefox via geckodriver.

## Running the app

Requires a local PostgreSQL database and `DATABASE_URL` set:

```bash
createuser --createdb rostfacto
createdb -O rostfacto rostfacto-dev
export DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev
cargo install sqlx-cli
sqlx migrate run
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

- `src/main.rs` — Axum router setup, routes, and startup.
- `src/handlers.rs` — All HTTP handlers. Heavy use of `sqlx::query_as!` and `query!` macros.
- `src/auth.rs` — GitHub OAuth login, session management, and Axum extractors (`AuthUser`, `MaybeAuthUser`).
- `src/config.rs` — Environment-based configuration (`Config::from_env`).
- `src/github.rs` — GitHub API helpers (get user, check team membership, list org teams).
- `src/models.rs` — `Retrospective`, `Item`, `Category` and `Status` enum types.
- `src/templates.rs` — Askama template structs for each page.
- `templates/` — Askama HTML templates.
- `migrations/` — sqlx migrations (PostgreSQL enum types, tables, constraints).
- `static/` — CSS, SVG icons, favicon.
- `tests/` — WebDriver integration tests plus shared helpers.

## Authentication & authorization

- **Demo mode**: when `GITHUB_ADMIN_ORG` is not set, the app runs without authentication. A synthetic `demo` user is created at startup, every request is treated as an admin, and a red banner warns that the instance is unsecured. This mode is used by the integration tests.
- **GitHub OAuth**: when `GITHUB_CLIENT_ID` and `GITHUB_CLIENT_SECRET` are set, users must sign in via GitHub (or GitHub Enterprise Server if `GITHUB_ENTERPRISE_URL` is configured).
- **Admins**: users who are members of the configured admin team (`GITHUB_ADMIN_ORG` / `GITHUB_ADMIN_TEAM_SLUG`). Only admins can create or delete retros.
- **Retro access**: each retro is assigned a team at creation time (`team_slug`). Non-admin users can only view or change retros whose team they belong to (checked live against the GitHub API). Admins can see and manage all retros.
- **Sessions**: stored in Postgres (`sessions` table) and referenced by a random `rostfacto_session` cookie.
- **403 vs 404**: non-existent retros return 404; existing retros the user is not authorized to access return 403.

### Required environment variables for production

| Variable | Purpose |
|----------|---------|
| `DATABASE_URL` | PostgreSQL connection string |
| `PUBLIC_URL` | Public base URL of the app (used for OAuth callback) |
| `SESSION_SECRET` | Secret used to sign session cookies |
| `GITHUB_CLIENT_ID` | GitHub OAuth app client ID |
| `GITHUB_CLIENT_SECRET` | GitHub OAuth app client secret |
| `GITHUB_ADMIN_ORG` | GitHub organization containing the admin team |
| `GITHUB_ADMIN_TEAM_SLUG` | Team slug of the admin team |
| `GITHUB_USER_ORG` | Organization whose teams can be assigned to retros |
| `GITHUB_ENTERPRISE_URL` | Base URL of a GitHub Enterprise Server instance (optional) |

## Key routes

| Route | Method | Purpose |
|-------|--------|---------|
| `/` | GET | Home page |
| `/retros` | GET | List retros the current user has access to (admins see all) |
| `/retros/new` | GET | Form to create a retro (admin only) |
| `/retros` | POST | Create a retro (title, slug, team_slug) — admin only |
| `/retro/{slug}` | GET | Show a retro board |
| `/retro/{slug}/delete` | DELETE | Delete a retro and its items (admin only) |
| `/retro/{retro_id}/archive` | POST | Archive all items in the retro |
| `/items/{category}/{retro_id}` | POST | Add a new item card |
| `/items/{id}/status` | POST | Change item status (highlight/complete/cancel) |
| `/auth/login` | GET | Start GitHub OAuth login |
| `/auth/callback` | GET | GitHub OAuth callback |
| `/auth/logout` | GET | Sign out and clear session |
| `/static/*` | GET | Static files |

## Database schema

- `retrospectives(id, title, slug, created_at, team_slug, created_by)` — unique slug is the public URL key; `team_slug` controls access.
- `items(id, retro_id, text, category, created_at, status)` — FK to `retrospectives` with `ON DELETE CASCADE`.
- `users(id, github_id, username, avatar_url, access_token, created_at)` — GitHub users who have logged in.
- `sessions(id, user_id, expires_at, created_at)` — server-side sessions.
- Enums:
  - `category` = `GOOD`, `BAD`, `WATCH`
  - `status` = `CREATED`, `HIGHLIGHTED`, `COMPLETED`, `ARCHIVED`
- Partial unique index: only one `HIGHLIGHTED` item per retro (`single_highlighted_item_per_retro`).

## Core domain logic

- **Card lifecycle**: `CREATED` → `HIGHLIGHTED` → `COMPLETED`. `ARCHIVED` is a terminal state used for all items at once.
- **Highlighting**: clicking a created card marks it highlighted. Because of the DB index, only one item per retro can be highlighted; an attempt to highlight a second card fails.
- **Completing**: a highlighted card shows "Complete" and "Cancel" buttons. Completing sets it to `COMPLETED`. Cancel returns it to `CREATED`.
- **All-done prompt**: when the last active item is completed, the server returns a modal asking whether to archive all cards. Declining keeps them visible as completed.
- **Archived items** are hidden from the retro board view.
- **Slug rules**: lowercase letters, numbers, dashes only, max 255 chars, unique.

## How the frontend works

- Templates are server-rendered Askama HTML.
- HTMX attributes are used for inline updates:
  - Adding cards swaps the result into `#good-items`, `#bad-items`, or `#watch-items`.
  - Status changes replace the nearest `.card`.
  - Delete buttons target the closest table row.
  - No custom JavaScript beyond HTMX attributes.

## Testing

- Integration tests are in `tests/integration_test.rs` using `thirtyfour`.
- `tests/test_helpers.rs` starts a geckodriver instance and provides page objects (`HomePage`, `RetrosPage`, `RetroPage`).
- The integration tests start their own app instance on a random port; you do not need to start a server beforehand and it will not conflict with a dev server on port 3000.
- The tests rely on **demo mode** (no `GITHUB_ADMIN_ORG` set) so they run without real GitHub credentials.
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
- Askama templates are embedded and type-checked. The struct fields in `src/templates.rs` must match the template variables.
- `models.rs` defines a `Category::ToString` that returns uppercase (`GOOD`/`BAD`/`WATCH`) to match the DB enum; don't confuse this with standard display formatting.
- `Status::Archived` is a real enum variant but `archived_retro.html` is currently not wired to any route (only `archive_modal.html` is used).
- The `AuthUser` extractor performs a live GitHub API call on every request to check admin/team membership; in demo mode this is skipped.
- OAuth callbacks use `PUBLIC_URL` to build the redirect URI, so it must match the GitHub OAuth app settings.
