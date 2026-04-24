#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COUSIN_APPLY="${SCRIPT_DIR}/cousin_apply.sh"

# shellcheck source=lib/cousins.sh
source "${SCRIPT_DIR}/lib/cousins.sh"
# shellcheck source=lib/pr_prompt.sh
source "${SCRIPT_DIR}/lib/pr_prompt.sh"
cousins__require_jq

# Children must not prompt — only this driver does, once, at the end.
export PR_PROMPT_SUPPRESS=1

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
    echo "No cousins defined in ${CONFIG_PATH}."
    exit 0
fi

WORK_DIR="$(mktemp -d)"
echo "Work dir: ${WORK_DIR}"
echo "Cousins:  ${NAMES[*]}"

PIDS=()
LOG_FILES=()

for name in "${NAMES[@]}"; do
    LOG_FILE="${WORK_DIR}/${name}.log"
    echo "Applying to cousin: ${name}"
    (
        bash "$COUSIN_APPLY" --cousin "$name" ${ARGS[@]+"${ARGS[@]}"} \
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

echo
echo "---"
echo "Finished: $((${#PIDS[@]} - FAILED))/${#PIDS[@]} succeeded"
echo "Work dir left at: ${WORK_DIR}"

# Collect PR URLs from each cousin's log and prompt once at the end.
PR_URLS=()
for LOG_FILE in "${LOG_FILES[@]}"; do
    [[ -f "$LOG_FILE" ]] || continue
    URL="$(pr_prompt_extract_url "$(cat "$LOG_FILE")")"
    [[ -n "$URL" ]] && PR_URLS+=("$URL")
done
unset PR_PROMPT_SUPPRESS
pr_prompt_finalize "${PR_URLS[@]}"

exit $FAILED
