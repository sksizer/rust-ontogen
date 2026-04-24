#!/usr/bin/env bash
# Shared helpers for reading cousins.json. Source this file — don't execute it.

cousins__require_jq() {
    if ! command -v jq &>/dev/null; then
        echo "Error: jq is required but not installed. Install with: brew install jq" >&2
        exit 1
    fi
}

# Print the path to cousins.json, respecting an optional override via
# COUSINS_CONFIG env var.
cousins_config_path() {
    local default="$1"   # caller passes scripts dir
    echo "${COUSINS_CONFIG:-${default}/cousins.json}"
}

# Print the JSON object for a single cousin by .name, or empty string if not
# found. Arg 1: path to cousins.json, arg 2: cousin name.
cousins_get_by_name() {
    local config_path="$1"
    local name="$2"
    jq -c --arg n "$name" '.cousins[] | select(.name == $n)' "$config_path"
}

# Print all cousin names, one per line. Arg 1: path to cousins.json.
cousins_list_names() {
    local config_path="$1"
    jq -r '.cousins[].name' "$config_path"
}
