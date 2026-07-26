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

## Project structure

- `src/main.rs` — Axum router setup, routes, and startup. No auth.
- `src/handlers.rs` — All HTTP handlers. Heavy use of `sqlx::query_as!` and `query!` macros.
- `src/models.rs` — `Retrospective`, `Item`, `Category` and `Status` enum types.
- `src/templates.rs` — Askama template structs for each page.
- `templates/` — Askama HTML templates.
- `migrations/` — sqlx migrations (PostgreSQL enum types, tables, constraints).
- `static/` — CSS, SVG icons, favicon.
- `tests/` — WebDriver integration tests plus shared helpers.

## Key routes

| Route | Method | Purpose |
|-------|--------|---------|
| `/` | GET | Home page |
| `/retros` | GET | List all retrospectives |
| `/retros/new` | GET | Form to create a retro |
| `/retros` | POST | Create a retro (title, slug) |
| `/retro/{slug}` | GET | Show a retro board |
| `/retro/{slug}/delete` | DELETE | Delete a retro and its items |
| `/retro/{retro_id}/archive` | POST | Archive all items in the retro |
| `/items/{category}/{retro_id}` | POST | Add a new item card |
| `/items/{id}/status` | POST | Change item status (highlight/complete/cancel) |
| `/static/*` | GET | Static files |

## Database schema

- `retrospectives(id, title, slug, created_at)` — unique slug is the public URL key.
- `items(id, retro_id, text, category, created_at, status)` — FK to `retrospectives` with `ON DELETE CASCADE`.
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
- The app must already be running on `http://localhost:3000` with a database before tests are executed.
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

- `sqlx` macros are compile-time checked. Any schema change must be reflected in the DB or `sqlx` prepare data; otherwise compilation fails.
- Askama templates are embedded and type-checked. The struct fields in `src/templates.rs` must match the template variables.
- `models.rs` defines a `Category::ToString` that returns uppercase (`GOOD`/`BAD`/`WATCH`) to match the DB enum; don't confuse this with standard display formatting.
- `Status::Archived` is a real enum variant but `archived_retro.html` is currently not wired to any route (only `archive_modal.html` is used).
- There is **no authentication** yet; the TODO list explicitly mentions GitHub/Enterprise auth and team-based authorization.

## Common next steps

- Add SSE for real-time updates across clients.
- Implement GitHub Enterprise auth, admin role, and team-scoped retro access.
- Add "likes" per item, edit item text, per-item timer, and an archive history view.
- Replace the integration test DB setup with a fixture/cleanup helper.
