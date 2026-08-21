#!/usr/bin/env bash
# Kill leftover Firefox/geckodriver processes from interrupted test runs.
#
# An interrupted run (Ctrl+C, timeout) cannot close its browser sessions:
# headless Firefox instances (temp profiles under /tmp named rust_mozprofile*)
# and geckodriver processes can linger. They pile up over time and saturate
# the machine, which makes the browser suite slow and flaky. Normal browsing
# sessions are never affected: your own Firefox profile is not named
# rust_mozprofile, and geckodriver only runs while tests are active.
set -euo pipefail

changed=0
for pattern in rust_mozprofile geckodriver; do
    before=$(pgrep -f "$pattern" | wc -l | tr -d ' ' || true)
    if [ "$before" -eq 0 ]; then
        continue
    fi
    pkill -f "$pattern" || true
    sleep 1
    after=$(pgrep -f "$pattern" | wc -l | tr -d ' ' || true)
    echo "Killed $((before - after)) leftover '$pattern' process(es)"
    changed=1
done

if [ "$changed" -eq 0 ]; then
    echo "No leftover test browser processes found."
else
    echo "Done. Re-run the suite serially:"
    echo "  cargo test --test integration_test -- --test-threads=1"
fi
