#!/bin/sh
# dev-port.sh — Deterministically derive a block of 4 consecutive ports from $(pwd).
#
# Usage:
#   dev-port.sh --service nuxt|storybook|mcp|http   # print one port
#   dev-port.sh --base                               # print base port only
#   dev-port.sh --all                                # print KEY=PORT pairs (eval-able)
#
# Port block layout (base + offset):
#   nuxt=+0  storybook=+1  mcp=+2  http=+3

set -e

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
BLOCK_SIZE=4
PORT_MIN=3000
PORT_MAX=9996          # leaves room for base + 3
FALLBACK_BASE=1420     # default if hashing is unavailable

# ---------------------------------------------------------------------------
# Hash the current working directory to a number in [PORT_MIN, PORT_MAX].
# Uses cksum (POSIX) — portable across macOS / Linux / BSD.
# Result is aligned to BLOCK_SIZE so the base always starts on a block boundary.
# ---------------------------------------------------------------------------
hash_directory() {
    dir="$(pwd)"
    raw=$(printf '%s' "$dir" | cksum | awk '{print $1}')
    range=$(( PORT_MAX - PORT_MIN + 1 ))
    base=$(( (raw % range) + PORT_MIN ))
    # Align to block boundary
    base=$(( base - (base % BLOCK_SIZE) ))
    # Ensure we're still in range after alignment
    if [ "$base" -lt "$PORT_MIN" ]; then
        base=$(( base + BLOCK_SIZE ))
    fi
    printf '%s' "$base"
}

# ---------------------------------------------------------------------------
# Check whether a single port is in use.
# Returns 0 if the port IS occupied, 1 if free.
# Uses /dev/tcp where available, falls back to lsof, then ss, then netstat.
# ---------------------------------------------------------------------------
port_occupied() {
    port="$1"
    if command -v lsof >/dev/null 2>&1; then
        lsof -iTCP:"$port" -sTCP:LISTEN -P -n >/dev/null 2>&1 && return 0
    elif command -v ss >/dev/null 2>&1; then
        ss -tlnp 2>/dev/null | grep -q ":${port} " && return 0
    elif command -v netstat >/dev/null 2>&1; then
        netstat -an 2>/dev/null | grep -q "[:.]${port} .*LISTEN" && return 0
    fi
    return 1
}

# ---------------------------------------------------------------------------
# Check whether an entire block of BLOCK_SIZE ports starting at $1 is free.
# Returns 0 if ALL ports in the block are free, 1 otherwise.
# ---------------------------------------------------------------------------
block_free() {
    b="$1"
    i=0
    while [ "$i" -lt "$BLOCK_SIZE" ]; do
        if port_occupied $(( b + i )); then
            return 1
        fi
        i=$(( i + 1 ))
    done
    return 0
}

# ---------------------------------------------------------------------------
# Find a free block starting from the hashed base, stepping by BLOCK_SIZE.
# Wraps around once, then gives up.
# ---------------------------------------------------------------------------
find_free_block() {
    start="$1"
    candidate="$start"

    # Forward scan from candidate to PORT_MAX
    while [ "$candidate" -le "$PORT_MAX" ]; do
        if block_free "$candidate"; then
            printf '%s' "$candidate"
            return 0
        fi
        candidate=$(( candidate + BLOCK_SIZE ))
    done

    # Wrap: scan from PORT_MIN (aligned) up to start
    candidate=$(( PORT_MIN - (PORT_MIN % BLOCK_SIZE) ))
    if [ "$candidate" -lt "$PORT_MIN" ]; then
        candidate=$(( candidate + BLOCK_SIZE ))
    fi
    while [ "$candidate" -lt "$start" ]; do
        if block_free "$candidate"; then
            printf '%s' "$candidate"
            return 0
        fi
        candidate=$(( candidate + BLOCK_SIZE ))
    done

    return 1
}

# ---------------------------------------------------------------------------
# Map service name → offset
# ---------------------------------------------------------------------------
service_offset() {
    case "$1" in
        nuxt)      printf '0' ;;
        storybook) printf '1' ;;
        mcp)       printf '2' ;;
        http)      printf '3' ;;
        *)
            echo "Error: unknown service '$1'. Use: nuxt, storybook, mcp, http" >&2
            exit 1
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
MODE=""
SERVICE=""

while [ $# -gt 0 ]; do
    case "$1" in
        --service)
            MODE="service"
            shift
            SERVICE="$1"
            ;;
        --base)
            MODE="base"
            ;;
        --all)
            MODE="all"
            ;;
        -h|--help)
            echo "Usage: dev-port.sh [--service nuxt|storybook|mcp|http] [--base] [--all]" >&2
            exit 0
            ;;
        *)
            echo "Error: unknown option '$1'" >&2
            exit 1
            ;;
    esac
    shift
done

if [ -z "$MODE" ]; then
    echo "Error: specify --service <name>, --base, or --all" >&2
    exit 1
fi

# Compute deterministic base
ideal_base=$(hash_directory)
base=$(find_free_block "$ideal_base") || {
    echo "Error: no free port block found in range ${PORT_MIN}-${PORT_MAX}" >&2
    exit 1
}

case "$MODE" in
    base)
        printf '%s\n' "$base"
        ;;
    service)
        offset=$(service_offset "$SERVICE")
        printf '%s\n' "$(( base + offset ))"
        ;;
    all)
        printf 'TAURI_DEV_PORT=%s\n' "$base"
        printf 'STORYBOOK_PORT=%s\n' "$(( base + 1 ))"
        printf 'MCP_PORT=%s\n' "$(( base + 2 ))"
        printf 'HTTP_PORT=%s\n' "$(( base + 3 ))"
        ;;
esac
