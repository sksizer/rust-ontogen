#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROMPT_DIR="${SCRIPT_DIR}/quality_check"

# --- Configuration -----------------------------------------------------------
MAX_RETRIES="${MAX_RETRIES:-3}"

# --- Locate a JS package runner ----------------------------------------------
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

# --- Parse arguments ---------------------------------------------------------
EXECUTE=false
TARGET_DIR=""

for arg in "$@"; do
    case "$arg" in
        --execute) EXECUTE=true ;;
        *) TARGET_DIR="$arg" ;;
    esac
done

if [[ -n "$TARGET_DIR" ]]; then
    TARGET_DIR="$(cd "$TARGET_DIR" && pwd)"
else
    TARGET_DIR="$(pwd)"
fi

# --- Allowed tools -----------------------------------------------------------
ALLOWED_TOOLS="Read Edit Write Bash"

# --- Build the role prompt (shared across all fixers) ------------------------
ROLE_PROMPT=""
if [[ -f "${PROMPT_DIR}/role.md" ]]; then
    ROLE_PROMPT="$(cat "${PROMPT_DIR}/role.md")"
fi

# --- Checks to run (order matters) ------------------------------------------
# Each entry: "name|check_command|fix_prompt_file"
CHECKS=(
    "format|cargo fmt --all --check|fix_format.md"
    "lint|cargo clippy -- --deny warnings|fix_lint.md"
    "test|cargo test|fix_test.md"
)

# --- Run a single check, retrying with the fixer prompt on failure -----------
run_check() {
    local name="$1"
    local check_cmd="$2"
    local fix_file="$3"
    local attempt=0
    local check_output

    while true; do
        echo "--- Running check: ${name} (attempt $((attempt + 1))/${MAX_RETRIES}) ---"

        if check_output="$(cd "$TARGET_DIR" && eval "$check_cmd" 2>&1)"; then
            echo "  PASSED: ${name}"
            return 0
        fi

        attempt=$((attempt + 1))
        if (( attempt >= MAX_RETRIES )); then
            echo "  FAILED: ${name} after ${MAX_RETRIES} attempts" >&2
            echo "$check_output"
            return 1
        fi

        echo "  Check failed, invoking fixer (attempt ${attempt}/${MAX_RETRIES})..."

        if [[ "$EXECUTE" == true ]]; then
            local fix_prompt="${ROLE_PROMPT}"$'\n\n'
            fix_prompt+="$(cat "${PROMPT_DIR}/${fix_file}")"
            fix_prompt+=$'\n\n'"## Check output"$'\n\n'"\`\`\`"$'\n'"${check_output}"$'\n'"\`\`\`"

            cd "$TARGET_DIR"
            echo "${fix_prompt}" | ${RUNNER} @anthropic-ai/claude-code --print \
                --allowed-tools ${ALLOWED_TOOLS}
        else
            echo "  [dry-run] Would invoke fixer with prompt from: ${fix_file}"
            echo "  [dry-run] Check output was:"
            echo "$check_output" | head -20
            return 1
        fi
    done
}

# --- Main --------------------------------------------------------------------
FAILED=0

echo "Quality check: ${TARGET_DIR}"
echo "Max retries per check: ${MAX_RETRIES}"
echo "Mode: $(if [[ "$EXECUTE" == true ]]; then echo "execute"; else echo "dry-run"; fi)"
echo ""

for entry in "${CHECKS[@]}"; do
    IFS='|' read -r name check_cmd fix_file <<< "$entry"
    if ! run_check "$name" "$check_cmd" "$fix_file"; then
        FAILED=$((FAILED + 1))
        # Stop on first unrecoverable failure
        echo ""
        echo "Stopping: ${name} could not be fixed."
        break
    fi
done

if (( FAILED == 0 )); then
    echo ""
    echo "All checks passed."
else
    exit 1
fi
