#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROMPT_DIR="${SCRIPT_DIR}/cargo_update"

# --- Colors (disabled when not a tty or NO_COLOR is set) --------------------
if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
    C_RESET=$'\033[0m'; C_BOLD=$'\033[1m'; C_DIM=$'\033[2m'
    C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'
    C_BLUE=$'\033[34m'; C_MAGENTA=$'\033[35m'; C_CYAN=$'\033[36m'
else
    C_RESET=; C_BOLD=; C_DIM=; C_RED=; C_GREEN=; C_YELLOW=; C_BLUE=; C_MAGENTA=; C_CYAN=
fi

# REPO_LABEL can be set by the caller (e.g. cargo_update_all.sh) to prefix logs
LABEL="${REPO_LABEL:-cargo_update}"
log()   { echo "${C_CYAN}[${LABEL}]${C_RESET} $*"; }
info()  { echo "${C_BLUE}[${LABEL}]${C_RESET} $*"; }
warn()  { echo "${C_YELLOW}[${LABEL}]${C_RESET} $*" >&2; }
error() { echo "${C_RED}[${LABEL}] ERROR:${C_RESET} $*" >&2; }

# --- Locate a JS package runner (pnpm dlx preferred, npx fallback) ----------
find_runner() {
    if command -v pnpm &>/dev/null; then
        echo "pnpm dlx"
    elif command -v npx &>/dev/null; then
        echo "npx"
    else
        error "neither pnpm nor npx found. Install one of them first."
        exit 1
    fi
}

RUNNER="$(find_runner)"

# --- Parse arguments --------------------------------------------------------
EXECUTE=false
TARGET_DIR=""

for arg in "$@"; do
    case "$arg" in
        --execute) EXECUTE=true ;;
        *) TARGET_DIR="$arg" ;;
    esac
done

# Resolve target directory (default: current directory)
if [[ -n "$TARGET_DIR" ]]; then
    TARGET_DIR="$(cd "$TARGET_DIR" && pwd)"
else
    TARGET_DIR="$(pwd)"
fi

# --- Compose the prompt from markdown files ---------------------------------
compose_prompt() {
    local prompt=""

    # Role comes first
    if [[ -f "${PROMPT_DIR}/role.md" ]]; then
        prompt+="$(cat "${PROMPT_DIR}/role.md")"
        prompt+=$'\n\n'
    fi

    # Then every other .md file in sorted order
    for f in "${PROMPT_DIR}"/*.md; do
        [[ "$(basename "$f")" == "role.md" ]] && continue
        prompt+="$(cat "$f")"
        prompt+=$'\n\n'
    done

    echo "$prompt"
}

PROMPT="$(compose_prompt)"

# --- Shared PR prompt helpers ----------------------------------------------
# shellcheck source=lib/pr_prompt.sh
source "${SCRIPT_DIR}/lib/pr_prompt.sh"

# --- Allowed tools ----------------------------------------------------------
ALLOWED_TOOLS="Read Edit Write Bash"

# --- Execute or dry-run -----------------------------------------------------
print_config() {
    info "Runner:        ${C_BOLD}${RUNNER}${C_RESET}"
    info "Target:        ${C_BOLD}${TARGET_DIR}${C_RESET}"
    info "Prompt length: ${C_BOLD}${#PROMPT}${C_RESET} chars"
    info "Allowed tools: ${C_DIM}${ALLOWED_TOOLS}${C_RESET}"
}

if [[ "$EXECUTE" == true ]]; then
    log "${C_GREEN}${C_BOLD}▶ EXECUTE${C_RESET} updating ${C_MAGENTA}${LABEL}${C_RESET}"
    print_config
    log "${C_DIM}──── claude output ────${C_RESET}"
    cd "$TARGET_DIR"
    OUTPUT="$(echo "${PROMPT}" | ${RUNNER} @anthropic-ai/claude-code --print \
        --allowed-tools ${ALLOWED_TOOLS})"
    echo "$OUTPUT"
    log "${C_DIM}──── end output ────${C_RESET}"

    PR_URL="$(pr_prompt_extract_url "$OUTPUT")"
    if [[ -n "$PR_URL" ]]; then
        log "${C_GREEN}✔ PR for ${C_MAGENTA}${LABEL}${C_GREEN}:${C_RESET} ${C_BOLD}${PR_URL}${C_RESET}"
        pr_prompt_finalize "$PR_URL"
    else
        log "${C_YELLOW}No PR URL detected in output for ${LABEL}${C_RESET}"
    fi
else
    log "${C_YELLOW}${C_BOLD}=== DRY RUN ===${C_RESET} (${C_MAGENTA}${LABEL}${C_RESET})"
    echo
    echo "${PROMPT}"
    log "${C_DIM}────────────────${C_RESET}"
    print_config
    echo
    log "Would run:"
    echo "  ${C_DIM}cd ${TARGET_DIR}${C_RESET}"
    echo "  ${C_DIM}echo \"\${PROMPT}\" | ${RUNNER} @anthropic-ai/claude-code --print --allowed-tools ${ALLOWED_TOOLS}${C_RESET}"
    echo
    log "Pass ${C_BOLD}--execute${C_RESET} to run this against Claude Code."
fi
