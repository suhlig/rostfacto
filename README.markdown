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
