#!/bin/sh
# Creates the least-privilege PostgreSQL role the app connects as, and grants
# it just enough privileges: SELECT/INSERT/UPDATE/DELETE on the app tables,
# USAGE on the sequences (identity columns), plus the same for objects the
# migrations create later (ALTER DEFAULT PRIVILEGES).
#
# Runs automatically on first boot of a fresh `postgres` volume (mounted into
# /docker-entrypoint-initdb.d). For an existing deployment, run it manually:
#
#   docker compose exec db /docker-entrypoint-initdb.d/init-app-role.sh
#
# Idempotent: re-running only rotates the app role's password.
set -euo pipefail

: "${POSTGRES_USER:?POSTGRES_USER is required}"
: "${POSTGRES_DB:?POSTGRES_DB is required}"
: "${POSTGRES_APP_PASSWORD:?POSTGRES_APP_PASSWORD is required}"

if psql --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" -tAc \
    "SELECT 1 FROM pg_roles WHERE rolname = 'rostfacto_app'" | grep -q 1; then
    psql -v ON_ERROR_STOP=1 -v app_password="$POSTGRES_APP_PASSWORD" \
        --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" -c \
        "ALTER ROLE rostfacto_app LOGIN PASSWORD :'app_password'"
else
    psql -v ON_ERROR_STOP=1 -v app_password="$POSTGRES_APP_PASSWORD" \
        --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" -c \
        "CREATE ROLE rostfacto_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD :'app_password'"
fi

psql -v ON_ERROR_STOP=1 -v app_db="$POSTGRES_DB" \
    --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-'EOSQL'
    GRANT CONNECT ON DATABASE :'app_db' TO rostfacto_app;
    GRANT USAGE ON SCHEMA public TO rostfacto_app;
    GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO rostfacto_app;
    GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO rostfacto_app;
    ALTER DEFAULT PRIVILEGES IN SCHEMA public
        GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO rostfacto_app;
    ALTER DEFAULT PRIVILEGES IN SCHEMA public
        GRANT USAGE ON SEQUENCES TO rostfacto_app;
EOSQL
