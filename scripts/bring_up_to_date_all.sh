#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOWNSTREAM_FILE="${SCRIPT_DIR}/downstream.txt"
BRING_UP="${SCRIPT_DIR}/bring_up_to_date.sh"

# shellcheck source=lib/pr_prompt.sh
source "${SCRIPT_DIR}/lib/pr_prompt.sh"
# Children must not prompt — only this driver does, once, at the end.
export PR_PROMPT_SUPPRESS=1

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
LOG_FILES=()

while IFS= read -r repo_url; do
    # Skip empty lines and comments
    [[ -z "$repo_url" || "$repo_url" == \#* ]] && continue

    # Derive repo name from URL (e.g. https://github.com/sksizer/rust-dir-aspect/ -> rust-dir-aspect)
    REPO_NAME="$(basename "${repo_url%/}")"
    CLONE_PATH="${WORK_DIR}/${REPO_NAME}"
    LOG_FILE="${WORK_DIR}/${REPO_NAME}.log"

    echo "Cloning: ${repo_url} -> ${CLONE_PATH}"
    (
        {
            git clone --quiet "$repo_url" "$CLONE_PATH"
            bash "$BRING_UP" ${ARGS[@]+"${ARGS[@]}"} "$CLONE_PATH"
        } 2>&1 | tee "$LOG_FILE"
    ) &
    PIDS+=($!)
    REPOS+=("$repo_url")
    CLONE_DIRS+=("$CLONE_PATH")
    LOG_FILES+=("$LOG_FILE")
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

# Collect PR URLs from each per-repo log and prompt once at the end.
PR_URLS=()
for LOG_FILE in "${LOG_FILES[@]}"; do
    [[ -f "$LOG_FILE" ]] || continue
    URL="$(pr_prompt_extract_url "$(cat "$LOG_FILE")")"
    [[ -n "$URL" ]] && PR_URLS+=("$URL")
done
unset PR_PROMPT_SUPPRESS
pr_prompt_finalize "${PR_URLS[@]}"

# Clean up clones
echo "Cleaning up: ${WORK_DIR}"
rm -rf "$WORK_DIR"

exit $FAILED
