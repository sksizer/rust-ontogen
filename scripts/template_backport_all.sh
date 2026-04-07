#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOWNSTREAM_FILE="${SCRIPT_DIR}/downstream.txt"
TEMPLATE_BACKPORT="${SCRIPT_DIR}/template_backport.sh"

if [[ ! -f "$DOWNSTREAM_FILE" ]]; then
    echo "Error: ${DOWNSTREAM_FILE} not found" >&2
    exit 1
fi

ARGS=("$@")

WORK_DIR="$(mktemp -d)"
echo "Clone directory: ${WORK_DIR}"

PIDS=()
REPOS=()
LOG_FILES=()

while IFS= read -r repo_url; do
    [[ -z "$repo_url" || "$repo_url" == \#* ]] && continue

    REPO_NAME="$(basename "${repo_url%/}" .git)"
    CLONE_PATH="${WORK_DIR}/${REPO_NAME}"
    LOG_FILE="${WORK_DIR}/${REPO_NAME}.log"

    echo "Cloning downstream: ${repo_url} -> ${CLONE_PATH}"
    (
        {
            git clone --quiet "$repo_url" "$CLONE_PATH"
            bash "$TEMPLATE_BACKPORT" ${ARGS[@]+"${ARGS[@]}"} "$CLONE_PATH"
        } >"$LOG_FILE" 2>&1
    ) &
    PIDS+=($!)
    REPOS+=("$repo_url")
    LOG_FILES+=("$LOG_FILE")
done < "$DOWNSTREAM_FILE"

FAILED=0
for i in "${!PIDS[@]}"; do
    echo
    echo "════════════════════════════════════════════════════════════════"
    echo "  ${REPOS[$i]}"
    echo "════════════════════════════════════════════════════════════════"
    if wait "${PIDS[$i]}"; then
        cat "${LOG_FILES[$i]}"
    else
        echo "FAILED:" >&2
        cat "${LOG_FILES[$i]}" >&2 || true
        FAILED=$((FAILED + 1))
    fi
done

echo
echo "---"
echo "Finished: $((${#PIDS[@]} - FAILED))/${#PIDS[@]} succeeded"
echo "Work dir left at: ${WORK_DIR}"
exit $FAILED
