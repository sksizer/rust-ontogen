#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECTS_FILE="${SCRIPT_DIR}/sks_projects.sh"
BRING_UP="${SCRIPT_DIR}/bring_up_to_date.sh"

if [[ ! -f "$PROJECTS_FILE" ]]; then
    echo "Error: ${PROJECTS_FILE} not found" >&2
    exit 1
fi

# Forward all args (e.g. --execute) to each invocation
ARGS=("$@")

PIDS=()
DIRS=()

while IFS= read -r dir; do
    # Skip empty lines and comments
    [[ -z "$dir" || "$dir" == \#* ]] && continue

    if [[ ! -d "$dir" ]]; then
        echo "Warning: ${dir} does not exist, skipping" >&2
        continue
    fi

    echo "Starting: ${dir}"
    bash "$BRING_UP" "${ARGS[@]}" "$dir" &
    PIDS+=($!)
    DIRS+=("$dir")
done < "$PROJECTS_FILE"

# Wait for all and report results
FAILED=0
for i in "${!PIDS[@]}"; do
    if wait "${PIDS[$i]}"; then
        echo "Done: ${DIRS[$i]}"
    else
        echo "FAILED: ${DIRS[$i]}" >&2
        FAILED=$((FAILED + 1))
    fi
done

echo "---"
echo "Finished: $((${#PIDS[@]} - FAILED))/${#PIDS[@]} succeeded"

exit $FAILED
