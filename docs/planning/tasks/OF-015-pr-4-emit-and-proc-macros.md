---
type: task
schema_version: '1'
status: ready
created: 2026-05-19
last_reviewed: 2026-05-19
impact: high
complexity: medium
tags: [ontogen-ts, ts-pipeline]
related: [OF-015, OF-015-pr-3]
---
# OF-015 PR 4 — Top-level `emit()` composition + `#[ontogen::ts_opaque]` / `#[ontogen::ts_name]`

## Goal

Implement the top-level `ontogen_ts::emit()` function that composes type collection + validation + per-type emission + error aggregation. Add the `#[ontogen::ts_opaque(target = "...")]` and `#[ontogen::ts_name = "..."]` proc-macro attrs in `crates/ontogen-macros/` (no-op at Rust compile time; ontogen-ts reads them via AST inspection). Hard-error on unsupported shapes (no `FallbackRecord`) — collect all errors into `Vec<EmitError>` before failing so the user sees the full punch-list in one build. Satisfies AC-8, AC-9, AC-10 of [OF-015](./OF-015-productionize-typescript-generation.md).

## Today

After PR 3 lands, `crates/ontogen-ts/src/pool.rs::scan_src_dir`, `resolve.rs::canonicalize`, `external.rs::resolve`, and `order.rs::topo_order` are individually working with unit tests. The top-level `crates/ontogen-ts/src/lib.rs::emit` is still `todo!()`. The proc-macro crate `crates/ontogen-macros/src/lib.rs` exposes the `#[ontogen::api]` attribute family but has no `ts_opaque` / `ts_name` attributes. There is no end-to-end fixture test exercising the full pipeline on a synthetic crate.

## Approach

Three commits inside the worktree:

1. **Proc-macro attrs** in `crates/ontogen-macros/src/lib.rs` (modify).
   - Add `#[proc_macro_attribute] pub fn ts_opaque(args: TokenStream, item: TokenStream) -> TokenStream`. Implementation: parse args (expect `target = "..."`), pass `item` through unchanged. Validation happens here so a malformed attr fails at Rust compile time.
   - Add `#[proc_macro_attribute] pub fn ts_name(args: TokenStream, item: TokenStream) -> TokenStream`. Implementation: parse args (expect a string literal), pass `item` through unchanged.
   - Both attrs are functionally no-ops at Rust compile time; their value is in the attribute's presence on the AST, which ontogen-ts reads.
   - Unit tests via `trybuild` for compile-success cases (well-formed args) and compile-fail cases (malformed args).
   - Tests in `crates/ontogen-macros/tests/`.

2. **Attribute extraction** in `crates/ontogen-ts/src/attr.rs` (modify, building on PR 2's serde-attr extraction).
   - `pub(crate) struct OntogenAttrs { ts_opaque: Option<String>, ts_name: Option<String> }`.
   - `pub(crate) fn extract_ontogen_attrs(attrs: &[syn::Attribute]) -> Result<OntogenAttrs, EmitError>`. Parse `#[ontogen::ts_opaque(target = "...")]` and `#[ontogen::ts_name = "..."]`. Reject malformed forms.
   - Unit tests cover well-formed + malformed + missing-attr cases.

3. **Top-level `emit()` composition** in `crates/ontogen-ts/src/lib.rs` (modify).
   - Implementation:
     ```
     pub fn emit(
         roots: &[TypePath],
         type_pool: &BTreeMap<TypePath, syn::Item>,
         config: &EmitConfig,
     ) -> Result<String, Vec<EmitError>> {
         // 1. Collect transitive closure of types reachable from `roots`.
         // 2. Resolve canonical paths; build dependency graph (order::dependency_graph).
         // 3. Detect name collisions on the reachable set (post-`ts_name` resolution). Hard-error.
         // 4. Topological order the reachable set (order::topo_order with cycle co-emission).
         // 5. For each type in order:
         //    a. Extract serde + ontogen attrs.
         //    b. If `ts_opaque`: emit a terminal alias `export type Name = <target>;` and skip recursion.
         //    c. Otherwise: dispatch to emit_struct / emit_enum / emit_type_alias, applying renames.
         // 6. Concatenate outputs with one blank line between types. Return.
         // All errors collected into Vec<EmitError>; never first-error fail-fast.
     }
     ```
   - Name-collision check: build `BTreeMap<TsName, Vec<TypePath>>` from reachable types (applying `ts_name` overrides). Any entry with `len > 1` becomes `EmitError::NameCollision { name, paths }`.
   - Build-fail UX is the caller's concern (ontogen renders each `EmitError` as `cargo:warning` then panics — see PR 5); the `emit` function itself just returns the `Vec<EmitError>`.
   - End-to-end test in `crates/ontogen-ts/tests/end_to_end.rs` (new):
     - Synthetic on-disk crate via `tempfile`: 1 file `lib.rs` with several structs/enums (primitives, containers, references, renames, external types, a `#[ontogen::ts_opaque]` use, a `#[ontogen::ts_name]` use, at least one unsupported shape).
     - Call `pool::scan_src_dir` then `emit`.
     - Assert: returned TS contains expected type defs; returned `Vec<EmitError>` for the negative case has exactly the expected variants.

Each commit builds clean and `just full-check` passes.

## Files to touch

- `crates/ontogen-macros/src/lib.rs` (modify) — add `ts_opaque` + `ts_name` attribute macros.
- `crates/ontogen-macros/tests/ts_attrs.rs` (new) — `trybuild` compile-pass/fail tests for the new attrs.
- `crates/ontogen-macros/Cargo.toml` (modify if needed) — `trybuild` to `[dev-dependencies]`.
- `crates/ontogen-ts/src/attr.rs` (modify) — extract ontogen attrs alongside serde attrs.
- `crates/ontogen-ts/src/lib.rs` (modify) — implement `emit` composition.
- `crates/ontogen-ts/src/emit.rs` (modify) — wire ontogen-attrs handling (ts_opaque short-circuit, ts_name override).
- `crates/ontogen-ts/tests/end_to_end.rs` (new) — synthetic-crate fixture test.

## Acceptance criteria

These are AC-8, AC-9, AC-10 from OF-015 — restated here for per-PR scope:

- [ ] AC-8.1: All errors collected into `Vec<EmitError>` before failing (no first-error fail-fast).
- [ ] AC-8.2: Hard error on unsupported shapes — no `FallbackRecord` placeholder, no warning-and-continue.
- [ ] AC-8.3: End-to-end test on a synthetic crate covering primitives, containers, references, serde renames, external types, and at least one negative case (unsupported shape) in one fixture, asserts the emitted TS + the returned `Vec<EmitError>`.
- [ ] AC-9.1: `#[ontogen::ts_opaque(target = "MyTsAlias")]` parses; walker treats the type as terminal; emitter outputs `export type Name = MyTsAlias;` (or similar terminal form) at the type's location without recursing into its fields.
- [ ] AC-9.2: `#[ontogen::ts_name = "FooStats"]` parses; emitter uses the override as the type's TS name; JSON wire is unaffected (serde never sees the attr).
- [ ] AC-9.3: Both attrs are no-ops at Rust compile time (no token generation; presence-on-AST only). `trybuild` compile-pass tests confirm.
- [ ] AC-9.4: Malformed args (`#[ontogen::ts_opaque(typo = "...")]`, `#[ontogen::ts_name = 42]`, etc.) fail at Rust compile time with clear errors via `trybuild` compile-fail.
- [ ] AC-10.1: Two reachable types emitting to the same TS name (post-`ts_name` resolution) fail with `EmitError::NameCollision { name, paths }` listing all colliding canonical paths.
- [ ] AC-10.2: Error human-renders with fix-path hints in priority order: `#[ontogen::ts_name = "..."]`, `#[ontogen::ts_opaque(target = "...")]`, Rust-side rename.
- [ ] AC-10.3: Unreachable collisions (two same-named types in the pool that neither reach from a root) do NOT error.

## Out of scope

- **`gen_servers` wiring** — PR 5.
- **Side-car deletion** — PR 6.
- **Pumice validation** — PR 7.
- **Docs** — PR 8.

## Dependencies

- [[OF-015-pr-3-collection-ordering-external-types]] must land first (provides the `pool`, `resolve`, `external`, `order` modules this PR composes).
