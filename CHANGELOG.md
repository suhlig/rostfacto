# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Real-time sync across clients via SSE (`GET /retro/{slug}/events`), with Postgres as the hub: an `events` table written by database triggers plus a `LISTEN`/`NOTIFY` notifier fan events out to connected browsers; reconnecting clients replay missed events via `Last-Event-ID`.
- Server-authoritative highlight timers: timer state lives on the item (`timer_started_at`, `timer_duration_seconds`, virtual generated `timer_ends_at`, `timer_elapsed_at`), started automatically on highlight, extended with +2 min, and marked elapsed by a background sweep; all clients see the same countdown.
- The all-done archive modal and the archived board now appear on every connected client, not just the one that triggered them.
- Live participants panel on the retro board: a roster of everyone currently connected, pushed over the same SSE stream as ephemeral `PARTICIPANTS` frames. Presence lives in a shared `presence` table, so it is correct across multiple app processes behind a load balancer; a disconnected participant is removed after a grace period (`PRESENCE_GRACE_SECONDS`, default 15 s), and graceful shutdown clears the instance's rows immediately. In demo mode each browser is identified by a `localStorage` id and named `Guest N`; in auth mode participants dedup by user and show their GitHub name and avatar.
- The participants panel's open/closed state is part of the browser's navigational state: it is carried in the `participants` query parameter (`?participants=open`/`?participants=closed`), so a board URL is deep-linkable and Back/Forward toggles the panel. A URL without the parameter falls back to the responsive default (open on desktop, collapsed on narrow screens).
- Deno-based formatting and linting for `static/js/` (`deno fmt`, `deno lint`, configured by `deno.json`), enforced by pre-commit and CI. No bundler or Node.js build step is involved.

### Changed

- Static assets (`static/`) are embedded into the binary with `rust-embed` and served from `/static/*` by `src/assets.rs` with the correct content type and an ETag (conditional requests get a 304). A release build is now genuinely self-contained: the release tarballs, which ship only the binary, and the container image no longer need a `static/` directory next to the executable. Debug builds still read the assets from disk, so the CSS/JS edit-and-reload workflow is unchanged.
- Events are now emitted by the application instead of database triggers: each mutation writes its `events` row (and `NOTIFY`s the `rostfacto_events` channel) in the same transaction via the `emit_event` helper, and migration 025 drops the trigger machinery. The SSE contract (event ids, payloads, replay, `X-Event-Id` dedup) is unchanged. See `adr/0001-database.markdown`.
- When duplicate renders of the same card appear (the HTMX add response and the SSE re-fetch arriving in either order), the freshest render now wins: a late-arriving add-card response used to replace a card the user had just highlighted with its pre-highlight render.
- The retro board's JavaScript is split into focused ES modules (`sync.js`, `timer.js`, `participants.js`, `action-items.js`, `shortcuts.js`, `ui.js`, `identity.js`) loaded by the `retro.js` entry point; they communicate through `sse:*` `CustomEvent`s on `<body>` instead of one large IIFE. Behavior is unchanged.
- The browser tests confirm that a card click actually fired its highlight request (htmx silently drops clicks on cards replaced mid-click by an SSE re-fetch) and re-dispatch the click otherwise.
- Bumped HTMX from 2.0.10 to 4.0.0. The board's JavaScript now listens for the renamed htmx 4 events (`htmx:before:request`, `htmx:after:request`, `htmx:after:swap`, `htmx:after:settle`) and reads the `X-Event-Id` response header from the fetch-based request context (`event.detail.ctx.response.headers`) instead of an XHR object. `base.html` sets `includeIndicatorCSS: false` and `noSwap: [204, 304, "4xx", "5xx"]`, since htmx 4 swaps error responses by default and would otherwise paste a full error page into a card. The bundle is now served from cdnjs (cdnjs.cloudflare.com), the CDN whose URL and SRI hash Renovate's `html` manager can update automatically, instead of jsDelivr; the CSP `script-src` allowlist was updated to match.

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
