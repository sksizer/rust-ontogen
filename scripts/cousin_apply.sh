#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROMPT_DIR="${SCRIPT_DIR}/cousin_apply"
DEFAULT_TEMPLATE_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# shellcheck source=lib/cousins.sh
source "${SCRIPT_DIR}/lib/cousins.sh"
# shellcheck source=lib/pr_prompt.sh
source "${SCRIPT_DIR}/lib/pr_prompt.sh"
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

usage() {
    cat >&2 <<EOF
Usage: $0 [--execute] --cousin <name>

Applies high-confidence template-sourced changes to a cousin's opted-in paths,
runs per-target checks, and opens a single PR against the cousin repo.

This script ALWAYS clones the cousin fresh into a tempdir so your local
working copies are never touched. There is no local-path override.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --execute) EXECUTE=true; shift ;;
        --cousin)  COUSIN_NAME="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *)         echo "Unknown arg: $1" >&2; usage; exit 2 ;;
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

# Refuse to run if the cousin has no opted-in targets — this is a no-op.
TARGET_COUNT="$(jq '(.targets // []) | length + ((.global_applies // []) | length)' <<<"$COUSIN_CONFIG_JSON")"
if [[ "$TARGET_COUNT" -eq 0 ]]; then
    echo "Cousin '${COUSIN_NAME}' has no targets or global_applies — nothing to do."
    exit 0
fi

COUSIN_URL="$(jq -r '.url' <<<"$COUSIN_CONFIG_JSON")"
TEMPLATE_DIR="${TEMPLATE_DIR:-$DEFAULT_TEMPLATE_DIR}"

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

# Apply mode: Claude edits files, runs checks, and creates a PR via gh.
ALLOWED_TOOLS="Read Edit Write Bash"

if [[ "$EXECUTE" == true ]]; then
    WORK_DIR="$(mktemp -d)"
    CLONE_PATH="${WORK_DIR}/${COUSIN_NAME}"
    echo "Runner:        ${RUNNER}"
    echo "Cousin:        ${COUSIN_NAME} (${COUSIN_URL})"
    echo "Clone path:    ${CLONE_PATH}"
    echo "Template:      ${TEMPLATE_DIR}"
    echo "Prompt length: ${#PROMPT} chars"
    echo "Allowed tools: ${ALLOWED_TOOLS}"
    echo "---"

    git clone --quiet "$COUSIN_URL" "$CLONE_PATH"

    cd "$CLONE_PATH"
    export TEMPLATE_DIR COUSIN_CONFIG_JSON COUSIN_NAME
    OUTPUT="$(echo "${PROMPT}" | ${RUNNER} @anthropic-ai/claude-code --print \
        --allowed-tools ${ALLOWED_TOOLS})"
    echo "$OUTPUT"

    PR_URL="$(pr_prompt_extract_url "$OUTPUT")"
    if [[ -n "$PR_URL" ]]; then
        pr_prompt_finalize "$PR_URL"
    else
        echo "No PR opened (no changes survived, or run was a no-op)."
    fi

    echo "Clone left at: ${CLONE_PATH}"
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
    echo "Pass --execute to actually run (clones the cousin fresh into a tempdir)."
fi
