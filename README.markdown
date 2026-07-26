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

# Test

The integration tests live in `tests/integration_test.rs` and use `thirtyfour` to drive Firefox via geckodriver. The app must already be running on `http://localhost:3000` before the tests start.

## Prerequisites

On macOS:

```command
brew install geckodriver
brew install --cask firefox
```

On other systems, install [geckodriver](https://github.com/mozilla/geckodriver/releases) and make sure it is on your `PATH`, and install a recent Firefox.

## Running the tests

In one terminal, start the app with a database:

```command
export DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev
cargo run
```

In another terminal, run the integration tests:

```command
export DATABASE_URL=postgres://rostfacto@localhost/rostfacto-dev
cargo test --test integration_test
```

Set `SHOW_BROWSER` to make Firefox visible while the tests run:

```command
SHOW_BROWSER=1 cargo test --test integration_test
```

# TODO

- Authentication using GitHub (Enterprise)
- Only admin users can create and delete retros, authorized by being part of a GH team
- Retro accessible only to a specific GH team
- Edit a card
- Timer for each retro card
- Likes for each retro card
- Archive button (if opportunity to archive after last card complete was not used)
- Archive display, grouped by the day the items were created
- Sync across all clients with SSE when
  * A new card appears
  * A card changes status
  * A card was liked
  * Retro was completed (all cards disappear)

# License

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

See the [LICENSE](LICENSE) file for full license text.

The concept and some of the artwork was inspired by [Postfacto](https://github.com/vmware-archive/postfacto), reproduced under the GNU Affero General Public License.
