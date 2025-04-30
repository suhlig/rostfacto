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

- retro-title
- Retro slug
- Only admin users can create and delete retros
- Retro is password-protected by default with a generated password
- Timer for each retro card
- Archive button (if opportunity to archive after last card complete was not used)
- Archive display, grouped by the day the items were created
- Sync across all clients with SSE, triggers and LISTEN/NOTIFY
  * A new card appears
  * A card changes status
  * Retro was completed (all cards disappear)
