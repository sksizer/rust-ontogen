---
type: task
schema_version: '1'
status: ready
created: 2026-05-19
last_reviewed: 2026-05-19
impact: high
complexity: medium
tags: [ontogen-ts, ts-pipeline, gen_servers]
related: [OF-015, OF-015-pr-4]
---
# OF-015 PR 5 — Wire `ontogen-ts::emit` into `gen_servers` (functional cutover)

## Goal

Replace the long-tail call site of `ts_sidecar::generate` in ontogen's `generate_transport` with `ontogen_ts::emit`. Build the `type_pool` from the user's `src/` and harvest root names from the existing `ts_bindings` partition. Emit `cargo:warning` lines + panic for each `EmitError`. Keep the side-car code in-tree but unused (deletion is PR 6's job — separate concerns for review). Iron-log must build clean. Satisfies AC-11 of [OF-015](./OF-015-productionize-typescript-generation.md).

## Today

`src/servers/mod.rs::generate_transport` currently dispatches the long-tail TS emission to `ts_sidecar::generate(...)` in `src/servers/generators/ts_sidecar.rs`. The side-car writes `src/bin/__ontogen_ts_export.rs` into the user's crate, then shells out to `cargo run --bin __ontogen_ts_export` with isolated `CARGO_TARGET_DIR`, capturing stdout as TS source. Helper functions `sidecar_lib_crate_name` and `sidecar_types_module_path` live in `src/servers/mod.rs`. The recursion guard `ONTOGEN_TS_SIDECAR_INNER` is checked at the top of `generate_transport`. `FallbackRecord` types and warning emission live in `src/transport.rs` and `src/servers/generators/ts_client.rs`. Iron-log's `examples/iron-log/src-tauri/` carries `.taurignore`, a `default-run = "iron-log"` in `Cargo.toml`, an `IRON_LOG_SKIP_SERVER_CODEGEN` env-gate in `build.rs`, and `specta-typescript` as a dependency — all workarounds for the side-car. None of these are touched by PR 5 (PR 6 cleans them up).

## Approach

Three commits inside the worktree:

1. **Add the ontogen-ts dependency** in the top-level `Cargo.toml`:
   - Add `ontogen-ts = { path = "crates/ontogen-ts", version = "0.0.0" }` (workspace-private; no crates.io publish yet per OF-015 design decision).
   - `cargo build` of the workspace succeeds.

2. **Wire `ontogen_ts::emit` into `generate_transport`** in `src/servers/mod.rs` (modify) + `src/servers/generators/ts_bindings.rs` (modify):
   - Replace the long-tail emit call (currently `ts_sidecar::generate(...)`) with `ontogen_ts::emit(roots, &type_pool, &config)`.
   - Build `type_pool` by calling `ontogen_ts::pool::scan_src_dir` on the user's crate's `src/` directory. The `src/` path is derived from the build-script invocation context (`std::env::var("CARGO_MANIFEST_DIR")` joined with `"src"`).
   - Harvest root names from the existing `ts_bindings::referenced_ts_types` and `ts_bindings::long_tail` partitions; convert each to `TypePath`.
   - Translate `EmitError`s to ontogen's existing error-rendering shape: each error renders as a `cargo:warning` line; after rendering all errors, panic with a summary so the build fails.
   - Emit `cargo:rerun-if-changed` for every source file in the type_pool's reach-set (one directive per file).
   - The `ONTOGEN_TS_SIDECAR_INNER` env guard at the top of `generate_transport` stays for now (PR 6 removes it once the side-car bin is no longer generated).
   - The side-car helpers `sidecar_lib_crate_name` / `sidecar_types_module_path` stay (dead code; PR 6 removes them).
   - Iron-log build verification: `cd examples/iron-log/src-tauri && cargo build` succeeds; the generated `examples/iron-log/src-nuxt/app/generated/types.ts` content is equivalent or better than what the side-car produced (no missing types, no extra noise).

3. **Update or add tests** in `tests/` (modify existing integration tests if any cover the long-tail path; the existing side-car-based behavior tests may need to be re-pointed at the new ontogen-ts call path).

Each commit builds clean and `just full-check` passes.

## Files to touch

- `Cargo.toml` (modify) — add `ontogen-ts` as a workspace-internal dependency of ontogen.
- `src/servers/mod.rs` (modify) — replace `ts_sidecar::generate` call with `ontogen_ts::emit`; preserve the `ONTOGEN_TS_SIDECAR_INNER` guard and side-car helper fns for now (PR 6 deletes).
- `src/servers/generators/ts_bindings.rs` (modify) — surface the long-tail handoff into ontogen-ts; root-name harvesting may live here or in `mod.rs`.
- `src/servers/generators/ts_sidecar.rs` (UNTOUCHED) — kept in-tree but no longer called.
- `examples/iron-log/src-nuxt/app/generated/types.ts` — regenerated content (the validation artifact; may need committing if the build emits it deterministically).
- `tests/` (modify) — re-point any tests that exercised the side-car code path.

## Acceptance criteria

These are AC-11 from OF-015 — restated here for per-PR scope:

- [ ] AC-11.1: `src/servers/mod.rs::generate_transport` calls `ontogen_ts::emit` instead of `ts_sidecar::generate` for the long-tail emission slice.
- [ ] AC-11.2: `type_pool` constructed by walking `.rs` files under the user's `src/` (path derived from `CARGO_MANIFEST_DIR` joined with `"src"`).
- [ ] AC-11.3: Root names harvested from existing `ts_bindings::referenced_ts_types` / `ts_bindings::long_tail` partitions; converted to `TypePath`.
- [ ] AC-11.4: `EmitError`s surfaced as `cargo:warning` lines + panic, same shape as the existing `CodegenError` handling.
- [ ] AC-11.5: `cargo:rerun-if-changed` emitted for every source file referenced during emission (one per file in the type_pool's reach-set).
- [ ] AC-11.6: Iron-log builds clean: `cargo build` in `examples/iron-log/src-tauri/` succeeds.
- [ ] AC-11.7: `examples/iron-log/src-nuxt/app/generated/types.ts` content is equivalent or better than the side-car's output (no missing types, no extra noise; spot-check by diff against the prior generated file).
- [ ] AC-11.8: Side-car infrastructure (`ts_sidecar.rs`, `sidecar_lib_crate_name`, `sidecar_types_module_path`, `ONTOGEN_TS_SIDECAR_INNER` guard) is still present in-tree but no longer reached by `generate_transport`.

## Out of scope

- **Side-car deletion** — PR 6.
- **FallbackRecord removal** — PR 6.
- **Iron-log workaround cleanup** (`.taurignore`, `default-run`, `IRON_LOG_SKIP_SERVER_CODEGEN`, `specta-typescript`) — PR 6.
- **Pumice validation** — PR 7.

## Dependencies

- [[OF-015-pr-4-emit-and-proc-macros]] must land first (provides the top-level `emit` function this PR calls).
