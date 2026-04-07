#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROMPT_DIR="${SCRIPT_DIR}/template_review"
# The template repo is the parent of scripts/
DEFAULT_TEMPLATE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# --- Locate a JS package runner (pnpm dlx preferred, npx fallback) ----------
find_runner() {
    if command -v pnpm &>/dev/null; then
        echo "pnpm dlx"
    elif command -v npx &>/dev/null; then
        echo "npx"
    else
        echo "Error: neither pnpm nor npx found. Install one of them first." >&2
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

# Allow caller to override the template location
TEMPLATE_DIR="${TEMPLATE_DIR:-$DEFAULT_TEMPLATE_DIR}"

# --- Compose the prompt from markdown files ---------------------------------
compose_prompt() {
    local prompt=""

    if [[ -f "${PROMPT_DIR}/role.md" ]]; then
        prompt+="$(cat "${PROMPT_DIR}/role.md")"
        prompt+=$'\n\n'
    fi

    for f in "${PROMPT_DIR}"/*.md; do
        [[ "$(basename "$f")" == "role.md" ]] && continue
        prompt+="$(cat "$f")"
        prompt+=$'\n\n'
    done

    echo "$prompt"
}

PROMPT="$(compose_prompt)"

# --- Allowed tools ----------------------------------------------------------
# Read-only review: no Edit/Write, no git mutations.
ALLOWED_TOOLS="Read Bash"

# --- Execute or dry-run -----------------------------------------------------
if [[ "$EXECUTE" == true ]]; then
    echo "Using runner:  ${RUNNER}"
    echo "Target:        ${TARGET_DIR}"
    echo "Template:      ${TEMPLATE_DIR}"
    echo "Prompt length: ${#PROMPT} chars"
    echo "Allowed tools: ${ALLOWED_TOOLS}"
    echo "---"
    cd "$TARGET_DIR"
    export TEMPLATE_DIR
    echo "${PROMPT}" | ${RUNNER} @anthropic-ai/claude-code --print \
        --allowed-tools ${ALLOWED_TOOLS}
else
    echo "=== DRY RUN ==="
    echo ""
    echo "${PROMPT}"
    echo "---"
    echo "Runner:        ${RUNNER}"
    echo "Target:        ${TARGET_DIR}"
    echo "Template:      ${TEMPLATE_DIR}"
    echo "Prompt length: ${#PROMPT} chars"
    echo "Allowed tools: ${ALLOWED_TOOLS}"
    echo ""
    echo "Would run:"
    echo "  cd ${TARGET_DIR}"
    echo "  TEMPLATE_DIR=${TEMPLATE_DIR} echo \"\${PROMPT}\" | ${RUNNER} @anthropic-ai/claude-code --print --allowed-tools ${ALLOWED_TOOLS}"
    echo ""
    echo "Pass --execute to run this against Claude Code."
fi
