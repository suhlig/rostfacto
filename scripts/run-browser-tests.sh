#!/usr/bin/env bash
# Run the WebDriver integration suite (integration_test, events_test,
# migration_test) serially.
#
# Firefox needs a writable home directory: at startup it reads and creates
# ~/Library/Application Support/Firefox. Under an agent sandbox that path is not
# writable, so Firefox aborts with "Could not find profile folder" and every
# browser test fails. Pointing Core Foundation at a throwaway home via
# CFFIXED_USER_HOME fixes that. Outside a sandbox the override is harmless:
# Firefox just uses the throwaway home for its profile bookkeeping.
#
# Extra arguments are passed to the test harness as a filter, e.g.:
#   scripts/run-browser-tests.sh participants
set -euo pipefail

cd "$(dirname "$0")/.."

export DATABASE_URL="${DATABASE_URL:-postgres://rostfacto@localhost/rostfacto-dev}"

fake_home="$(mktemp -d "${TMPDIR:-/tmp}/rostfacto-firefox-home.XXXXXX")"
export CFFIXED_USER_HOME="$fake_home"
cleanup() { rm -rf "$fake_home" 2>/dev/null || true; }
trap cleanup EXIT INT TERM

cargo test --test integration_test --test events_test --test migration_test -- --test-threads=1 "$@"
