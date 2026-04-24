#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COUSIN_REVIEW="${SCRIPT_DIR}/cousin_review.sh"

# shellcheck source=lib/cousins.sh
source "${SCRIPT_DIR}/lib/cousins.sh"
cousins__require_jq

CONFIG_PATH="$(cousins_config_path "$SCRIPT_DIR")"
if [[ ! -f "$CONFIG_PATH" ]]; then
    echo "Error: cousins config not found at ${CONFIG_PATH}" >&2
    exit 1
fi

ARGS=("$@")

NAMES=()
while IFS= read -r name; do
    [[ -n "$name" ]] && NAMES+=("$name")
done < <(cousins_list_names "$CONFIG_PATH")
if [[ ${#NAMES[@]} -eq 0 ]]; then
    echo "No cousins defined in ${CONFIG_PATH}. Add entries under '.cousins' (see \$example key for schema)."
    exit 0
fi

WORK_DIR="$(mktemp -d)"
echo "Work dir: ${WORK_DIR}"
echo "Cousins:  ${NAMES[*]}"

PIDS=()
LOG_FILES=()

for name in "${NAMES[@]}"; do
    LOG_FILE="${WORK_DIR}/${name}.log"
    echo "Reviewing cousin: ${name}"
    (
        bash "$COUSIN_REVIEW" --cousin "$name" ${ARGS[@]+"${ARGS[@]}"} \
            >"$LOG_FILE" 2>&1
    ) &
    PIDS+=($!)
    LOG_FILES+=("$LOG_FILE")
done

FAILED=0
for i in "${!PIDS[@]}"; do
    echo
    echo "════════════════════════════════════════════════════════════════"
    echo "  ${NAMES[$i]}"
    echo "════════════════════════════════════════════════════════════════"
    if wait "${PIDS[$i]}"; then
        cat "${LOG_FILES[$i]}"
    else
        echo "FAILED:" >&2
        cat "${LOG_FILES[$i]}" >&2 || true
        FAILED=$((FAILED + 1))
    fi
done

# Aggregate into a single report for convenience.
AGGREGATE="${WORK_DIR}/REVIEW.md"
{
    echo "# Cousin Review — $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo
    for i in "${!NAMES[@]}"; do
        echo "---"
        echo "## ${NAMES[$i]}"
        echo
        cat "${LOG_FILES[$i]}"
        echo
    done
} >"$AGGREGATE"

echo
echo "---"
echo "Finished: $((${#PIDS[@]} - FAILED))/${#PIDS[@]} succeeded"
echo "Aggregate report: ${AGGREGATE}"
exit $FAILED
