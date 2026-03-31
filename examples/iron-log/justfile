# Default recipe: show help
default:
    @just --list

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
