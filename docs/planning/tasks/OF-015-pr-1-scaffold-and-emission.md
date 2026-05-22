---
type: task
schema_version: '2'
status: closed/done
created: 2026-05-15
last_reviewed: 2026-05-19
completion_note: "Shipped in #55 (merge 13c2fcd, 2026-05-15). Commits 9ddfbc7 + 0633746 + 26d6c81 + 1e4abe2 — scaffold + per-type emission for AC-1/2/3."
impact: high
complexity: medium
tags: [ontogen-ts, ts-pipeline]
related: [OF-015]
---
# OF-015 PR 1 — Scaffold `crates/ontogen-ts/` + per-type emission

## Resolution

Shipped in PR [#55](https://github.com/sksizer/rust-ontogen/pull/55), merged 2026-05-15 as `13c2fcd`. Four commits on `feat/OF-015-pr-1-scaffold-and-emission`:

- `9ddfbc7` — scaffold crate with public API skeleton (`TypePath`, `EmitConfig`, `EmitError`, `BigIntBehavior`, `RenameAll`, `emit` as `todo!()` stub).
- `0633746` — per-type emission for primitives, containers, smart-pointer peel.
- `26d6c81` — emission for named structs and enums.
- `1e4abe2` — convert per-type tests to on-disk fixture pairs (`tests/fixtures/*.rs` + `*.ts`).

The body below is preserved as historical record of the task as scoped. The "Today" section described state-before-PR-55 (no `crates/ontogen-ts/` directory) — that state no longer holds; see `crates/ontogen-ts/src/` on `main` for the shipped surface.

## Goal

Stand up the `ontogen-ts` crate as a workspace member and define its public API surface (`TypePath`, `EmitConfig`, `EmitError`, the `emit` signature). Implement per-type emission for the phase-1 supported subset on hardcoded `syn::Item` fixtures so subsequent PRs (rename engine, type collection, wiring) have a foundation to build on. Satisfies AC-1, AC-2, AC-3 of [OF-015](./OF-015-productionize-typescript-generation.md).

## Today

The TypeScript bindings pipeline is currently a `specta` side-car (`src/servers/generators/ts_sidecar.rs`) that ontogen launches from `build.rs` context: it writes a binary into the user's crate (`src/bin/__ontogen_ts_export.rs`), compiles it through `cargo` with an isolated `CARGO_TARGET_DIR`, runs it, and captures stdout. OF-014's design spike documented the structural problems (recursion guard via `ONTOGEN_TS_SIDECAR_INNER`, target-dir lock contention, doubled cold-build time, source-tree pollution, watcher loops). The schema-known emitter in `src/servers/generators/ts_bindings.rs` (head: entities + generated DTOs) is unchanged by this PR; the long-tail emission is what ontogen-ts will eventually replace. There is no `crates/ontogen-ts/` directory today; the workspace's `Cargo.toml` lists `ontogen-core` and `ontogen-macros` as members.

## Approach

Three commits inside the worktree, in order:

1. **Crate scaffold.** Add `crates/ontogen-ts/Cargo.toml` and `crates/ontogen-ts/src/lib.rs` registering the crate in the workspace. Wire it into the root `Cargo.toml`'s `workspace.members`. Add `syn` (with `full` + `extra-traits`) and `quote` as deps. Define the public types as skeletons:
   - `TypePath(Vec<String>)` newtype with `new(path: Vec<String>) -> Result<Self, ...>` rejecting empty paths
   - `EmitConfig` struct with phase-1 fields (`external_types`, `bigint_behavior`, `case_default`, `strict_unsupported`)
   - `EmitError` enum with four variants (`UnsupportedShape`, `UnsupportedSerdeAttr`, `UnresolvedReference`, `NameCollision`)
   - `pub fn emit(...) -> Result<String, Vec<EmitError>>` as a `todo!()` stub
   - `cargo build -p ontogen-ts` succeeds.

2. **Primitive + container + smart-pointer emission.** Add `pub(crate) fn emit_type(ty: &syn::Type, config: &EmitConfig) -> Result<String, EmitError>` that classifies a `syn::Type` and produces its TS rendering. Handle in this order:
   - Smart-pointer peel: `Box<T>`, `Rc<T>`, `Arc<T>`, `Cow<'_, T>`, `Pin<P>` → recurse on inner
   - Runtime-coordination primitives: `RefCell<T>`, `Mutex<T>`, `RwLock<T>` → `UnsupportedShape`
   - Container generics: `Option<T>` → `T | null`; `Vec<T>` → `T[]`; `HashMap<K, V>` / `BTreeMap<K, V>` → `Record<K, V>` (K must be `String` or id-like primitive per validation)
   - Reference types: `&T`, `&[T]` → unwrap to owned form (`&str` → `String` → `string`; `&[T]` → `Vec<T>` → `T[]`)
   - Primitives: all integer types and `f32`/`f64` → `number`; `bool` → `boolean`; `String`/`&str` → `string`
   - Anything else (single-segment ident not in the above) → defer with a placeholder pointing at the type's terminal ident (full pool/external-types lookup is PR 3 work; for now, return a fall-through that subsequent PRs build on)
   - Unit tests covering each branch with hardcoded `syn::Type` fixtures parsed inline via `syn::parse_str`.

3. **Struct + enum emission.** Add `pub(crate) fn emit_struct(item: &syn::ItemStruct, config: &EmitConfig) -> Result<String, EmitError>` and `emit_enum(item: &syn::ItemEnum, config: &EmitConfig) -> Result<String, EmitError>`.
   - **Structs**: produce `export type Name = { field1: type1, field2: type2, ... };` over named fields. Tuple structs (`struct Foo(u32)`) and unit structs (`struct Bar;`) return `UnsupportedShape` with a clear reason.
   - **Enums**: produce `export type Name = 'Variant1' | 'Variant2' | ...;` for C-style enums (variants without payloads). Variants with payloads under default (untagged-variant-name) representation get phase-1's representation (likely `{ type: 'V1', data: ... }` or similar — see Open questions below for spec). Enums where representation is contested or `#[serde(tag)]` is present return `UnsupportedSerdeAttr` for now (PR 2 will reject; full tagged-enum support is phase-2 work).
   - Unit tests parse fixture structs and enums via `syn::parse_quote!` and assert the emitted TS strings.

Each commit is self-contained: builds clean, `just full-check` passes, tests pass.

## Approach

Three commits inside the worktree, in order:

1. **Crate scaffold.** Add `crates/ontogen-ts/Cargo.toml` and `crates/ontogen-ts/src/lib.rs` registering the crate in the workspace. Wire it into the root `Cargo.toml`'s `workspace.members`. Add `syn` (with `full` + `extra-traits`) and `quote` as deps. Define the public types as skeletons:
   - `TypePath(Vec<String>)` newtype with `new(path: Vec<String>) -> Result<Self, ...>` rejecting empty paths
   - `EmitConfig` struct with phase-1 fields (`external_types`, `bigint_behavior`, `case_default`, `strict_unsupported`)
   - `EmitError` enum with four variants (`UnsupportedShape`, `UnsupportedSerdeAttr`, `UnresolvedReference`, `NameCollision`)
   - `pub fn emit(...) -> Result<String, Vec<EmitError>>` as a `todo!()` stub
   - `cargo build -p ontogen-ts` succeeds.

2. **Primitive + container + smart-pointer emission.** Add `pub(crate) fn emit_type(ty: &syn::Type, config: &EmitConfig) -> Result<String, EmitError>` that classifies a `syn::Type` and produces its TS rendering. Handle in this order:
   - Smart-pointer peel: `Box<T>`, `Rc<T>`, `Arc<T>`, `Cow<'_, T>`, `Pin<P>` → recurse on inner
   - Runtime-coordination primitives: `RefCell<T>`, `Mutex<T>`, `RwLock<T>` → `UnsupportedShape`
   - Container generics: `Option<T>` → `T | null`; `Vec<T>` → `T[]`; `HashMap<K, V>` / `BTreeMap<K, V>` → `Record<K, V>` (K must be `String` or id-like primitive per validation)
   - Reference types: `&T`, `&[T]` → unwrap to owned form (`&str` → `String` → `string`; `&[T]` → `Vec<T>` → `T[]`)
   - Primitives: all integer types and `f32`/`f64` → `number`; `bool` → `boolean`; `String`/`&str` → `string`
   - Anything else (single-segment ident not in the above) → defer with a placeholder pointing at the type's terminal ident (full pool/external-types lookup is PR 3 work; for now, return a fall-through that subsequent PRs build on)
   - Unit tests covering each branch with hardcoded `syn::Type` fixtures parsed inline via `syn::parse_str`.

3. **Struct + enum emission.** Add `pub(crate) fn emit_struct(item: &syn::ItemStruct, config: &EmitConfig) -> Result<String, EmitError>` and `emit_enum(item: &syn::ItemEnum, config: &EmitConfig) -> Result<String, EmitError>`.
   - **Structs**: produce `export type Name = { field1: type1, field2: type2, ... };` over named fields. Tuple structs (`struct Foo(u32)`) and unit structs (`struct Bar;`) return `UnsupportedShape` with a clear reason.
   - **Enums**: produce `export type Name = 'Variant1' | 'Variant2' | ...;` for C-style enums (variants without payloads). Variants with payloads under default (untagged-variant-name) representation get phase-1's representation (likely `{ type: 'V1', data: ... }` or similar — see Open questions below for spec). Enums where representation is contested or `#[serde(tag)]` is present return `UnsupportedSerdeAttr` for now (PR 2 will reject; full tagged-enum support is phase-2 work).
   - Unit tests parse fixture structs and enums via `syn::parse_quote!` and assert the emitted TS strings.

Each commit is self-contained: builds clean, `just full-check` passes, tests pass.

## Files to touch

- **`crates/ontogen-ts/Cargo.toml`** (new) — package metadata, `syn`/`quote` deps.
- **`crates/ontogen-ts/src/lib.rs`** (new) — public API types + module wiring.
- **`crates/ontogen-ts/src/emit.rs`** (new) — `emit_type`, `emit_struct`, `emit_enum`. Unit tests inline (`#[cfg(test)] mod tests`).
- **`crates/ontogen-ts/src/types.rs`** (new) — `TypePath`, `EmitConfig`, `EmitError`, `BigIntBehavior`, `RenameAll`.
- **`Cargo.toml`** (root) — add `"crates/ontogen-ts"` to `workspace.members`.

## Out of scope (for this PR — these land in PRs 2-8)

- **Serde rename family** (`rename`, `rename_all`, `skip` precedence + case transforms + property tests against `serde_json::to_string`) — PR 2 (AC-4).
- **Type collection / walking / cycle detection / topological ordering** — PR 3 (AC-6, AC-7).
- **Use-resolution + external-types lookup** — PR 3 (AC-5).
- **Top-level `emit()` composition** (collecting roots, dispatching to per-type emission, aggregating errors) — PR 4 (AC-8).
- **`#[ontogen::ts_opaque]` and `#[ontogen::ts_name]` proc-macro attrs** — PR 4 (AC-9, AC-10).
- **`gen_servers` wiring** — PR 5 (AC-11).
- **Side-car deletion + iron-log cleanup** — PR 6 (AC-12, AC-13, AC-14).
- **Pumice integration validation** — PR 7 (AC-15).
- **User-facing docs** — PR 8 (AC-16).

## Acceptance criteria

These are AC-1, AC-2, AC-3 from OF-015 — restated here for the per-PR scope:

- [ ] **AC-1**: `crates/ontogen-ts/` exists as a workspace member with `Cargo.toml` registering `[lib]` and depending on `syn` (with `full` + `extra-traits` features) + `quote`. Root `Cargo.toml`'s `workspace.members` includes `"crates/ontogen-ts"`. `cargo build -p ontogen-ts` succeeds on a clean target dir.
- [ ] **AC-2**: Public API surface matches the design pass:
  - `pub struct TypePath(Vec<String>)` newtype with a constructor that rejects empty paths
  - `pub struct EmitConfig { external_types, bigint_behavior, case_default, strict_unsupported }` (fields' exact types per OF-015 design pass)
  - `pub enum EmitError { UnsupportedShape, UnsupportedSerdeAttr, UnresolvedReference, NameCollision }` with structured fields per the design pass
  - `pub fn emit(roots: &[TypePath], type_pool: &BTreeMap<TypePath, syn::Item>, config: &EmitConfig) -> Result<String, Vec<EmitError>>` signature exists (body is `todo!()` for this PR; PR 4 implements it)
- [ ] **AC-3**: Per-type emission produces correct TS for the phase-1 supported subset, validated by unit tests:
  - Named structs with primitive fields (all integers, `f32`/`f64`, `bool`, `String`, `&str`) emit `{ field: tsType, ... }`
  - `Option<T>` → `T | null`; `Vec<T>` → `T[]`; `HashMap<K, V>` / `BTreeMap<K, V>` → `Record<K, V>` (K validated as `String` or id-like primitive)
  - C-style enums emit `'Variant1' | 'Variant2' | ...`
  - Smart-pointer wrappers (`Box`, `Rc`, `Arc`, `Cow`, `Pin`) peeled transparently — `Box<u32>` and `u32` produce identical TS
  - Reference types (`&str`, `&[T]`) normalized through the same path as their owned counterparts
  - Tuple structs and unit structs return `UnsupportedShape`
  - Runtime-coordination wrappers (`RefCell`, `Mutex`, `RwLock`) return `UnsupportedShape`
  - At least one unit test per branch above

## Open questions

- **Enum representation for variants with payloads under default (untagged-variant-name) serde behavior.** Phase-1 design pass committed to "C-style enums and tagged enums where the tag is implicit from variant idents" but didn't pin down the exact TS shape for payload-carrying variants. Two candidates: `{ Variant1: Payload } | { Variant2: Payload }` (the externally-tagged JSON shape serde emits by default) or `{ type: 'Variant1', data: Payload } | ...`. Default serde behavior for non-`#[serde(tag = "...")]` enums with variant payloads is the externally-tagged shape — TS should match. Confirm during implementation that this is what we emit; if the user prefers a different shape they can opt in via `#[serde(tag = "type")]`, which is phase-2 work and lands in OF-015 phase 2.
- **`BigIntBehavior` enum variants.** Design pass committed to plumbing BigInt through config (vs the spike's hardcoded `Number`). The exact enum is `BigIntBehavior { Number, BigInt, String }` based on what `specta-typescript` exposes; verify during implementation that these are the three modes worth supporting, or follow whatever serde_json's effective behaviour suggests if the user serializes `u64`/`i64` differently.

## Notes

- Tests use `syn::parse_quote!` and `syn::parse_str` for hardcoded fixtures; no file I/O, no cargo invocation. Per-PR work-task quality checks (`just full-check`) run inside the worktree only.
- The `emit_type` fall-through for "type's terminal ident wasn't in the above branches" is deliberately permissive in this PR — it returns a placeholder so the per-type unit tests can run. PR 3 replaces this fall-through with real pool/external-types resolution.
- Per OF-015 design pass: `BTreeMap` / `BTreeSet` for any maps/sets in the public API surface so determinism is by-construction in later PRs. Set this convention here even though PR 1 doesn't use collections heavily.
