# Ontogen -- Task Backlog

## Active Tasks

### 1. MCP-to-CLI Generator
**Status:** Proposed
**Doc:** [cli-generator.md](./cli-generator.md)
**Summary:** Add a CLI client generator that turns MCP tools into clap subcommands. Generates a standalone binary crate. 4 phases: CRUD subcommands, custom actions, DX polish, generic library extraction.

### 2. Rustdoc Documentation
**Status:** Not started
**Summary:** Add comprehensive `rustdoc` documentation to the crate:
- Crate-level doc (`lib.rs`) with overview, architecture diagram, and quick-start example
- Module-level docs for each generator (`schema`, `persistence`, `store`, `api`, `servers`, `clients`)
- Public API docs -- all public types, functions, and enums with doc comments and usage examples
- Builder/config docs showing how to wire the pipeline in a `build.rs`
- `#![warn(missing_docs)]` enforced once coverage is sufficient
- `cargo doc --open` should produce a useful, navigable reference

### 3. Phase 6 -- Typed Channels + Instrumentation
**Status:** Not started
**Doc:** See [proposal.md](./proposal.md) Phase 6
**Summary:** Per-entity broadcast channels for change events, tracing instrumentation on generated store methods.

## Completed

- Phase 1: Schema parsing + SeaORM + markdown I/O + DTO generation
- Phase 2: Store layer generation with lifecycle hooks
- Phase 3: API layer generation with scan+merge
- Phase 4: Server transport generation (HTTP, IPC, MCP)
- Phase 5: Client generation (TypeScript, admin registry)
- Phase 5b: All 13 entities migrated to codegen pipeline
- Consolidated from two prior codegen crates into unified ontogen pipeline
