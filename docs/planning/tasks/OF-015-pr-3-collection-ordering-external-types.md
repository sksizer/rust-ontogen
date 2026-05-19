---
type: task
schema_version: '1'
status: closed/done
created: 2026-05-19
last_reviewed: 2026-05-19
impact: high
complexity: medium
tags: [ontogen-ts, ts-pipeline]
related: [OF-015, OF-015-pr-2]
completion_note: "Shipped in #62 (merge 4886d2e, 2026-05-19). Commit 1fda7be — type collection + use-resolution + external-types + topological ordering covering AC-5/6/7."
---
# OF-015 PR 3 — Type collection, topological ordering, use-resolution, external-types

## Goal

Build the type-pool walker that scans a user's `src/` for module-level structs/enums/type-aliases, keys them by canonical `TypePath`, and resolves cross-file references through per-file `use` declarations. Build the external-types table with shipped defaults + user override merge. Produce deterministic output via BTreeMap/BTreeSet-by-construction and Kahn's-algorithm topological ordering. Satisfies AC-5, AC-6, AC-7 of [OF-015](./OF-015-productionize-typescript-generation.md).

## Today

After PR 2 lands, `crates/ontogen-ts/src/emit.rs::emit_type` falls through to a placeholder when it encounters a single-segment ident that isn't a primitive/container/smart-pointer — there's no pool to look the ident up in. There's no walker, no use-resolution, no external-types table. `EmitConfig.external_types: HashMap<String, &'static str>` is declared but never read. `crates/ontogen-ts/src/types.rs::TypePath` is a newtype with no canonical-comparison helpers. The `emit` function signature accepts `type_pool: &BTreeMap<TypePath, syn::Item>` but the function body is still `todo!()` (PR 4's job to compose).

## Approach

Four commits inside the worktree:

1. **Pool walker** in `crates/ontogen-ts/src/pool.rs` (new).
   - `pub fn scan_src_dir(src_dir: &Path) -> Result<BTreeMap<TypePath, syn::Item>, ScanError>`. Recursive walk of `src/`, parse each `.rs` via `syn::parse_file`, collect every module-level `ItemStruct`, `ItemEnum`, `ItemType` regardless of visibility.
   - Key each item by canonical `TypePath`: module path derived from source-file relative path under `src/` (`foo/bar.rs` → `["crate", "foo", "bar"]`, `mod.rs` paths collapsed), plus the item ident as the terminal segment.
   - Skip `examples/`, `benches/`, `tests/`, `build.rs` — `src/` only.
   - Include `cfg`-gated types (we don't cfg-eval; `syn::parse_file` gives raw AST).
   - Pool keys use canonical paths from where the item is *defined* (not re-export paths).
   - Unit tests: synthetic on-disk fixture (use `tempfile`) with a few `.rs` files, assert the pool keys.

2. **Per-file use-resolution** in `crates/ontogen-ts/src/resolve.rs` (new).
   - `pub(crate) struct FileImports { simple: BTreeMap<String, TypePath>, /* one-segment ident → canonical path */ }`.
   - `pub(crate) fn parse_imports(file: &syn::File) -> FileImports`. Walk `Item::Use` declarations: handle `use foo::Bar`, `use foo::Bar as Baz`, `use foo::{Bar, Baz}`, `use foo::*` (glob — record glob set or reject at resolution time).
   - `pub(crate) fn canonicalize(path: &syn::Path, imports: &FileImports, defined_in: &TypePath) -> Result<TypePath, EmitError>`. One-segment refs → consult imports. Multi-segment refs → take as-qualified, strip `crate::` for local-pool lookup.
   - Glob imports referenced via a one-segment ident raise `UnresolvedReference { name, referenced_by, hint: "glob import; qualify or use explicit import" }`.
   - Unit tests cover each `use` form.

3. **External-types table** in `crates/ontogen-ts/src/external.rs` (new).
   - `pub(crate) const DEFAULT_EXTERNAL_TYPES: &[(&str, &str)]` — the shipped set: `chrono::DateTime` → `"string"`, `chrono::NaiveDate` → `"string"`, `chrono::NaiveDateTime` → `"string"`, `chrono::NaiveTime` → `"string"`, `time::OffsetDateTime` → `"string"`, `time::PrimitiveDateTime` → `"string"`, `time::Date` → `"string"`, `time::Time` → `"string"`, `uuid::Uuid` → `"string"`, `url::Url` → `"string"`, `std::path::PathBuf` → `"string"`, `std::net::IpAddr` → `"string"`, `std::net::Ipv4Addr` → `"string"`, `std::net::Ipv6Addr` → `"string"`, `serde_json::Value` → `"unknown"`.
   - `pub(crate) fn resolve(canonical: &TypePath, user: &HashMap<String, &'static str>) -> Option<&str>` — user-provided overrides win on conflict. Match by canonical path with generic args stripped.
   - Unit tests cover default-only, override-only, override-wins-on-conflict.

4. **Topological ordering** in `crates/ontogen-ts/src/order.rs` (new).
   - `pub(crate) fn dependency_graph(pool: &BTreeMap<TypePath, syn::Item>) -> BTreeMap<TypePath, BTreeSet<TypePath>>` — for each item, walk its fields/variants and collect canonicalized refs (using `resolve::canonicalize`).
   - `pub(crate) fn topo_order(graph: &BTreeMap<TypePath, BTreeSet<TypePath>>, reachable: &BTreeSet<TypePath>) -> Vec<TypePath>` — Kahn's algorithm restricted to `reachable`. BTreeSet's natural ordering provides the alphabetical tiebreaker within each topo level for free.
   - Cycle handling: cycle members are co-emitted as an alphabetical group at the cycle's topo level (their refs to each other become forward references, which TS allows).
   - Unit tests: synthetic graphs with no cycle, with one cycle, with two disconnected components.

Each commit builds clean and `just full-check` passes.

## Files to touch

- `crates/ontogen-ts/src/pool.rs` (new) — pool walker.
- `crates/ontogen-ts/src/resolve.rs` (new) — use-resolution + canonicalization.
- `crates/ontogen-ts/src/external.rs` (new) — external-types defaults + override merge.
- `crates/ontogen-ts/src/order.rs` (new) — dependency graph + Kahn's algorithm.
- `crates/ontogen-ts/src/lib.rs` (modify) — register the new modules.
- `crates/ontogen-ts/src/types.rs` (modify) — `TypePath` helpers if needed (e.g. `strip_generic_args`).
- `crates/ontogen-ts/Cargo.toml` (modify) — add `walkdir` (or std `read_dir` recursion); `tempfile` to `[dev-dependencies]` for fixture tests.

## Acceptance criteria

These are AC-5, AC-6, AC-7 from OF-015 — restated here for per-PR scope:

- [ ] AC-5.1: Default external-types set ships exactly per OF-015 design pass (`chrono`, `time`, `uuid`, `url`, `std::path::PathBuf`, `std::net` family → `"string"`; `serde_json::Value` → `"unknown"`).
- [ ] AC-5.2: User-provided overrides via `EmitConfig.external_types` merge on top (user wins on conflict).
- [ ] AC-5.3: Walker matches canonical paths (not terminal idents); generic args stripped at match time so `DateTime<Utc>`, `DateTime<Local>` both hit `chrono::DateTime`.
- [ ] AC-6.1: `type_pool` keyed by canonical `TypePath`, value `syn::Item` (struct/enum/alias).
- [ ] AC-6.2: Pool walker collects every module-level `ItemStruct` / `ItemEnum` / `ItemType` under user's `src/`, regardless of visibility.
- [ ] AC-6.3: Roots reach transitive closure via field-type walking.
- [ ] AC-6.4: Self-referential types (cycles) emit cleanly (TS forward references resolve).
- [ ] AC-6.5: Output ordering is deterministic: topological by reference, alphabetical-by-canonical-path within each topo level.
- [ ] AC-6.6: `BTreeMap` / `BTreeSet` used throughout — HashMap iteration order never leaks into output.
- [ ] AC-7.1: Per-file `use` declarations parsed during `type_pool` construction.
- [ ] AC-7.2: One-segment refs resolve through the file's imports table to canonical paths.
- [ ] AC-7.3: Multi-segment refs taken as-qualified; `crate::` prefix stripped for local-type pool lookup.
- [ ] AC-7.4: Glob imports rejected at resolution time with `UnresolvedReference` carrying a hint.
- [ ] AC-7.5: Re-exports: canonical pool key is path-where-defined; re-export resolution happens before pool lookup.

## Out of scope

- **Top-level `emit` composition** — PR 4.
- **`#[ontogen::ts_opaque]` / `#[ontogen::ts_name]`** — PR 4.
- **`gen_servers` wiring** — PR 5.

## Dependencies

- [[OF-015-pr-2-serde-rename-engine]] should land first (provides the rename-aware emission this PR's collection step ultimately feeds). PR 3 doesn't strictly depend on PR 2's renames at compile time, but stacking sequentially keeps the workflow linear.
