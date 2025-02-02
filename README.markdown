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
cargo run
```

# TODO

- Retro slug
- Only admin users can create and delete retros
- Retro is password-protected by default with a generated password
- Timer for each retro card
- Sync across all clients with SSE, triggers and LISTEN/NOTIFY
  * New cards appearing
  * Card status
  * Retro done (all cards disappear)
