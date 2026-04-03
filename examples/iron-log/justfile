# Default recipe: show help
default:
    @just --list

## Initialization ------------------------------------------------------------

# Run project initialization script
initialize:
    scripts/initialize.sh

# Run project rename script
rename:
    scripts/rename.sh

## Development ---------------------------------------------------------------

# Run tauri dev server (auto-assigned port)
dev:
    pnpm tauri dev

# Build for production
build:
    pnpm tauri build

# Build with debug symbols
build-debug:
    pnpm tauri build --debug

# Launch Storybook dev server (auto-assigned port)
storybook:
    pnpm run storybook

# Show auto-assigned port block for this worktree
ports:
    @scripts/dev-port.sh --all

## Frontend ------------------------------------------------------------------

# Run Nuxt dev server
frontend-dev:
    cd src-nuxt && pnpm run dev

# Build Nuxt for production
frontend-build:
    cd src-nuxt && pnpm run build

# Generate static Nuxt site
frontend-generate:
    cd src-nuxt && pnpm run generate

# Preview Nuxt production build
frontend-preview:
    cd src-nuxt && pnpm run preview

# Run frontend linter
frontend-lint:
    pnpm run frontend:lint

# Run frontend tests
frontend-test:
    pnpm run frontend:test

# Run frontend type checking
frontend-typecheck:
    pnpm run frontend:typecheck

# Format frontend code
frontend-format:
    cd src-nuxt && pnpm run format

# Check frontend formatting
frontend-format-check:
    cd src-nuxt && pnpm run format:check

## Linting & Formatting ------------------------------------------------------

# Run all linters (frontend + Rust)
lint:
    pnpm run frontend:lint
    cd src-tauri && cargo clippy -- -D warnings

# Auto-fix lint issues
lint-fix:
    cd src-nuxt && pnpm eslint . --fix
    cd src-tauri && cargo clippy --fix --allow-dirty

# Format all code
format:
    pnpm run format

# Check formatting without changes
format-check:
    pnpm run format:check
    cd src-tauri && cargo fmt -- --check

# Run frontend type checking
typecheck:
    pnpm run frontend:typecheck

## Testing -------------------------------------------------------------------

# Run all tests (frontend + Rust)
test:
    pnpm run frontend:test
    cd src-tauri && cargo test

# Run frontend unit tests only
test-unit:
    pnpm run frontend:test

## Rust ----------------------------------------------------------------------

# Run cargo clippy
rust-lint:
    cd src-tauri && cargo clippy -- -D warnings

# Run cargo fmt
rust-format:
    cd src-tauri && cargo fmt

# Run cargo test
rust-test:
    cd src-tauri && cargo test

# Alias for rust-lint
backend-lint: rust-lint

# Check backend formatting
backend-format-check:
    pnpm run backend:format:check

# Alias for rust-test
backend-test: rust-test

## CI & Setup ----------------------------------------------------------------

# Build Storybook static site
storybook-build:
    pnpm run storybook:build

# Run full CI pipeline (lint, format-check, typecheck, test, build, storybook-build)
ci: lint format-check typecheck test build storybook-build

# Install dependencies and git hooks
setup:
    pnpm run project:init
    pnpm lefthook install

# Remove build artifacts
clean:
    pnpm run clean

# Install system dependencies (Debian/Ubuntu)
install-deps-debian:
    sudo apt install build-essential pkg-config libgtk-3-dev libglib2.0-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev

# Create a new release
release:
    pnpm run release

# Check template sync status
template-check:
    pnpm run template:check
