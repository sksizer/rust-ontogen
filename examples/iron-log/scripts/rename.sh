#!/bin/bash
# rename.sh — Non-interactive project rename
#
# Replaces all template default references (tauri_nuxt / com.sksizer.example)
# with the values provided via environment variables.
#
# Usage:
#   PROJECT_NAME=my_app BUNDLE_ID=com.example.myapp bash scripts/rename.sh
#   PROJECT_NAME=my_app BUNDLE_ID=com.example.myapp make rename
#
# Environment variables:
#   PROJECT_NAME  — New project name in snake_case (e.g. my_app)
#   BUNDLE_ID     — New bundle identifier in reverse-domain notation (e.g. com.example.myapp)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

OLD_NAME="tauri_nuxt"
OLD_BUNDLE_ID="com.sksizer.example"

if [ -z "${PROJECT_NAME:-}" ]; then
    echo "Error: PROJECT_NAME environment variable is required." >&2
    echo "Usage: PROJECT_NAME=my_app BUNDLE_ID=com.example.myapp bash $0" >&2
    exit 1
fi

if [ -z "${BUNDLE_ID:-}" ]; then
    echo "Error: BUNDLE_ID environment variable is required." >&2
    echo "Usage: PROJECT_NAME=my_app BUNDLE_ID=com.example.myapp bash $0" >&2
    exit 1
fi

# Derive the lib name (Cargo convention: <name>_lib)
OLD_LIB_NAME="${OLD_NAME}_lib"
NEW_LIB_NAME="${PROJECT_NAME}_lib"

echo "Renaming: $OLD_NAME -> $PROJECT_NAME"
echo "Bundle:   $OLD_BUNDLE_ID -> $BUNDLE_ID"
echo ""

# --- Root package.json ---
echo "  Updating package.json ..."
sed -i.bak "s/\"name\": \"${OLD_NAME}\"/\"name\": \"${PROJECT_NAME}\"/" "$PROJECT_ROOT/package.json"
rm -f "$PROJECT_ROOT/package.json.bak"

# --- src-tauri/tauri.conf.json ---
echo "  Updating src-tauri/tauri.conf.json ..."
sed -i.bak \
    -e "s/\"productName\": \"${OLD_NAME}\"/\"productName\": \"${PROJECT_NAME}\"/" \
    -e "s/\"identifier\": \"${OLD_BUNDLE_ID}\"/\"identifier\": \"${BUNDLE_ID}\"/" \
    -e "s/\"title\": \"${OLD_NAME}\"/\"title\": \"${PROJECT_NAME}\"/" \
    "$PROJECT_ROOT/src-tauri/tauri.conf.json"
rm -f "$PROJECT_ROOT/src-tauri/tauri.conf.json.bak"

# --- src-tauri/Cargo.toml ---
echo "  Updating src-tauri/Cargo.toml ..."
sed -i.bak \
    -e "s/^name = \"${OLD_NAME}\"/name = \"${PROJECT_NAME}\"/" \
    -e "s/^name = \"${OLD_LIB_NAME}\"/name = \"${NEW_LIB_NAME}\"/" \
    "$PROJECT_ROOT/src-tauri/Cargo.toml"
rm -f "$PROJECT_ROOT/src-tauri/Cargo.toml.bak"

echo ""
echo "Rename complete."
