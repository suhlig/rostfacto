# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Real-time sync across clients via SSE (`GET /retro/{slug}/events`), with Postgres as the hub: an `events` table written by database triggers plus a `LISTEN`/`NOTIFY` notifier fan events out to connected browsers; reconnecting clients replay missed events via `Last-Event-ID`.
- Server-authoritative highlight timers: timer state lives on the item (`timer_started_at`, `timer_duration_seconds`, virtual generated `timer_ends_at`, `timer_elapsed_at`), started automatically on highlight, extended with +2 min, and marked elapsed by a background sweep; all clients see the same countdown.
- The all-done archive modal and the archived board now appear on every connected client, not just the one that triggered them.

## [1.1.0] - 2025-05-02

### Added

- 404 error page and custom error handling.
- `--bind-address` CLI option.
- Read `DATABASE_URL` from the environment.
- `AGENTS.md` with project notes for contributors.
- Renovate configuration for automated dependency updates.
- Page object pattern for integration tests.

### Changed

- Migrated integration tests from Fantoccini to Thirtyfour and run them in parallel.
- Redesigned the UI to be closer to the original Postfacto look and feel.
- Rebranded styling with rust-themed colors.
- Refined card status transitions and the archive flow.
- Identified retros by slug instead of numeric ID.
- Bumped Axum to 0.8.4 and HTMX to 2.0.10.

## [1.0.3] - 2025-01-12

### Added

- Initial Rust retrospective board using Axum, HTMX, and SQLx.
- Create and list retrospective boards.
- Add cards to Good, Bad, and Watch columns.
- Highlight and complete cards during a retro.
- Initial integration tests (run serially).
