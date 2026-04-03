.DEFAULT_GOAL := help

.PHONY: help initialize rename \
        dev build build-debug storybook ports \
        lint lint-fix format format-check typecheck \
        frontend-dev frontend-build frontend-generate frontend-preview \
        frontend-lint frontend-test frontend-typecheck \
        frontend-format frontend-format-check \
        test test-unit \
        rust-lint rust-format rust-test \
        backend-lint backend-format-check backend-test \
        ci setup clean storybook-build \
        install-deps-debian release template-check

## Development ---------------------------------------------------------------

help: ## Show this help message
	@echo ""
	@echo "Usage: make <target>"
	@echo ""
	@echo "Initialization:"
	@echo "  initialize     Run project initialization script"
	@echo "  rename         Run project rename script"
	@echo ""
	@echo "Development:"
	@echo "  dev            Run tauri dev server (auto-assigned port)"
	@echo "  build          Build for production"
	@echo "  build-debug    Build with debug symbols"
	@echo "  storybook      Launch Storybook dev server (auto-assigned port)"
	@echo "  ports          Show auto-assigned port block for this worktree"
	@echo ""
	@echo "Frontend:"
	@echo "  frontend-dev          Run Nuxt dev server"
	@echo "  frontend-build        Build Nuxt for production"
	@echo "  frontend-generate     Generate static Nuxt site"
	@echo "  frontend-preview      Preview Nuxt production build"
	@echo "  frontend-lint         Run frontend linter"
	@echo "  frontend-test         Run frontend tests"
	@echo "  frontend-typecheck    Run frontend type checking"
	@echo "  frontend-format       Format frontend code"
	@echo "  frontend-format-check Check frontend formatting"
	@echo ""
	@echo "Linting & Formatting:"
	@echo "  lint           Run all linters (frontend + Rust)"
	@echo "  lint-fix       Auto-fix lint issues"
	@echo "  format         Format all code"
	@echo "  format-check   Check formatting without changes"
	@echo "  typecheck      Run frontend type checking"
	@echo ""
	@echo "Testing:"
	@echo "  test           Run all tests (frontend + Rust)"
	@echo "  test-unit      Run frontend unit tests only"
	@echo ""
	@echo "Rust / Backend:"
	@echo "  rust-lint      Run cargo clippy"
	@echo "  rust-format    Run cargo fmt"
	@echo "  rust-test      Run cargo test"
	@echo "  backend-lint         Alias for rust-lint"
	@echo "  backend-format-check Check backend formatting"
	@echo "  backend-test         Alias for rust-test"
	@echo ""
	@echo "CI & Setup:"
	@echo "  ci             Run full CI pipeline (lint, format-check, typecheck, test, build, storybook-build)"
	@echo "  storybook-build Build Storybook static site"
	@echo "  setup          Install dependencies and git hooks"
	@echo "  clean          Remove build artifacts"
	@echo "  install-deps-debian  Install system dependencies (Debian/Ubuntu)"
	@echo "  release        Create a new release"
	@echo "  template-check Check template sync status"
	@echo ""

## Initialization ------------------------------------------------------------

initialize: ## Run project initialization script
	scripts/initialize.sh

rename: ## Run project rename script
	scripts/rename.sh

## Development targets -------------------------------------------------------

dev: ## Run tauri dev server
	pnpm tauri dev

build: ## Build for production
	pnpm tauri build

build-debug: ## Build with debug symbols
	pnpm tauri build --debug

storybook: ## Launch Storybook dev server
	pnpm run storybook

ports: ## Show auto-assigned port block for this worktree
	@scripts/dev-port.sh --all

## Frontend ------------------------------------------------------------------

frontend-dev: ## Run Nuxt dev server
	cd src-nuxt && pnpm run dev

frontend-build: ## Build Nuxt for production
	cd src-nuxt && pnpm run build

frontend-generate: ## Generate static Nuxt site
	cd src-nuxt && pnpm run generate

frontend-preview: ## Preview Nuxt production build
	cd src-nuxt && pnpm run preview

frontend-lint: ## Run frontend linter
	pnpm run frontend:lint

frontend-test: ## Run frontend tests
	pnpm run frontend:test

frontend-typecheck: ## Run frontend type checking
	pnpm run frontend:typecheck

frontend-format: ## Format frontend code
	cd src-nuxt && pnpm run format

frontend-format-check: ## Check frontend formatting
	cd src-nuxt && pnpm run format:check

## Linting & Formatting ------------------------------------------------------

lint: ## Run all linters (frontend + Rust)
	pnpm run frontend:lint
	cd src-tauri && cargo clippy -- -D warnings

lint-fix: ## Auto-fix lint issues
	cd src-nuxt && pnpm eslint . --fix
	cd src-tauri && cargo clippy --fix --allow-dirty

format: ## Format all code
	pnpm run format

format-check: ## Check formatting without changes
	pnpm run format:check
	cd src-tauri && cargo fmt -- --check

typecheck: ## Run frontend type checking
	pnpm run frontend:typecheck

## Testing -------------------------------------------------------------------

test: ## Run all tests (frontend + Rust)
	pnpm run frontend:test
	cd src-tauri && cargo test

test-unit: ## Run frontend unit tests only
	pnpm run frontend:test

## Rust / Backend ------------------------------------------------------------

rust-lint: ## Run cargo clippy
	cd src-tauri && cargo clippy -- -D warnings

rust-format: ## Run cargo fmt
	cd src-tauri && cargo fmt

rust-test: ## Run cargo test
	cd src-tauri && cargo test

backend-lint: rust-lint ## Alias for rust-lint

backend-format-check: ## Check backend formatting
	pnpm run backend:format:check

backend-test: rust-test ## Alias for rust-test

## CI & Setup ----------------------------------------------------------------

storybook-build: ## Build Storybook static site
	pnpm run storybook:build

ci: lint format-check typecheck test build storybook-build ## Run full CI pipeline

setup: ## Install dependencies and git hooks
	pnpm run project:init
	pnpm lefthook install

clean: ## Remove build artifacts
	pnpm run clean

install-deps-debian: ## Install system dependencies (Debian/Ubuntu)
	sudo apt install build-essential pkg-config libgtk-3-dev libglib2.0-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev

release: ## Create a new release
	pnpm run release

template-check: ## Check template sync status
	pnpm run template:check
