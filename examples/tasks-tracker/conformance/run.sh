#!/usr/bin/env bash
#
# Run the JSON:API conformance check: build tasks-tracker, serve a throwaway
# copy of its vault, and drive it with a third-party JSON:API client
# (conformance.mjs).
#
#   run.sh [--locked]
#
# --locked     build with `cargo build --locked` (CI: a stale lockfile fails)
# PORT         port to serve on (default 39302)
#
# The check creates, changes and deletes records, so the server runs in a temp
# directory holding a copy of `data/`; the committed vault is never written.
# Only the server this script started is stopped, by its recorded PID.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
example="$(dirname "$here")"
manifest="$example/Cargo.toml"
port="${PORT:-39302}"

locked=()
case "${1:-}" in
    --locked) locked=(--locked) ;;
    "") ;;
    *) echo "usage: $0 [--locked]" >&2; exit 2 ;;
esac

# The `+` form keeps an empty array legal under `set -u` on bash 3.2 (macOS).
cargo build ${locked[@]+"${locked[@]}"} --manifest-path "$manifest"
# CARGO_TARGET_DIR or a config file can move the target directory; ask cargo.
target_dir="$(cargo metadata --format-version 1 --no-deps --manifest-path "$manifest" |
    node -e 'let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => console.log(JSON.parse(s).target_directory))')"
bin="$target_dir/debug/tasks-tracker"

(cd "$here" && npm ci --no-audit --no-fund --loglevel=error)

tmp="$(mktemp -d "${TMPDIR:-/tmp}/tasks-tracker-conformance.XXXXXX")"
pid=""
cleanup() {
    if [ -n "$pid" ]; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    rm -rf "$tmp"
}
trap cleanup EXIT

cp -R "$example/data" "$tmp/data"
# `exec` makes $! the server's own PID rather than a wrapping subshell's.
(cd "$tmp" && PORT="$port" exec "$bin") >"$tmp/server.log" 2>&1 &
pid=$!

# Wait for this server's own "serving" line rather than for the port to answer:
# if something else already holds the port, the port answers but this server
# has failed to bind.
ready=""
for _ in $(seq 1 100); do
    if ! kill -0 "$pid" 2>/dev/null; then
        break
    fi
    if grep -q 'serving the vault at' "$tmp/server.log"; then
        ready=1
        break
    fi
    sleep 0.1
done
if [ -z "$ready" ]; then
    echo "tasks-tracker did not start on port $port:" >&2
    cat "$tmp/server.log" >&2
    exit 1
fi
cat "$tmp/server.log"

BASE_URL="http://127.0.0.1:$port/api" node "$here/conformance.mjs"
