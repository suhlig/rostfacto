# Build stage: compile with the checked-in sqlx offline query cache (.sqlx),
# so no database is needed. Askama templates are compiled into the binary.
FROM rust:1-slim-trixie AS builder

WORKDIR /app
ENV SQLX_OFFLINE=true

# Install sqlx-cli so a later stage can run migrations. Caching this before the
# source copy keeps the tool layer stable across routine code changes.
RUN cargo install sqlx-cli --version ^0.9 --no-default-features --features rustls,postgres

COPY Cargo.toml Cargo.lock ./
COPY .sqlx .sqlx
COPY src src
COPY templates templates

RUN cargo build --release

# Migration stage: a small image that runs `sqlx migrate run` against a database.
FROM debian:trixie-slim AS migrator

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
COPY migrations /app/migrations

WORKDIR /app
ENTRYPOINT ["sqlx"]
CMD ["migrate", "run"]

# Runtime stage: just the binary, static assets, and CA roots (for the GitHub API).
FROM debian:trixie-slim

LABEL org.opencontainers.image.source="https://github.com/suhlig/rostfacto" \
      org.opencontainers.image.description="Team retrospectives, inspired by Postfacto" \
      org.opencontainers.image.licenses="AGPL-3.0-only"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --user-group rostfacto

WORKDIR /app

COPY --from=builder /app/target/release/rostfacto /usr/local/bin/rostfacto
# Served from disk at runtime, relative to the working directory
COPY static static

USER rostfacto
EXPOSE 3000

# Requires DATABASE_URL; run `sqlx migrate run` against the database separately.
ENTRYPOINT ["/usr/local/bin/rostfacto"]
