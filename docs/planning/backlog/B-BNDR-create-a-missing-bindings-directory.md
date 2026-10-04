---
type: backlog
schema_version: '2'
id: B-BNDR
tags:
- clients
- generator
last_reviewed: '2026-10-04'
---

# Create a missing bindings directory instead of panicking

`gen_clients` writes the schema-known bindings before any client generator runs (`src/clients/mod.rs`, the `written_bindings` loop, which `.expect("Failed to write schema-known bindings")`). It calls `write_and_format_ts` (`crates/ontogen-core/src/utils.rs`), and with `TsFormatter::None` that goes straight to `write_if_changed`, which does not create the parent directory (only the formatter paths do, through `resolve_ts_path`). A `bindings_path` whose directory does not exist therefore panics instead of being created. The client outputs themselves are safe: `ts_client.rs`, `transport.rs` and `admin.rs` call `create_dir_all` on their parent first.

Fix by creating the parent in `write_and_format_ts` (or `write_if_changed`), and return a `CodegenError` rather than `expect` on failure.

This predates E0004 phase 3a.
