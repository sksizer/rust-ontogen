#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOWNSTREAM_FILE="${SCRIPT_DIR}/downstream.txt"
CARGO_UPDATE="${SCRIPT_DIR}/cargo_update.sh"

if [[ ! -f "$DOWNSTREAM_FILE" ]]; then
    echo "Error: ${DOWNSTREAM_FILE} not found" >&2
    exit 1
fi

# Forward all args (e.g. --execute) to each invocation
ARGS=("$@")

# Create a shared tmp directory for all clones
WORK_DIR="$(mktemp -d)"
echo "Clone directory: ${WORK_DIR}"

PIDS=()
REPOS=()
CLONE_DIRS=()

while IFS= read -r repo_url; do
    # Skip empty lines and comments
    [[ -z "$repo_url" || "$repo_url" == \#* ]] && continue

    # Derive repo name from URL (e.g. https://github.com/sksizer/rust-dir-aspect/ -> rust-dir-aspect)
    REPO_NAME="$(basename "${repo_url%/}")"
    CLONE_PATH="${WORK_DIR}/${REPO_NAME}"

    echo "Cloning: ${repo_url} -> ${CLONE_PATH}"
    (
        git clone --quiet "$repo_url" "$CLONE_PATH"
        bash "$CARGO_UPDATE" ${ARGS[@]+"${ARGS[@]}"} "$CLONE_PATH"
    ) &
    PIDS+=($!)
    REPOS+=("$repo_url")
    CLONE_DIRS+=("$CLONE_PATH")
done < "$DOWNSTREAM_FILE"

# Wait for all and report results
FAILED=0
for i in "${!PIDS[@]}"; do
    if wait "${PIDS[$i]}"; then
        echo "Done: ${REPOS[$i]}"
    else
        echo "FAILED: ${REPOS[$i]}" >&2
        FAILED=$((FAILED + 1))
    fi
done

echo "---"
echo "Finished: $((${#PIDS[@]} - FAILED))/${#PIDS[@]} succeeded"

# Clean up clones
echo "Cleaning up: ${WORK_DIR}"
rm -rf "$WORK_DIR"

exit $FAILED
