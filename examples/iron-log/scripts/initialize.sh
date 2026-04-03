#!/bin/bash
# initialize.sh — Interactive project initialization
#
# Prompts for a project name and bundle ID, then delegates to rename.sh
# to update all template references.
#
# Usage:
#   bash scripts/initialize.sh
#   make initialize
#   just initialize

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo ""
echo "=== Project Initialization ==="
echo ""
echo "This will rename the template defaults (tauri_nuxt / com.sksizer.example)"
echo "to your project's name and bundle identifier."
echo ""

# Prompt for project name
read -rp "Project name (snake_case, e.g. my_app): " PROJECT_NAME

if [ -z "$PROJECT_NAME" ]; then
    echo "Error: project name cannot be empty." >&2
    exit 1
fi

# Validate snake_case
if ! echo "$PROJECT_NAME" | grep -qE '^[a-z][a-z0-9_]*$'; then
    echo "Error: project name must be snake_case (lowercase letters, digits, underscores; must start with a letter)." >&2
    exit 1
fi

# Prompt for bundle ID
read -rp "Bundle ID (reverse-domain, e.g. com.example.myapp): " BUNDLE_ID

if [ -z "$BUNDLE_ID" ]; then
    echo "Error: bundle ID cannot be empty." >&2
    exit 1
fi

# Validate reverse-domain format
if ! echo "$BUNDLE_ID" | grep -qE '^[a-z][a-z0-9]*(\.[a-z][a-z0-9]*)+$'; then
    echo "Error: bundle ID must be reverse-domain notation (e.g. com.example.myapp)." >&2
    exit 1
fi

echo ""
echo "Renaming project to: $PROJECT_NAME"
echo "Bundle ID:           $BUNDLE_ID"
echo ""

export PROJECT_NAME
export BUNDLE_ID
bash "$SCRIPT_DIR/rename.sh"

echo ""
echo "Done! Project initialized as '$PROJECT_NAME' ($BUNDLE_ID)."
echo ""
