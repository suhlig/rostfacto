# Rostfacto

This project aims to revive [Postfacto](https://github.com/vmware-archive/postfacto), but in Rust.

# Run

```command
brew install postgresql@17
brew services start postgresql@17
createdb rostfacto-dev
export DATABASE_URL=postgres://localhost/rostfacto-dev
cargo install sqlx-cli
sqlx migrate run
cargo watch -x run
```

# TODO

- Authentication using GitHub (Enterprise)
- Only admin users can create and delete retros, authorized by being part of a GH team
- Retro accessible only to a specific GH team
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
