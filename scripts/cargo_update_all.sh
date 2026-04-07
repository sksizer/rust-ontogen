#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOWNSTREAM_FILE="${SCRIPT_DIR}/downstream.txt"
CARGO_UPDATE="${SCRIPT_DIR}/cargo_update.sh"

# shellcheck source=lib/pr_prompt.sh
source "${SCRIPT_DIR}/lib/pr_prompt.sh"
# Children must not prompt — only this driver does, once, at the end.
export PR_PROMPT_SUPPRESS=1

# --- Colors (disabled when not a tty or NO_COLOR is set) --------------------
if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
    C_RESET=$'\033[0m'
    C_BOLD=$'\033[1m'
    C_DIM=$'\033[2m'
    C_RED=$'\033[31m'
    C_GREEN=$'\033[32m'
    C_YELLOW=$'\033[33m'
    C_BLUE=$'\033[34m'
    C_MAGENTA=$'\033[35m'
    C_CYAN=$'\033[36m'
else
    C_RESET=; C_BOLD=; C_DIM=; C_RED=; C_GREEN=; C_YELLOW=; C_BLUE=; C_MAGENTA=; C_CYAN=
fi

log()    { echo "${C_CYAN}[update-all]${C_RESET} $*"; }
warn()   { echo "${C_YELLOW}[update-all]${C_RESET} $*" >&2; }
error()  { echo "${C_RED}[update-all] ERROR:${C_RESET} $*" >&2; }

if [[ ! -f "$DOWNSTREAM_FILE" ]]; then
    error "${DOWNSTREAM_FILE} not found"
    exit 1
fi

# Forward all args (e.g. --execute) to each invocation
ARGS=("$@")

# Create a shared tmp directory for all clones
WORK_DIR="$(mktemp -d)"
log "Clone directory: ${C_DIM}${WORK_DIR}${C_RESET}"
if [[ ${#ARGS[@]} -gt 0 ]]; then
    log "Forwarding args: ${C_BOLD}${ARGS[*]}${C_RESET}"
else
    log "Mode: ${C_YELLOW}dry-run${C_RESET} (pass --execute to apply)"
fi

# Collect repos first so we can report a plan
REPOS=()
while IFS= read -r repo_url; do
    [[ -z "$repo_url" || "$repo_url" == \#* ]] && continue
    REPOS+=("$repo_url")
done < "$DOWNSTREAM_FILE"

log "Found ${C_BOLD}${#REPOS[@]}${C_RESET} downstream repo(s):"
for r in "${REPOS[@]}"; do
    echo "  ${C_DIM}•${C_RESET} $r"
done

PIDS=()
CLONE_DIRS=()
REPO_NAMES=()

for repo_url in "${REPOS[@]}"; do
    # Derive repo name from URL (e.g. https://github.com/sksizer/rust-dir-aspect/ -> rust-dir-aspect)
    REPO_NAME="$(basename "${repo_url%/}" .git)"
    CLONE_PATH="${WORK_DIR}/${REPO_NAME}"
    LOG_FILE="${WORK_DIR}/${REPO_NAME}.log"

    log "${C_MAGENTA}▶ ${REPO_NAME}${C_RESET} — cloning ${C_DIM}${repo_url}${C_RESET}"
    (
        {
            echo "${C_BOLD}${C_MAGENTA}=== ${REPO_NAME} ===${C_RESET}"
            echo "${C_CYAN}[${REPO_NAME}]${C_RESET} repo: ${repo_url}"
            echo "${C_CYAN}[${REPO_NAME}]${C_RESET} clone path: ${CLONE_PATH}"
            git clone --quiet "$repo_url" "$CLONE_PATH"
            echo "${C_CYAN}[${REPO_NAME}]${C_RESET} running cargo_update.sh ${ARGS[*]:-}"
            REPO_LABEL="$REPO_NAME" bash "$CARGO_UPDATE" ${ARGS[@]+"${ARGS[@]}"} "$CLONE_PATH"
        } >"$LOG_FILE" 2>&1
    ) &
    PIDS+=($!)
    REPO_NAMES+=("$REPO_NAME")
    CLONE_DIRS+=("$CLONE_PATH")
done

# Wait for all and report results
FAILED=0
for i in "${!PIDS[@]}"; do
    NAME="${REPO_NAMES[$i]}"
    URL="${REPOS[$i]}"
    LOG_FILE="${WORK_DIR}/${NAME}.log"
    if wait "${PIDS[$i]}"; then
        echo
        echo "${C_GREEN}${C_BOLD}✔ ${NAME}${C_RESET} ${C_DIM}(${URL})${C_RESET}"
        [[ -f "$LOG_FILE" ]] && sed "s/^/  ${C_DIM}│${C_RESET} /" "$LOG_FILE"
    else
        echo
        echo "${C_RED}${C_BOLD}✘ ${NAME} FAILED${C_RESET} ${C_DIM}(${URL})${C_RESET}" >&2
        [[ -f "$LOG_FILE" ]] && sed "s/^/  ${C_RED}│${C_RESET} /" "$LOG_FILE" >&2
        FAILED=$((FAILED + 1))
    fi
done

echo
echo "${C_BOLD}────────────────────────────────────────${C_RESET}"
if [[ $FAILED -eq 0 ]]; then
    log "${C_GREEN}All ${#PIDS[@]} repo(s) succeeded${C_RESET}"
else
    log "${C_YELLOW}$((${#PIDS[@]} - FAILED))/${#PIDS[@]} succeeded, ${C_RED}${FAILED} failed${C_RESET}"
fi

# Collect PR URLs from every per-repo log so we can prompt once at the end.
PR_URLS=()
for i in "${!PIDS[@]}"; do
    LOG_FILE="${WORK_DIR}/${REPO_NAMES[$i]}.log"
    [[ -f "$LOG_FILE" ]] || continue
    URL="$(pr_prompt_extract_url "$(cat "$LOG_FILE")")"
    [[ -n "$URL" ]] && PR_URLS+=("$URL")
done

# Unset suppression so the prompt actually fires for THIS (top-level) script.
unset PR_PROMPT_SUPPRESS
pr_prompt_finalize "${PR_URLS[@]}"

# Clean up clones
log "Cleaning up: ${C_DIM}${WORK_DIR}${C_RESET}"
rm -rf "$WORK_DIR"

exit $FAILED
