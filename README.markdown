# Rostfacto

This project aims to revive [Postfacto](https://github.com/vmware-archive/postfacto), but in Rust.

# Run

```command
brew services start postgresql@14
createdb retro_db
cargo install sqlx-cli
export DATABASE_URL=postgres://localhost/retro_db
sqlx migrate run
cargo run
```
