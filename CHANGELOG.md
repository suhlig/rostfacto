# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Real-time sync across clients via SSE (`GET /retro/{slug}/events`), with Postgres as the hub: an `events` table written by database triggers plus a `LISTEN`/`NOTIFY` notifier fan events out to connected browsers; reconnecting clients replay missed events via `Last-Event-ID`.
- Server-authoritative highlight timers: timer state lives on the item (`timer_started_at`, `timer_duration_seconds`, virtual generated `timer_ends_at`, `timer_elapsed_at`), started automatically on highlight, extended with +2 min, and marked elapsed by a background sweep; all clients see the same countdown.
- The all-done archive modal and the archived board now appear on every connected client, not just the one that triggered them.
- Live participants panel on the retro board: a roster of everyone currently connected, pushed over the same SSE stream as ephemeral `PARTICIPANTS` frames. Presence lives in a shared `presence` table, so it is correct across multiple app processes behind a load balancer; a disconnected participant is removed after a grace period (`PRESENCE_GRACE_SECONDS`, default 15 s), and graceful shutdown clears the instance's rows immediately. In demo mode each browser is identified by a `localStorage` id and named `Guest N`; in auth mode participants dedup by user and show their GitHub name and avatar.
- The participants panel's open/closed state is part of the browser's navigational state: it is carried in the `participants` query parameter (`?participants=open`/`?participants=closed`), so a board URL is deep-linkable and Back/Forward toggles the panel. A URL without the parameter falls back to the responsive default (open on desktop, collapsed on narrow screens).
- The new-retro form auto-fills the slug from the title while typing (stopping once the slug is edited by hand) and flags a slug that is already in use next to the field via `GET /retros/slug-check`.
- Deno-based formatting and linting for `static/js/` (`deno fmt`, `deno lint`, configured by `deno.json`), enforced by pre-commit and CI. No bundler or Node.js build step is involved.
- Dialogs are opened and closed declaratively with the Invoker Commands API (`command="show-modal"`/`command="close"` + `commandfor`, plus `closedby="any"`), replacing the `data-open-dialog`/`data-close-dialog` delegated handler. A feature-detected fallback in `dialog-commands.js` (imported by `site.js`) emulates the commands on Firefox < 144 and Safari < 26.2.
- Request elements carry `hx-disable` to disable the triggering control while a request is in flight, guarding against double submits.
- The `hx-pending` and `hx-browser-indicator` htmx 4 extensions: the add-card and add-action-item forms show a "Sending…" placeholder while the request is in flight, and requests show the browser's tab spinner (Chromium only).
- A Deno unit test for the Invoker Commands fallback (`tests/js/dialog_commands_test.js`), run with `deno test tests/js/` and enforced by pre-commit and CI.
- Inline validation errors: a rejected card or action-item add/edit (e.g. whitespace-only text) now shows the server's message next to the form via `hx-status:400`, instead of silently doing nothing. htmx requests get a small fragment (`templates/inline_error.html`); other requests keep the full error page.

### Changed

- A rejected new-retro submission (validation error or a slug that is already in use) now re-renders the form with the error message and the submitted values preserved, instead of a standalone error page; the duplicate-slug case is a `409 Conflict` rather than a `500`.
- Static assets (`static/`) are embedded into the binary with `rust-embed` and served from `/static/*` by `src/assets.rs` with the correct content type and an ETag (conditional requests get a 304). A release build is now genuinely self-contained: the release tarballs, which ship only the binary, and the container image no longer need a `static/` directory next to the executable. Debug builds still read the assets from disk, so the CSS/JS edit-and-reload workflow is unchanged.
- Events are now emitted by the application instead of database triggers: each mutation writes its `events` row (and `NOTIFY`s the `rostfacto_events` channel) in the same transaction via the `emit_event` helper, and migration 025 drops the trigger machinery. The SSE contract (event ids, payloads, replay, `X-Event-Id` dedup) is unchanged. See `adr/0001-database.markdown`.
- When duplicate renders of the same card appear (the HTMX add response and the SSE re-fetch arriving in either order), the freshest render now wins: a late-arriving add-card response used to replace a card the user had just highlighted with its pre-highlight render.
- The retro board's JavaScript is split into focused ES modules (`sync.js`, `timer.js`, `participants.js`, `action-items.js`, `shortcuts.js`, `ui.js`, `identity.js`) loaded by the `retro.js` entry point; they communicate through `sse:*` `CustomEvent`s on `<body>` instead of one large IIFE. Behavior is unchanged.
- The browser tests confirm that a card click actually fired its highlight request (htmx silently drops clicks on cards replaced mid-click by an SSE re-fetch) and re-dispatch the click otherwise.
- Bumped HTMX from 2.0.10 to 4.0.0. The board's JavaScript now listens for the renamed htmx 4 events (`htmx:before:request`, `htmx:after:request`, `htmx:after:swap`, `htmx:after:settle`) and reads the `X-Event-Id` response header from the fetch-based request context (`event.detail.ctx.response.headers`) instead of an XHR object. `base.html` sets `includeIndicatorCSS: false` and `noSwap: [204, 304, "4xx", "5xx"]`, since htmx 4 swaps error responses by default and would otherwise paste a full error page into a card. The bundle is now served from cdnjs (cdnjs.cloudflare.com), the CDN whose URL and SRI hash Renovate's `html` manager can update automatically, instead of jsDelivr; the CSP `script-src` allowlist was updated to match.
- The all-done archive prompt is now returned as an `<hx-partial>` that swaps the existing `#archive-modal` `outerHTML`, instead of emitting a second dialog with a duplicate id.
- The slug availability check uses the `next span` extended selector rather than the `#slug-status` id (the id is kept for `aria-describedby`).
- `site.js` is now an ES module (loaded with `type="module"`) that imports the dialog fallback from the new `dialog-commands.js` module, so the fallback can be unit-tested.

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
