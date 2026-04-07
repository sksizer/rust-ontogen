#!/usr/bin/env bash
# Shared helpers for collecting PR URLs and prompting the user whether to open
# them at the end of a script run. Source this file — don't execute it.
#
# Environment:
#   PR_PROMPT_SUPPRESS=1  — disable the interactive prompt entirely. Parent
#                           drivers (e.g. *_all.sh) set this when invoking
#                           single-repo scripts so only the outermost script
#                           asks. Also honored by CI via: PR_PROMPT_SUPPRESS=1

# Open a URL in the default browser (best-effort, never fails the caller).
pr_prompt__open_url() {
    local url="$1"
    case "$(uname -s)" in
        Darwin)  open "$url" ;;
        Linux)   xdg-open "$url" ;;
        MINGW*|MSYS*|CYGWIN*) cmd.exe /c start "$url" ;;
    esac 2>/dev/null || true
}

# Extract the first GitHub PR URL from a blob of text. Prints nothing if none.
pr_prompt_extract_url() {
    grep -oE 'https://github\.com/[^[:space:]]+/pull/[0-9]+' <<<"$1" | head -1 || true
}

# Given a list of PR URLs (one per argument), show them and — if stdin is a
# TTY and PR_PROMPT_SUPPRESS is unset — ask whether to open them all now.
# No-op if no URLs are passed.
pr_prompt_finalize() {
    local urls=("$@")
    [[ ${#urls[@]} -eq 0 ]] && return 0

    echo
    echo "PR URL(s):"
    for url in "${urls[@]}"; do
        echo "  $url"
    done

    if [[ -n "${PR_PROMPT_SUPPRESS:-}" ]]; then
        return 0
    fi
    if [[ ! -t 0 || ! -t 1 ]]; then
        echo "(non-interactive; skipping open prompt)"
        return 0
    fi

    echo
    local reply=""
    read -r -p "Open $( [[ ${#urls[@]} -gt 1 ]] && echo "all ${#urls[@]} PRs" || echo "PR" ) in browser now? [y/N] " reply || return 0
    case "$reply" in
        y|Y|yes|YES)
            for url in "${urls[@]}"; do
                pr_prompt__open_url "$url"
            done
            ;;
        *) ;;
    esac
}
