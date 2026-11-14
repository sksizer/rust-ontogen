#!/usr/bin/env bash
# The whole example on both backends: the Rust scenario, then the Swift tests
# through the FFI.
set -euo pipefail
cd "$(dirname "$0")/.."

for backend in sqlite markdown; do
    echo "=== $backend"
    cargo test -p "iron-log-$backend" --test scenario
    BACKEND="$backend" PROFILE=debug scripts/build-xcframework.sh
    swift test --package-path app --scratch-path "app/.build/$backend"
done
