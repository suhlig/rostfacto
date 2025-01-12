# Rostfacto

This project aims to revive [Postfacto](https://github.com/vmware-archive/postfacto), but in Rust.

# Run


```command
brew install postgresql@17
brew services start postgresql@17
createdb retro_db
export DATABASE_URL=postgres://localhost/retro_db
cargo install sqlx-cli
sqlx migrate run
cargo run
```
