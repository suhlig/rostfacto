# Build stage: compile with the checked-in sqlx offline query cache (.sqlx),
# so no database is needed. Askama templates are compiled into the binary.
# Base images are pinned by digest so a rebuilt image is reproducible and a
# compromised tag cannot change what ships. The builder base tracks the
# toolchain pinned in rust-toolchain.toml; bump both together.
FROM rust:1.98.0-slim-trixie@sha256:cc0448b41c3b7b7fea44f5dc50eacba729a56db365b65b7bd5e8a82d5b3db078 AS builder

WORKDIR /app
ENV SQLX_OFFLINE=true

# Install sqlx-cli so a later stage can run migrations. Pinned to the exact
# version and cached before the source copy so the tool layer stays stable
# across routine code changes.
RUN cargo install sqlx-cli --version 0.9.0 --no-default-features --features rustls,postgres

COPY Cargo.toml Cargo.lock ./
COPY .sqlx .sqlx
COPY src src
COPY templates templates

RUN cargo build --release

# Migration stage: a small image that runs `sqlx migrate run` against a database.
FROM debian:trixie-slim@sha256:3a39a0592364683e6bab97937b72cad5a8fa6dcbbee90edb3bb48c7f8e94f258 AS migrator

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
COPY migrations /app/migrations

WORKDIR /app
ENTRYPOINT ["sqlx"]
CMD ["migrate", "run"]

# Runtime stage: just the binary, static assets, and CA roots (for the GitHub API).
FROM debian:trixie-slim@sha256:3a39a0592364683e6bab97937b72cad5a8fa6dcbbee90edb3bb48c7f8e94f258

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
