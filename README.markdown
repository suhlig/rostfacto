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
| `GITHUB_USER_ORG` | The organization whose teams can be assigned to retros. Usually the same as `GITHUB_ADMIN_ORG`. |
| `GITHUB_ENTERPRISE_URL` | Base URL of your GitHub Enterprise Server, e.g. `https://github.example.com`. Leave unset for github.com. |

When you create the OAuth App, set the callback URL to:

```
<PUBLIC_URL>/auth/callback
```

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

- Auto-fill the retro slug from the title while typing; avoid clashes with existing slugs
- Allow multiple `GITHUB_USER_ORG`s
- Allow adding a card anonymously
- Publish a container image on every release and tag it with the release version
- Rework home page to be a landing page with
  - screenshots of individual actions like adding a card, shown as a carousel (instead of the animated GIF we currently have)
  - what's different to Postfacto
- Release 2.0

# License

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

See the [LICENSE](LICENSE) file for full license text.

The concept and some of the artwork was inspired by [Postfacto](https://github.com/vmware-archive/postfacto), reproduced under the GNU Affero General Public License.
