# Architecture

This document describes the architecture, conventions, and project structure of the Tauri + Nuxt template.

## Project Structure

```
template-tauri-nuxt/
├── .github/              # CI/CD workflows and Dependabot config
│   └── workflows/        # ci, build-check, release
├── scripts/              # Helper scripts, CI tooling, and dev utilities
│   ├── backend-*         # Rust CI helpers (lint, format-check, test)
│   ├── dev-port.sh       # Auto-port assignment for parallel worktrees
│   ├── initialize.sh     # Interactive project initialization
│   ├── rename.sh         # Non-interactive rename (env-driven)
│   ├── sync-template-check  # Template drift detection
│   └── tauri-wrapper.mjs # Wraps tauri CLI to inject auto-selected ports
├── docs/                 # Project documentation
├── src-nuxt/             # Frontend — Nuxt.js application
│   ├── .storybook/       # Storybook configuration
│   ├── app/              # Vue components, assets, and pages
│   │   └── components/   # Extracted components + co-located stories
│   ├── server/           # Nuxt server code
│   ├── tests/            # Frontend tests (Vitest)
│   └── public/           # Static assets
├── src-tauri/            # Backend — Rust/Tauri application
│   ├── src/              # Rust source code
│   ├── capabilities/     # Tauri capability definitions
│   └── icons/            # App icons (all platforms)
├── Makefile              # Make-based command interface
├── justfile              # Just-based command interface (mirrors Makefile)
├── mise.toml             # Tool version management and port env vars
├── lefthook.yml          # Git hook configuration
├── commitlint.config.ts  # Conventional commit enforcement
└── package.json          # Root package — orchestrates cross-project scripts
```

## Command Interface

### Makefile and justfile

Both `Makefile` and `justfile` are canonical command interfaces. They expose **identical targets** so developers can use whichever runner they prefer:

- **make** — available everywhere, no extra install needed
- **just** — installable via `cargo install just` or `mise use just`

Pick one and delete the other, or keep both — they will always mirror each other.

### Command Mirroring Rule

**Every `package.json` script must have a corresponding Makefile target and justfile recipe.** This ensures a single, consistent entry point for all operations regardless of which tool a developer uses.

When adding a new script to `package.json`:

1. Add a Makefile target with a `##` comment for the help system
2. Add a justfile recipe with a `#` comment for `just --list`
3. Use the same target name in both

When a Makefile target or justfile recipe calls a `package.json` script, it should use `pnpm run <script>` (not `cd` + `pnpm` unless the script must run in a subdirectory).

### Naming Conventions

| Context | Convention | Example |
|---------|-----------|---------|
| Makefile targets | kebab-case | `build-debug`, `format-check` |
| justfile recipes | kebab-case | `build-debug`, `format-check` |
| package.json scripts | kebab-case with `:` namespacing | `frontend:lint`, `clean:frontend` |
| Rust crate names | snake_case | `tauri_nuxt` |
| Shell scripts | kebab-case | `dev-port.sh` |
| Environment variables | UPPER_SNAKE_CASE | `TAURI_DEV_PORT` |

## Auto-Port System

The auto-port system gives each worktree (or checkout directory) a **deterministic block of 4 consecutive ports** derived from the absolute path of the project. This allows multiple worktrees of the same project to run simultaneously without port conflicts.

| Service | Env Variable | Offset |
|---------|-------------|--------|
| Nuxt dev server | `TAURI_DEV_PORT` | base + 0 |
| Storybook | `STORYBOOK_PORT` | base + 1 |
| MCP server | `MCP_PORT` | base + 2 |
| HTTP server | `HTTP_PORT` | base + 3 |

**How it works:**

1. `scripts/dev-port.sh` hashes the current working directory (via `cksum`) to pick a port block in the 3000–9996 range, aligned to 4-port boundaries.
2. If those ports are occupied, it scans forward (then wraps around) for a free block.
3. `scripts/tauri-wrapper.mjs` wraps the `tauri` CLI: for `dev` commands, it resolves port assignments and injects the correct `devUrl` into Tauri's config via `--config`.
4. `mise.toml` sets port env vars automatically when entering the directory.

```bash
# View your assigned ports
make ports              # or: just ports / scripts/dev-port.sh --all

# Override manually (all 4 ports shift together)
TAURI_DEV_PORT=5000 make dev
```

## Tool Versions (mise)

Tool versions are pinned in `mise.toml`:

- **Node**: LTS
- **pnpm**: latest
- **Rust**: 1.93.0 (auto-installs `tauri-cli` v2.10.0 via postinstall)

When entering the project directory with mise active, port env vars are set automatically and tool versions are enforced.

## Initialization

When using this template for a new project:

```bash
# Interactive — prompts for project name and bundle ID:
make initialize         # or: just initialize

# Non-interactive — set env vars:
PROJECT_NAME=my_app BUNDLE_ID=com.example.myapp make rename
```

The initialization scripts update all references to the template defaults (`tauri_nuxt`, `com.sksizer.example`) across:

- `package.json` (root) — `name` field
- `src-tauri/tauri.conf.json` — `productName`, `identifier`, window `title`
- `src-tauri/Cargo.toml` — `name`, lib `name`

## CI/CD

| Workflow | Trigger | What it does |
|----------|---------|-------------|
| `ci.yml` | Push/PR to main | Frontend lint + typecheck + test; Rust format + clippy + test |
| `build-check.yml` | Push/PR to main | Full Tauri build verification |
| `release.yml` | Tag `v*` | Multi-platform build (macOS aarch64/x86_64, Ubuntu, Windows) via `tauri-apps/tauri-action`; creates draft GitHub release |

## Git Hooks (Lefthook)

**pre-commit** (parallel):
- `eslint` — lint JS/TS/Vue files
- `oxlint` — fast lint pass
- `prettier` — format check
- `clippy` — Rust lint (warnings = errors)
- `rustfmt` — Rust format check
- `typecheck` — Nuxt type checking

**commit-msg**: `commitlint` enforces Conventional Commits.

## Linting & Formatting

| Layer | Frontend | Backend |
|-------|----------|---------|
| Fast lint | oxlint | — |
| Full lint | ESLint + @nuxt/eslint | cargo clippy (-D warnings) |
| Format | Prettier (no semi, single quotes, 100 width) | cargo fmt |

## Testing

| Layer | Tool | Command |
|-------|------|---------|
| Frontend | Vitest + @nuxt/test-utils | `make test-unit` |
| Backend | cargo test | `make rust-test` |
| All | — | `make test` |

## Version Management

Single version source in root `package.json`, synced to `src-tauri/Cargo.toml` via release-it + `@release-it/bumper`.

```bash
pnpm run release    # bumps version, tags as v{version}, syncs Cargo.toml
```
