# Changelog

All notable changes to this project will be documented in this file.

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
