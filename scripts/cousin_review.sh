#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROMPT_DIR="${SCRIPT_DIR}/cousin_review"
DEFAULT_TEMPLATE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# shellcheck source=lib/cousins.sh
source "${SCRIPT_DIR}/lib/cousins.sh"
cousins__require_jq

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
COUSIN_NAME=""
CLONE_PATH=""

usage() {
    cat >&2 <<EOF
Usage: $0 [--execute] --cousin <name> [<local_clone_path>]

Reviews one cousin repo defined in scripts/cousins.json (or COUSINS_CONFIG
override) for template-sourced improvements scoped to its opted-in paths.

If <local_clone_path> is omitted, the cousin's 'url' is cloned into a tempdir.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --execute) EXECUTE=true; shift ;;
        --cousin)  COUSIN_NAME="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *)         CLONE_PATH="$1"; shift ;;
    esac
done

if [[ -z "$COUSIN_NAME" ]]; then
    usage
    exit 2
fi

CONFIG_PATH="$(cousins_config_path "$SCRIPT_DIR")"
if [[ ! -f "$CONFIG_PATH" ]]; then
    echo "Error: cousins config not found at ${CONFIG_PATH}" >&2
    exit 1
fi

COUSIN_CONFIG_JSON="$(cousins_get_by_name "$CONFIG_PATH" "$COUSIN_NAME")"
if [[ -z "$COUSIN_CONFIG_JSON" ]]; then
    echo "Error: no cousin named '${COUSIN_NAME}' in ${CONFIG_PATH}" >&2
    exit 1
fi

COUSIN_URL="$(jq -r '.url' <<<"$COUSIN_CONFIG_JSON")"

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

# Read-only review: no Edit/Write, no git mutations.
ALLOWED_TOOLS="Read Bash"

if [[ "$EXECUTE" == true ]]; then
    # Obtain a clone if one wasn't provided.
    OWN_CLONE=false
    if [[ -z "$CLONE_PATH" ]]; then
        WORK_DIR="$(mktemp -d)"
        CLONE_PATH="${WORK_DIR}/${COUSIN_NAME}"
        git clone --quiet "$COUSIN_URL" "$CLONE_PATH"
        OWN_CLONE=true
    else
        CLONE_PATH="$(cd "$CLONE_PATH" && pwd)"
    fi

    echo "Runner:        ${RUNNER}"
    echo "Cousin:        ${COUSIN_NAME} (${COUSIN_URL})"
    echo "Target:        ${CLONE_PATH}"
    echo "Template:      ${TEMPLATE_DIR}"
    echo "Prompt length: ${#PROMPT} chars"
    echo "Allowed tools: ${ALLOWED_TOOLS}"
    echo "---"

    cd "$CLONE_PATH"
    export TEMPLATE_DIR COUSIN_CONFIG_JSON COUSIN_NAME
    echo "${PROMPT}" | ${RUNNER} @anthropic-ai/claude-code --print \
        --allowed-tools ${ALLOWED_TOOLS}

    if [[ "$OWN_CLONE" == true ]]; then
        echo "Clone left at: ${CLONE_PATH}"
    fi
else
    echo "=== DRY RUN ==="
    echo
    echo "${PROMPT}"
    echo "---"
    echo "Runner:        ${RUNNER}"
    echo "Cousin:        ${COUSIN_NAME} (${COUSIN_URL})"
    echo "Template:      ${TEMPLATE_DIR}"
    echo "Config:        ${CONFIG_PATH}"
    echo "Prompt length: ${#PROMPT} chars"
    echo "Allowed tools: ${ALLOWED_TOOLS}"
    echo
    echo "Pass --execute to actually run (will clone the cousin if no path given)."
fi
