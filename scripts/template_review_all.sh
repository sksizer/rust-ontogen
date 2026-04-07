#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOWNSTREAM_FILE="${SCRIPT_DIR}/downstream.txt"
TEMPLATE_REVIEW="${SCRIPT_DIR}/template_review.sh"
TEMPLATE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ ! -f "$DOWNSTREAM_FILE" ]]; then
    echo "Error: ${DOWNSTREAM_FILE} not found" >&2
    exit 1
fi

# Forward all args (e.g. --execute) to each invocation
ARGS=("$@")

WORK_DIR="$(mktemp -d)"
echo "Clone directory: ${WORK_DIR}"
echo "Template:        ${TEMPLATE_DIR}"

PIDS=()
REPOS=()
REPORT_FILES=()

while IFS= read -r repo_url; do
    [[ -z "$repo_url" || "$repo_url" == \#* ]] && continue

    REPO_NAME="$(basename "${repo_url%/}" .git)"
    CLONE_PATH="${WORK_DIR}/${REPO_NAME}"
    REPORT_FILE="${WORK_DIR}/${REPO_NAME}.report.md"

    echo "Cloning: ${repo_url} -> ${CLONE_PATH}"
    (
        git clone --quiet "$repo_url" "$CLONE_PATH"
        TEMPLATE_DIR="$TEMPLATE_DIR" \
            bash "$TEMPLATE_REVIEW" ${ARGS[@]+"${ARGS[@]}"} "$CLONE_PATH" \
            >"$REPORT_FILE" 2>&1
    ) &
    PIDS+=($!)
    REPOS+=("$repo_url")
    REPORT_FILES+=("$REPORT_FILE")
done < "$DOWNSTREAM_FILE"

# Wait for all jobs and print each report sequentially
FAILED=0
for i in "${!PIDS[@]}"; do
    if wait "${PIDS[$i]}"; then
        echo
        echo "════════════════════════════════════════════════════════════════"
        echo "  ${REPOS[$i]}"
        echo "════════════════════════════════════════════════════════════════"
        cat "${REPORT_FILES[$i]}"
    else
        echo
        echo "════════════════════════════════════════════════════════════════"
        echo "  FAILED: ${REPOS[$i]}"
        echo "════════════════════════════════════════════════════════════════" >&2
        cat "${REPORT_FILES[$i]}" >&2 || true
        FAILED=$((FAILED + 1))
    fi
done

# Aggregate reports into a single file for convenience
AGGREGATE="${WORK_DIR}/REVIEW.md"
{
    echo "# Template Backport Review — $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo
    for i in "${!REPORT_FILES[@]}"; do
        echo "---"
        echo "## ${REPOS[$i]}"
        echo
        cat "${REPORT_FILES[$i]}"
        echo
    done
} >"$AGGREGATE"

echo
echo "---"
echo "Finished: $((${#PIDS[@]} - FAILED))/${#PIDS[@]} succeeded"
echo "Aggregate report: ${AGGREGATE}"
echo "(clones kept at ${WORK_DIR} so you can inspect — delete when done)"

exit $FAILED
