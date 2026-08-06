# Rostfacto

This project aims to revive [Postfacto](https://github.com/vmware-archive/postfacto), but in Rust.

# Run

```command
brew install postgresql@18
brew services start postgresql@18

# Create a dedicated database role for the app
createuser --createdb rostfacto
createdb -O rostfacto rostfacto-dev

export DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev
cargo install sqlx-cli
sqlx migrate run
cargo watch -x run
```

## Authentication (GitHub)

By default the app runs in **demo mode** (no authentication) if `GITHUB_ADMIN_ORG` is not set. To enable GitHub or GitHub Enterprise authentication, configure the following environment variables:

| Variable | Where to get it |
|----------|-----------------|
| `PUBLIC_URL` | The public base URL of your deployment, e.g. `https://rostfacto.example.com`. Used to build the OAuth callback URL. |
| `SESSION_SECRET` | Generate a random secret, e.g. `openssl rand -hex 32`. |
| `GITHUB_CLIENT_ID` | Create an OAuth App at <https://github.com/settings/developers> (or on your GHE instance). |
| `GITHUB_CLIENT_SECRET` | Same OAuth App page as above. |
| `GITHUB_ADMIN_ORG` | The GitHub organization that contains your admin team. You can see your orgs at <https://github.com/settings/organizations>. |
| `GITHUB_ADMIN_TEAM_SLUG` | Inside that org, open the team page and take the slug from the URL, e.g. `https://github.com/orgs/ORG/teams/TEAM_SLUG`. |
| `GITHUB_USER_ORG` | Colon-separated list of organizations whose teams can be assigned to retros, e.g. `org-a:org-b`. Usually includes `GITHUB_ADMIN_ORG`. |
| `GITHUB_ENTERPRISE_URL` | Base URL of your GitHub Enterprise Server, e.g. `https://github.example.com`. Leave unset for github.com. |

When you create the OAuth App, set the callback URL to:

```
<PUBLIC_URL>/auth/callback
```

# Real-time sync

Multiple clients on the same retro stay in sync via server-sent events (SSE):

- The board subscribes to `GET /retro/{slug}/events`; every mutation (card added, status changed, liked, edited, timer changed, retro archived) is pushed to all connected clients immediately.
- Postgres is the hub: database triggers write every event to an `events` table and `NOTIFY` a channel that a background task fans out to the connected browsers. The event log is durable, so a client that reconnects catches up on everything it missed (`Last-Event-ID` replay).
- A client's own mutations are deduplicated, so the HTMX response and the SSE event for the same change are applied exactly once.
- The highlight timer is **server-authoritative**: highlighting a card starts a five-minute countdown in the database, the +2 min button extends it, and a background sweep marks it elapsed so every client sees `0:00` at the same time. The countdown ticks locally, but the deadline always comes from the server.
- Archiving a retro (or completing the last card, via the all-done modal) clears the board and stops all timers on every connected client.

# Test

The integration tests live in `tests/integration_test.rs` and use `thirtyfour` to drive Firefox via geckodriver. They start their own instance of the app on a random port, so you can keep your dev server running on port 3000.

## Prerequisites

On macOS:

```command
brew install geckodriver
brew install --cask firefox
```

On other systems, install [geckodriver](https://github.com/mozilla/geckodriver/releases) and make sure it is on your `PATH`, and install a recent Firefox.

## Running the tests

```command
export DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev
cargo test --test integration_test
```

Set `SHOW_BROWSER` to make Firefox visible while the tests run:

```command
SHOW_BROWSER=1 cargo test --test integration_test
```

# TODO

- Publish a container image on every release and tag it with the release version
- Release 2.0
- Auto-fill the retro slug from the title while typing; avoid clashes with existing slugs
- Allow adding a card anonymously
- Rework home page to be a landing page with
  - screenshots of individual actions like adding a card, shown as a carousel (instead of the animated GIF we currently have)
  - what's different to Postfacto
- Mobile version
- Limit growth of the `events` table
- Clean archived retros after e.g. a year
- Periodic cleanup of sessions

# License

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

See the [LICENSE](LICENSE) file for full license text.

The concept and some of the artwork was inspired by [Postfacto](https://github.com/vmware-archive/postfacto), reproduced under the GNU Affero General Public License.
