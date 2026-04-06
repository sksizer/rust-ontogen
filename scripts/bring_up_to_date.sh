#!/usr/bin/env bash
# 'standard' strict mode preamble
# -e exit immediately
# -u treat unset variables as error,
# -o exit code is rightmost command that failed through | commands like some_cmd | grep foo - it captures a failure from some_cmd.
set -euo pipefail

# gets the dir of the path invoking the script (that could be relative like ./scripts/bring_up_to_date.sh)
# cd into that directory, then calls pwd to get the absolute path to the directory with the script to base
# the rest of the paths on
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROMPT_DIR="${SCRIPT_DIR}/bring_up_to_date"

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

# --- Compose the prompt from markdown files ---------------------------------
# role.md first, then remaining files in sorted order
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

# --- Open a URL in the default browser (best-effort, never fails the script)
open_url() {
    local url="$1"
    case "$(uname -s)" in
        Darwin)  open "$url" ;;
        Linux)   xdg-open "$url" ;;
        MINGW*|MSYS*|CYGWIN*) cmd.exe /c start "$url" ;;
    esac 2>/dev/null || true
}

# --- Allowed tools ----------------------------------------------------------
# Read/Edit/Write for file changes, Bash for just commands and git, WebFetch for fetching upstream
ALLOWED_TOOLS="Read Edit Write Bash WebFetch"

# --- Execute or dry-run -----------------------------------------------------
if [[ "$EXECUTE" == true ]]; then
    echo "Using runner: ${RUNNER}"
    echo "Target: ${TARGET_DIR}"
    echo "Prompt length: ${#PROMPT} chars"
    echo "Allowed tools: ${ALLOWED_TOOLS}"
    echo "---"
    cd "$TARGET_DIR"
    OUTPUT="$(echo "${PROMPT}" | ${RUNNER} @anthropic-ai/claude-code --print \
        --allowed-tools ${ALLOWED_TOOLS})"
    echo "$OUTPUT"

    # Try to extract a PR URL from the output and open it
    PR_URL="$(echo "$OUTPUT" | grep -oE 'https://github\.com/[^ ]+/pull/[0-9]+' | head -1 || true)"
    if [[ -n "$PR_URL" ]]; then
        echo "Opening PR: ${PR_URL}"
        open_url "$PR_URL"
    fi
else
    echo "=== DRY RUN ==="
    echo ""
    echo "${PROMPT}"
    echo "---"
    echo "Runner: ${RUNNER}"
    echo "Target: ${TARGET_DIR}"
    echo "Prompt length: ${#PROMPT} chars"
    echo "Allowed tools: ${ALLOWED_TOOLS}"
    echo ""
    echo "Would run:"
    echo "  cd ${TARGET_DIR}"
    echo "  echo \"\${PROMPT}\" | ${RUNNER} @anthropic-ai/claude-code --print --allowed-tools ${ALLOWED_TOOLS}"
    echo ""
    echo "Pass --execute to run this against Claude Code."
fi
