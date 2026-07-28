#!/usr/bin/env bash
set -euo pipefail

if [ -z "${DATABASE_URL:-}" ]; then
    echo "DATABASE_URL is not set; cannot run sqlx prepare."
    echo "Set it to your local Postgres database and try again."
    exit 1
fi

cargo sqlx prepare -- --all-targets

if ! git diff --quiet .sqlx; then
    echo ""
    echo "sqlx query metadata changed. Run 'git add .sqlx' and commit again."
    exit 1
fi
