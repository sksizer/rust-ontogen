#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROMPT_DIR="${SCRIPT_DIR}/template_backport"
TEMPLATE_REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

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
DOWNSTREAM_DIR=""

for arg in "$@"; do
    case "$arg" in
        --execute) EXECUTE=true ;;
        *) DOWNSTREAM_DIR="$arg" ;;
    esac
done

if [[ -z "$DOWNSTREAM_DIR" ]]; then
    echo "Usage: $0 [--execute] <downstream_repo_path>" >&2
    exit 2
fi
DOWNSTREAM_DIR="$(cd "$DOWNSTREAM_DIR" && pwd)"
DOWNSTREAM_NAME="$(basename "$DOWNSTREAM_DIR")"

# --- Determine the template's git origin URL (so we can clone fresh) --------
TEMPLATE_ORIGIN_URL="${TEMPLATE_ORIGIN_URL:-$(git -C "$TEMPLATE_REPO_ROOT" remote get-url origin)}"

# --- Compose the prompt -----------------------------------------------------
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

# --- Open a URL in the default browser (best-effort) -----------------------
open_url() {
    local url="$1"
    case "$(uname -s)" in
        Darwin)  open "$url" ;;
        Linux)   xdg-open "$url" ;;
        MINGW*|MSYS*|CYGWIN*) cmd.exe /c start "$url" ;;
    esac 2>/dev/null || true
}

# --- Allowed tools ----------------------------------------------------------
# Backport mode: Claude must edit files, run checks, and create a PR.
ALLOWED_TOOLS="Read Edit Write Bash"

# --- Execute or dry-run -----------------------------------------------------
if [[ "$EXECUTE" == true ]]; then
    # Fresh clone of the template into a temp dir — never edit the user's working copy.
    WORK_DIR="$(mktemp -d)"
    TEMPLATE_CLONE="${WORK_DIR}/template"
    echo "Runner:           ${RUNNER}"
    echo "Downstream:       ${DOWNSTREAM_DIR} (${DOWNSTREAM_NAME})"
    echo "Template origin:  ${TEMPLATE_ORIGIN_URL}"
    echo "Template clone:   ${TEMPLATE_CLONE}"
    echo "Prompt length:    ${#PROMPT} chars"
    echo "Allowed tools:    ${ALLOWED_TOOLS}"
    echo "---"

    git clone --quiet "$TEMPLATE_ORIGIN_URL" "$TEMPLATE_CLONE"

    cd "$TEMPLATE_CLONE"
    export DOWNSTREAM_DIR DOWNSTREAM_NAME
    OUTPUT="$(echo "${PROMPT}" | ${RUNNER} @anthropic-ai/claude-code --print \
        --allowed-tools ${ALLOWED_TOOLS})"
    echo "$OUTPUT"

    PR_URL="$(echo "$OUTPUT" | grep -oE 'https://github\.com/[^ ]+/pull/[0-9]+' | head -1 || true)"
    if [[ -n "$PR_URL" ]]; then
        echo "Opening PR: ${PR_URL}"
        open_url "$PR_URL"
    else
        echo "No PR opened (no candidates, or run was a no-op)."
    fi

    echo "Template clone left at: ${TEMPLATE_CLONE}"
else
    echo "=== DRY RUN ==="
    echo
    echo "${PROMPT}"
    echo "---"
    echo "Runner:           ${RUNNER}"
    echo "Downstream:       ${DOWNSTREAM_DIR} (${DOWNSTREAM_NAME})"
    echo "Template origin:  ${TEMPLATE_ORIGIN_URL}"
    echo "Prompt length:    ${#PROMPT} chars"
    echo "Allowed tools:    ${ALLOWED_TOOLS}"
    echo
    echo "Would clone the template fresh, set DOWNSTREAM_DIR/DOWNSTREAM_NAME,"
    echo "and run Claude to apply backports + open a PR."
    echo
    echo "Pass --execute to actually run."
fi
