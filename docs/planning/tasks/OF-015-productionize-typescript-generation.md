---
status: open
last_reviewed: 2026-05-14
---
# OF-015 - Replace the TS bindings side-car with `ontogen-ts`

- **Severity:** Medium-High. Closes the structural foot-guns the OF-014 spike documented (recursive cargo, doubled compile time, source-tree pollution, watcher loops, CI disk pressure) and obsoletes the consumer-side workarounds OF-019 just shipped. Until this lands, every new ontogen adopter pays for the side-car's surface area.
- **Status:** Open. Originally spawned 2026-05-13 from [OF-014](./OF-014-redesign-ts-bindings-pipeline.md) to *productionize* the spike. Rewritten 2026-05-14 after a design pass: replace the spike outright with a build-time AST emitter rather than polish each side-car symptom one at a time.
- **Source:** OF-014 spike outcome + OF-019 documentation lift + the design discussion the rewrite captures here.

## Problem

The OF-014 spike works on iron-log but the side-car architecture is structurally expensive: ontogen runs in build-script context, can't reach the user's types directly, and therefore drives `specta` by writing a binary into the user's crate (`src/bin/__ontogen_ts_export.rs`), compiling it through cargo with an isolated `CARGO_TARGET_DIR`, running it, and capturing stdout.

Six of the eight items on OF-014's spike punch-list are side-car *symptoms*: recursion guard via `ONTOGEN_TS_SIDECAR_INNER`, target-dir lock contention (`rust-lang/cargo#8938`), cold-build doubling, side-car source-file cleanup, `cargo run` ambiguity, Tauri watcher loops. OF-019 then surfaced three more consumer-side workarounds that exist purely to paper over the side-car (`default-run`, `.taurignore`, the CI env-gate idiom). Productionizing the spike means polishing each symptom; replacing the spike means most symptoms cease to exist.

The lift is justified because the side-car's value — driving `specta` at runtime to get TS for arbitrary Rust types — turns out not to be irreplaceable. The user's types are already syntactically visible (ontogen scans them with `syn` to find custom API endpoints). A bounded Rust→TS translator that operates on the AST closes the same gap without ever invoking user-crate code at runtime.

## Direction

Stand up a new sibling crate `ontogen-ts` (alongside `ontogen-core` and `ontogen-macros`) whose job is "given a set of root types, a pool of candidate type definitions, and an emit config, produce TypeScript source." Ontogen's `gen_servers` depends on it directly. The schema-known emitter in `ts_bindings.rs` (head: entities + generated DTOs) is unchanged; ontogen-ts handles the long tail. The side-car gets ripped out once ontogen-ts covers iron-log + Pumice's long tail.

The API (sketch — final shape lands during phase-1 implementation):

```rust
// ontogen-ts
pub fn emit(
    roots: &[TypePath],
    type_pool: &HashMap<TypePath, syn::Item>,
    config: &EmitConfig,
) -> Result<String, Vec<EmitError>>;

pub struct TypePath(Vec<String>);   // newtype; fully-qualified, ≥1 segment

pub struct EmitConfig {
    pub external_types: HashMap<String, &'static str>,  // Uuid → "string", DateTime → "string"
    pub bigint_behavior: BigIntBehavior,                // Number | BigInt | String
    pub case_default: Option<RenameAll>,                // forced rename_all when type has none
    pub strict_unsupported: bool,                       // hard error vs. warn-and-skip
}

pub enum EmitError {
    UnsupportedShape    { type_path: TypePath, reason: String },
    UnsupportedSerdeAttr{ type_path: TypePath, attr: String },
    UnresolvedReference { name: String, referenced_by: TypePath },
    NameCollision       { name: String, paths: Vec<TypePath> },
}
```

Pool-in (eager): ontogen pre-loads every candidate `syn::Item` into the `type_pool` HashMap before calling `emit`. Resolver-trait flexibility is YAGNI today; can be added as a non-breaking second entry point if a real consumer needs lazy resolution.

ontogen-ts owns: type collection (root → reachable closure, dedup, cycle detection), supported-subset validation, serde-rename rendering, name ordering, emission. Ontogen owns: scanning the user's crate for root types, building the `type_pool`, deciding how to surface errors (cargo:warning vs. build-fail).

## Scope

### In — phase 1

1. **Stand up the `ontogen-ts` crate.** Sibling to `ontogen-core` / `ontogen-macros`. Path-dep inside the workspace at first; crates.io publish deferred until the API settles.

2. **Supported subset (phase 1):**
   - Named structs (no tuple structs, no unit structs).
   - C-style enums and tagged enums where the tag is implicit from variant idents.
   - Containers: `Vec<T>`, `Option<T>`, `HashMap<K, V>`, `BTreeMap<K, V>` (key type must be `String` or an id-like primitive).
   - Primitives: `bool`, all integer types, `f32`/`f64`, `String`, `&str`.
   - References (`&str`, `&[T]`) — already AST-typed in ontogen-core post-OF-013.
   - External-types table (defaults + per-project overrides): `Uuid`, `DateTime<_>`, `NaiveDate`, `OffsetDateTime`, `Url` → TS `string` by default. Open question on whether to ship defaults or require explicit declaration.
   - `#[ontogen::ts_opaque(target = "MyTsAlias")]` escape hatch (new attr in `ontogen-macros`): user provides the TS rendering, ontogen-ts treats the type as terminal.
   - `#[ontogen::ts_name = "FooStats"]` disambiguation attr (new attr in `ontogen-macros`): overrides the default terminal-ident TS name when the user needs to break a name collision. Our attribute, our semantics — serde never sees it, JSON wire is unaffected.

3. **Serde rename family (phase 1):**
   - `#[serde(rename = "...")]` on fields and enum variants — token substitution.
   - `#[serde(rename_all = "camelCase|snake_case|PascalCase|kebab-case|SCREAMING_SNAKE_CASE|lowercase|UPPERCASE")]` on containers — case-transform table.
   - `#[serde(skip)]` on fields — drop the field.
   - **Precedence**: field-level `rename` wins over container `rename_all`. Mirror serde's behavior exactly.
   - **Case transforms**: roll our own (~100 LoC). Do *not* depend on `heck` — its rules diverge from serde on acronyms (`HTMLParser` → wrong name). Property tests round-trip small fixtures through `serde_json::to_string` to verify wire-name equality; fixtures cover the acronym + digit edge cases.

4. **Wire into ontogen's `gen_servers`:**
   - Replace `ts_sidecar::generate` call with `ontogen_ts::emit`.
   - Build `type_pool` by `syn::parse_file`-ing every `.rs` under the user's crate's `src/`, collecting struct/enum items, keyed by fully-qualified `TypePath` (module path derived from the source-file path under `src/`, plus item ident).
   - Collect root names from the existing `referenced_ts_types` + `long_tail` partition in `ts_bindings.rs`.
   - Surface `EmitError`s via the existing `FallbackRecord` channel (or a successor — see scope item 6).
   - Emit `cargo:rerun-if-changed` for the source files of types ontogen-ts reaches — single replacement for the side-car's missing rerun directives.

5. **Delete the side-car infrastructure:**
   - `src/servers/generators/ts_sidecar.rs` removed.
   - `sidecar_lib_crate_name` / `sidecar_types_module_path` helpers in `src/servers/mod.rs` removed.
   - `ONTOGEN_TS_SIDECAR_INNER` env guard removed.
   - Iron-log's `examples/iron-log/src-tauri/` cleanup (in same release):
     - Delete `.taurignore`.
     - Drop `default-run = "iron-log"` from `Cargo.toml [package]`.
     - Drop `IRON_LOG_SKIP_SERVER_CODEGEN` env-gate from `build.rs`.
     - Drop `specta-typescript` from `[dependencies]` (specta itself stays — Tauri IPC bridge uses it).
   - `src/bin/__ontogen_ts_export.rs` no longer generated.

6. **OF-006 `FallbackRecord` warning is removed entirely.** ontogen-ts has exactly two outcomes per type — emit it, or hard-error with a structured `EmitError` pointing at the type and the reason. No `Record<string, unknown>` placeholder, no warning-and-continue, no configurable strictness knob. Rationale: the configurable-strictness middle ground re-introduces exactly the silent-untyping foot-gun OF-006 originally shipped to fix, gated behind an opt-in. The `#[ontogen::ts_opaque(target = "...")]` escape hatch covers the legitimate case where a user wants to opt a third-party type out. Build-fail UX: collect all errors into `Vec<EmitError>`, render each as a `cargo:warning` line, then panic — user sees the full punch-list in one build, not fix-one-rebuild-fix-one. Migration note: the `FallbackRecord` type and the warning-emission code paths in `transport.rs` / `ts_client.rs` get deleted; any consumer grep rule that matched the old warning text breaks (call this out in the cutover release's changelog).

7. **User-facing docs:**
   - New `site/src/content/docs/guides/typescript-bindings.mdx` — the end-to-end TS bindings guide OF-006 originally asked for, now unblocked.
   - Rewrite `guides/client-generation.mdx`'s `bindings_path` section *again* — the OF-019 rewrite still references the spike's mechanism (specta side-car + side-car write); revise to point at ontogen-ts.
   - Strip the "Integration gotchas" section from `client-generation.mdx` — its three subsections (`default-run`, `.taurignore`, CI env-gate) are all side-car-only and no longer apply.
   - Strip the `.taurignore` step from `cookbook/tauri-integration.mdx` and remove `default-run` from the recipe Cargo.toml.
   - Strip the third "Known Issues" bullet from `README.md` (the side-car summary added by OF-019).
   - Document the supported subset, the external-types table, and the `#[ontogen::ts_opaque]` escape hatch.

### In — phase 2 (this ticket if cheap, otherwise spawned)

8. **Shape-changing serde attrs:** `tag`, `content`, `untagged`, `flatten`. Materially more work than the rename family — `untagged` emits TS unions; `flatten` requires structural merging; `tag`/`content` change the wire shape from `{variant: payload}` to `{type: "variant", ...payload}` (internally tagged) or `{type: "variant", content: payload}` (adjacently tagged). Defer until phase 1 ships and a real consumer needs them; spawn a separate ticket if so.

### Out

- **Alternative output targets** (Zod schemas, OpenAPI, JSON Schema). OF-014 open question 2; needs its own design.
- **Macro-generated types** (anything produced by a derive macro that ontogen-ts can't see at AST level — e.g., `#[derive(Builder)]` synthesizing accessor types). Document as a known limitation; users who need them stay on `#[ontogen::ts_opaque]` or contribute a follow-up.
- **`ts-rs` / `typeshare` adoption** as the long-tail engine instead of building our own. Evaluated and rejected during the design pass — both are bolted-on derive crates with their own subset rules, and we don't want to inherit their attribute semantics or release cadence. Revisit if our subset proves more painful to maintain than expected.

## Crate naming

Initial: `ontogen-ts` (family-namespaced, sibling to `ontogen-core` + `ontogen-macros`). Signals it's part of ontogen's release cadence and consumers know to upgrade in lockstep. If we later want broader discoverability ("rust ast → typescript" search results, standalone adoption outside ontogen), rename at crates.io-publish time — that's a one-commit cost. Don't optimize for the standalone-library outcome up front.

## Effort

Medium. Substantial new code but bounded scope and well-defined API. Most algorithmic work (AST walking, dedup, cycle detection, supported-subset matching) is straightforward `syn` idioms. The case-transform engine is the one piece needing careful spec compliance.

Rough breakdown (single dev, focused weeks):

| Slice                                                                  | Effort  |
|------------------------------------------------------------------------|---------|
| ontogen-ts scaffold + `TypePath` + `EmitConfig` + `EmitError`          | ½ day   |
| Per-type emission (struct + enum, primitives + containers)             | 1 day   |
| Serde rename phase-1 (transforms + property tests)                     | 1 day   |
| Type collection + cycle detection                                       | ½ day   |
| Supported-subset validation                                             | ½ day   |
| `#[ontogen::ts_opaque]` + `#[ontogen::ts_name]` attrs in `ontogen-macros` | ½ day |
| External-types table + defaults                                         | ½ day   |
| Ontogen wiring (`gen_servers`, root collection, `type_pool`)            | 1 day   |
| Delete side-car infrastructure + iron-log cleanup                       | ½ day   |
| User-facing docs (new guide + revisions to existing pages)              | 1 day   |
| Pumice integration validation                                           | 1 day   |
| **Total**                                                               | ~8 days |

## PR breakdown

Phase-1 implementation lands as a sequence of 8 PRs, each against `main` from a branch in the `worktree-of-015-ontogen-ts` worktree. Each PR represents shippable state — CI green, workspace builds — and references the ACs below it satisfies. Earlier PRs are library scaffolding (no behavioural change to ontogen); PR 5 is the functional cutover (ontogen-ts replaces the side-car in `gen_servers`); PR 6 deletes the now-dead side-car code; PR 7 validates against Pumice; PR 8 ships docs.

| PR | Scope | Stages | Satisfies ACs |
|----|-------|--------|---------------|
| 1  | `crates/ontogen-ts/` scaffold + per-type emission (struct + enum, primitives + hardcoded containers + smart-pointer peel) | 1 + 2 | AC-1, AC-2, AC-3 |
| 2  | Serde rename engine (8 modes, our own transforms, property tests vs `serde_json::to_string`) | 3 | AC-4 |
| 3  | Type collection, topological ordering, use-resolution, external-types table | 4 + 5 | AC-5, AC-6, AC-7 |
| 4  | Top-level `emit` entry point + `#[ontogen::ts_opaque]` / `#[ontogen::ts_name]` proc-macro attrs | 6 + 7 | AC-8, AC-9, AC-10 |
| 5  | Ontogen wiring — `gen_servers` calls `ontogen_ts::emit` instead of `ts_sidecar::generate`; side-car code still present but unused | 8 | AC-11 |
| 6  | Side-car deletion + iron-log workaround cleanup + `FallbackRecord` removal | 9 | AC-12, AC-13, AC-14 |
| 7  | Pumice integration validation + any subset-gap backports into earlier PRs | 10 | AC-15 |
| 8  | User-facing docs (new TS-bindings guide, `client-generation.mdx` rewrite, OF-019 doc rollback) | 11 | AC-16 |

## Acceptance criteria

- [ ] **AC-1**: `crates/ontogen-ts/` exists as a workspace member with `Cargo.toml` registering `[lib]` and depending on `syn` + `quote` (and any other phase-1 deps). Root `Cargo.toml`'s `workspace.members` includes `"crates/ontogen-ts"`. `cargo build -p ontogen-ts` succeeds on a clean target dir.
- [ ] **AC-2**: Public API surface matches the design pass:
  - `pub fn emit(roots: &[TypePath], type_pool: &BTreeMap<TypePath, syn::Item>, config: &EmitConfig) -> Result<String, Vec<EmitError>>`
  - `pub struct TypePath(Vec<String>)` newtype with constructor that rejects empty paths
  - `pub struct EmitConfig { external_types, bigint_behavior, case_default, strict_unsupported }`
  - `pub enum EmitError { UnsupportedShape, UnsupportedSerdeAttr, UnresolvedReference, NameCollision }` with structured fields per the design pass
- [ ] **AC-3**: Per-type emission produces correct TS for the phase-1 supported subset:
  - Named structs with primitive fields (all integers, `f32`/`f64`, `bool`, `String`, `&str`) emit `{ field: tsType, ... }`
  - `Option<T>` → `T | null`; `Vec<T>` → `T[]`; `HashMap<K, V>` / `BTreeMap<K, V>` → `Record<K, V>` (K must be `String` or id-like primitive)
  - C-style enums emit `'Variant1' | 'Variant2' | ...`
  - Tagged enums (variant-name-only, no `#[serde(tag)]` yet) emit per phase-1 representation rules
  - Smart-pointer wrappers (`Box`, `Rc`, `Arc`, `Cow`, `Pin`) peeled transparently
  - Reference types (`&str`, `&[T]`) normalized through the same path as their owned counterparts
- [ ] **AC-4**: Serde rename family works correctly, validated by property tests:
  - `#[serde(rename = "wireName")]` on fields and enum variants substitutes the wire name
  - `#[serde(rename_all = "...")]` on containers covers all 8 serde modes (`lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE`)
  - Field-level `rename` wins over container `rename_all` (precedence preserved)
  - `#[serde(skip)]` drops the field from TS emission
  - At least 20 fixture round-trips through `serde_json::to_string` confirm wire-name equality (acronym + digit edge cases included: `HTMLParser → htmlParser`, `parse_url_v2 → parseUrlV2`, etc.)
- [ ] **AC-5**: External-types table works as designed:
  - Default set shipped: `chrono::DateTime`, `chrono::NaiveDate`, `chrono::NaiveDateTime`, `chrono::NaiveTime`, `time::OffsetDateTime`, `time::PrimitiveDateTime`, `time::Date`, `time::Time`, `uuid::Uuid`, `url::Url`, `std::path::PathBuf`, `std::net::IpAddr`, `std::net::Ipv4Addr`, `std::net::Ipv6Addr` → `"string"`; `serde_json::Value` → `"unknown"`
  - User-provided overrides via `EmitConfig.external_types` merge on top (user wins on conflict)
  - Walker matches against canonical paths (`chrono::DateTime`), not terminal idents — generic args stripped at match time so `DateTime<Utc>`, `DateTime<Local>`, etc. all hit the same entry
- [ ] **AC-6**: Type collection and ordering work as designed:
  - `type_pool` keyed by canonical `TypePath`, value `syn::Item` (struct, enum, or alias)
  - Pool walker collects every module-level `ItemStruct` / `ItemEnum` / `ItemType` under user's `src/`, regardless of visibility
  - Roots reach transitive closure via field-type walking
  - Cycles detected (self-referential types emit as `interface X { children: X[] }`-equivalent)
  - Output ordering is deterministic: topological by reference, alphabetical-by-canonical-path within each topo level
  - `BTreeMap` / `BTreeSet` used throughout — HashMap iteration order never leaks into output
- [ ] **AC-7**: Use-resolution works correctly:
  - Per-file `use` declarations parsed during `type_pool` construction
  - One-segment refs (`DateTime`) resolve through the file's imports table to canonical paths (`chrono::DateTime`)
  - Multi-segment refs (`chrono::DateTime`, `crate::foo::Bar`) taken as-qualified; `crate::` prefix stripped for local-type pool lookup
  - Glob imports (`use chrono::*`) rejected at resolution time with `UnresolvedReference` carrying a hint pointing the user at qualification or explicit `use`
  - Re-exports: canonical pool key is path-where-defined; re-export resolution happens before pool lookup
- [ ] **AC-8**: Top-level `emit` function correctly composes collection + validation + emission:
  - All errors collected into `Vec<EmitError>` before failing (no first-error fail-fast)
  - Hard error on unsupported shapes — no `FallbackRecord` placeholder, no warning-and-continue
  - Build-fail UX: each `EmitError` renders as a `cargo:warning` line, then `panic!` with summary
  - End-to-end test on a synthetic crate covering primitives, containers, references, serde renames, external types, and at least one negative case (unsupported shape) all in one fixture
- [ ] **AC-9**: Proc-macro attrs land in `crates/ontogen-macros/`:
  - `#[ontogen::ts_opaque(target = "MyTsAlias")]` — walker treats the type as terminal; emits the target string at reference sites without recursing into the type's fields
  - `#[ontogen::ts_name = "FooStats"]` — overrides the default terminal-ident TS name during emission and collision-detection; JSON wire unaffected (serde never sees the attr)
  - Both attrs are no-ops at Rust compile time (no token generation; ontogen-ts reads them via syn AST inspection)
- [ ] **AC-10**: Name-collision detection works:
  - Two reachable types emitting to the same TS name (post-`ts_name` resolution) fail with `EmitError::NameCollision { name, paths }` listing all colliding canonical paths
  - Error human-renders with fix-path hints in priority order: `#[ontogen::ts_name = "..."]`, `#[ontogen::ts_opaque(target = "...")]`, Rust-side rename
  - Unreachable collisions (two same-named types in the pool that neither reach from a root) do not error
- [ ] **AC-11**: Ontogen pipeline wiring uses ontogen-ts:
  - `src/servers/mod.rs::generate_transport` calls `ontogen_ts::emit` instead of `ts_sidecar::generate` for the long-tail emission slice
  - `type_pool` constructed by walking `.rs` files under the user's `src/` (path derived from build-script invocation context)
  - Root names harvested from existing `ts_bindings::referenced_ts_types` / `ts_bindings::long_tail` partitions
  - `EmitError`s surfaced as `cargo:warning` lines + panic, same shape as existing `CodegenError` handling
  - `cargo:rerun-if-changed` emitted for every source file referenced during emission (one per file in the type_pool's reach-set)
  - Iron-log builds clean: `cargo build` in `examples/iron-log/src-tauri/` succeeds; `bindings.ts` content equivalent or better than the side-car's output (no missing types, no extra noise)
- [ ] **AC-12**: Side-car infrastructure fully deleted:
  - `src/servers/generators/ts_sidecar.rs` removed
  - `src/servers/mod.rs::sidecar_lib_crate_name` and `sidecar_types_module_path` helpers removed
  - `ONTOGEN_TS_SIDECAR_INNER` env guard removed from `generate_transport`
  - `FallbackRecord` type and warning-emission paths in `transport.rs` / `ts_client.rs` removed
  - No `cargo run` of an inner build-script binary anywhere in `gen_servers`'s code path
- [ ] **AC-13**: Iron-log example cleanup:
  - `examples/iron-log/src-tauri/.taurignore` deleted (no longer needed without the side-car)
  - `default-run = "iron-log"` removed from `examples/iron-log/src-tauri/Cargo.toml [package]` (the side-car bin no longer exists, so `cargo run` is unambiguous)
  - `IRON_LOG_SKIP_SERVER_CODEGEN` env-gate removed from `examples/iron-log/src-tauri/build.rs` (no more CI disk-pressure concern)
  - `specta-typescript = "=0.0.10"` removed from `[dependencies]` (the long-tail-emitter dep is gone). `specta` itself stays — Tauri IPC bridge uses it.
- [ ] **AC-14**: End-to-end iron-log build is clean post-cleanup: `cargo build` in `examples/iron-log/src-tauri/` succeeds with zero fallback warnings; `src/bin/__ontogen_ts_export.rs` is no longer regenerated; `bindings.ts` (`examples/iron-log/src-nuxt/app/generated/types.ts`) contains all expected entity + DTO + long-tail types in deterministic order.
- [ ] **AC-15**: Pumice integration validates phase-1's supported subset covers their full long-tail. Run ontogen-ts against Pumice's current branch before declaring phase 1 done; catalog any unsupported-shape errors; if any surface, backport fixes to PRs 1-4 *before* PR 6 (side-car deletion) lands, so Pumice retains a working fallback throughout. After fixes, Pumice's build is clean against the new pipeline.
- [ ] **AC-16**: User-facing docs land:
  - New `site/src/content/docs/guides/typescript-bindings.mdx` — the end-to-end TS-bindings guide OF-006 originally asked for, now describing the ontogen-ts model
  - `site/src/content/docs/guides/client-generation.mdx` `bindings_path` section rewritten to reflect ontogen-ts (replaces the OF-019 spike-grade prose); "Integration gotchas" section removed (no longer applicable)
  - `site/src/content/docs/cookbook/tauri-integration.mdx`: `.taurignore` step removed; `default-run` removed from recipe Cargo.toml; subsequent steps renumbered
  - `README.md`: third "Known Issues" bullet (OF-019 summary) removed
  - Supported subset documented (struct shapes, enum shapes, container handling, smart-pointer transparency, external-types table)
  - `#[ontogen::ts_opaque]` and `#[ontogen::ts_name]` attrs documented with examples
- [ ] **AC-17**: After all PRs land, `just full-check` passes on main; `cargo build` in `examples/iron-log/src-tauri/` succeeds; CI workflows pass.

## Decisions captured during the design pass (2026-05-14)

- **Migration semantics**: hard-cutover. Delete `ts_sidecar.rs` + spike scaffolding when ontogen-ts ships; no parallel `BindingsStrategy` enum. The side-car was spike-grade from day one and the only consumers are iron-log + Pumice. Parallel paths would force us to maintain both code surfaces through a long deprecation window — worst of both worlds. Pumice has the OF-019 workarounds as a working fallback if ontogen-ts has subset gaps on cutover day; mitigation is to validate ontogen-ts against Pumice's full long-tail set before the cutover release and treat any gap as a phase-1 blocker.
- **OF-006 `FallbackRecord` warning**: removed entirely. Hard-error only (see scope item 6). The `strict_unsupported: bool` knob is *not* added — it would re-introduce the silent-untyping foot-gun under an opt-in.
- **Type-pool scope**: most permissive at the collector layer; selection happens during validation. Walk every `.rs` file under the user's `src/`, collect every `ItemStruct` / `ItemEnum` / `ItemType` defined at module level regardless of visibility, key by fully-qualified `TypePath`. `pub(crate)` types are included because they can be referenced from a `pub` API endpoint and still flow over the wire. Function-local and impl-block-nested types are excluded because they can't appear as plain return-type idents in a public API signature. Type aliases and generic structs go in the pool even though their *emission* policy is separate — pooling them keeps "unsupported shape" errors honest instead of disguising them as "unresolved reference."
- **Scan root**: `src/` only. Not `examples/`, `benches/`, `tests/`, or `build.rs`. Those don't ship wire code.
- **`cfg`-gated types**: pooled like any other. `syn::parse_file` gives us raw AST without cfg-eval; if a non-feature-gated root references a feature-gated type, the build fails — but it would have failed at Rust compile time too, so we're just surfacing the error earlier with a clearer message.
- **Re-exports**: canonical key in the pool is the path where the item is *defined*, not where it's re-exported. Re-export resolution happens in root-collection (ontogen side) before pool lookup.
- **Name-collision behavior**: hard error via `NameCollision { name, paths }`. Triggered at emit time, only on collisions between *reachable* types — two same-named types in the pool that neither reach from a root are fine. Error message lists both Rust paths plus the three fix paths in priority order: `#[ontogen::ts_name = "..."]` (preferred — TS-only rename, wire unaffected), `#[ontogen::ts_opaque(target = "...")]` (if one type should be opaque anyway), Rust-side rename (brute force). Hierarchical TS-directory emission as a richer disambiguation strategy is filed as [OF-020](./OF-020-hierarchical-ts-bindings.md) — speculative future work, only earns its keep if a real consumer hits collision-fatigue.
- **User-defined generics**: rejected in phase 1 with `UnsupportedShape { type_path, reason: "user-defined generics not supported in phase 1; use a concrete type alias (e.g. `pub type PaginatedWorkouts = Paginated<Workout>`) or `#[ontogen::ts_opaque]`" }`. The hardcoded container generics (`Option<T>`, `Vec<T>`, `HashMap<K, V>`, `BTreeMap<K, V>`) continue to work because each has a known TS rendering the emitter hardcodes. User generics require generic-instantiation tracking, name-mangling, and a decision between monomorphization-as-default vs TS-generic emission — all material work not justified by current consumers. Future support tracked in [OF-021](./OF-021-user-defined-generics-in-ts-emitter.md).
- **Smart-pointer transparency**: `Box<T>`, `Rc<T>`, `Arc<T>`, `Cow<'_, T>`, and `Pin<P>` are peeled silently before classification. All five are transparent to `serde_json` at runtime (the wire shape is identical regardless of wrapper), so forcing user annotation around them would be friction for no semantic gain. Implementation: one extra step in the walker — peel any of `{Box, Rc, Arc, Cow, Pin}` from the head of a type expression and re-classify the inner type. Runtime-coordination primitives (`RefCell<T>`, `Mutex<T>`, `RwLock<T>`) are *not* in the peel set — they're rejected with `UnsupportedShape` because they're coordination primitives that shouldn't appear in wire types; user can refactor or use `#[ontogen::ts_opaque]`. Custom user-defined wrapper types like `pub struct Inner<T>(T)` fall through to the user-generics path (rejected per OF-021 in phase 1).
- **Type-alias handling**: follow + inline. When the walker encounters a reference to a type alias, resolve it through the pool and substitute the underlying type at the reference site; emit no separate `type Foo = ...` declaration. Matches `serde_json`'s wire behavior (aliases are invisible at runtime) and enables OF-021's concrete-alias escape hatch (`pub type PaginatedWorkouts = Paginated<Workout>` resolves through the alias to `Paginated<T = Workout>`, the walker substitutes `T = Workout` into `Paginated`'s field list, and emits `export type PaginatedWorkouts = { items: Workout[]; total: number }` — single-instance monomorphization for free as a side-effect of alias resolution). Chained aliases resolve recursively; cycle detection is a belt-and-braces safety check since cycles in `type` aliases don't compile at the Rust level. Aliases with unsubstituted type parameters (`type Foo<T: Display> = HashMap<String, T>`) are rejected in phase 1 — user must write a concrete alias instead. Aliases that resolve to external types chain naturally through the external-types table.
- **Use-resolution / path canonicalization**: the walker resolves each referenced type's source path to its canonical full path before any lookup. Per-file `use` declarations are parsed during `type_pool` construction and held as a local imports table. When the walker sees a `syn::Type::Path` with one segment, it consults the imports table; if the segment maps to a `use` target, that's the canonical path. If multi-segment, the path is already qualified (strip `crate::` for local types). This resolution is required for `type_pool` lookups to work (pool is keyed by canonical paths from the [Type-pool scope](#decisions-captured-during-the-design-pass-2026-05-14) decision) and is reused for `external_types` lookups — same canonical-path query against both tables in turn.
- **External-types match shape**: full canonical path. `external_types` is keyed by canonical paths like `"chrono::DateTime"`, `"uuid::Uuid"`, `"time::OffsetDateTime"`. Walker resolves source paths to canonical via the use-resolution machinery above, then queries the table. Distinct entries for distinct origin crates means no terminal-ident collision risk; users with `chrono::DateTime` and (hypothetically) `time::DateTime` can render them differently. Generic args are stripped at match time — `DateTime<Utc>`, `DateTime<Local>`, `DateTime<FixedOffset>` all hit the same `"chrono::DateTime"` entry and emit the configured TS rendering. The walker treats external-types matches as terminal (doesn't recurse into generic args).
- **Glob imports**: rejected at the resolution layer in phase 1. `use chrono::*;` followed by a reference to `DateTime<Utc>` can't be resolved without parsing the imported crate's source, which is out of scope. Walker emits `UnresolvedReference { name, referenced_by, hint: "type may come from a glob import; qualify the reference (e.g., chrono::DateTime<Utc>) or replace the glob with an explicit use" }`. Cheap and clear; if real consumers hit this often, follow-up tickets can add glob support or terminal-ident fallback.
- **External-types defaults**: ship a small built-in set, merged with user-provided overrides (user wins on conflict). Standard library-defaults pattern; ergonomic for the common case, honest for power users. Default set (all rendered as TS `string` unless noted): `chrono::DateTime`, `chrono::NaiveDate`, `chrono::NaiveDateTime`, `chrono::NaiveTime`, `time::OffsetDateTime`, `time::PrimitiveDateTime`, `time::Date`, `time::Time`, `uuid::Uuid`, `url::Url`, `std::path::PathBuf`, `std::net::IpAddr`, `std::net::Ipv4Addr`, `std::net::Ipv6Addr`, plus `serde_json::Value` → `unknown`. Deliberately excluded because their wire encoding depends on consumer serde flags: `std::time::Duration`, `std::time::SystemTime`, `bytes::Bytes`, `rust_decimal::Decimal`, `bigdecimal::BigDecimal`. Override semantics: `default_map.chain(user_map).collect()` — user-provided keys overwrite defaults on collision. No explicit "unset" mechanism; user can supply an alternative rendering instead.
- **Rendering shape (phase-1)**: the value type for `external_types` in phase 1 is `&'static str` (or owned `String`) — primitive TS renderings only. Richer renderings (e.g., `import { Moment } from 'moment'` for `chrono::DateTime`) are an additive future extension tracked in [OF-022](./OF-022-richer-external-type-renderings.md). Phase-1's API is forward-compatible via a future `From<&str> for ExternalTypeRendering` conversion when OF-022 lands.
- **Output determinism**: topological by reference order, alphabetical-by-canonical-path within each topo level, with cycle members co-emitted as an alphabetical group. Achieved primarily *by construction*: `type_pool` is a `BTreeMap<TypePath, syn::Item>`, the visited-set for cycle detection is a `BTreeSet<TypePath>`, the dependency graph adjacency lists are `BTreeSet<TypePath>` per node. HashMap iteration order never leaks into the output. Kahn's algorithm over the dep graph produces topo levels; within each level, the already-sorted BTreeSet provides the alphabetical tiebreaker for free. Cost: `O(log n)` lookups vs `O(1)`, negligible for the few-hundred-type scale ontogen-ts operates at. Benefit: determinism without scattering `.sort()` calls at output boundaries. Note: ordering is by *canonical Rust path*, not by emitted TS name — adding/removing a `#[ontogen::ts_name]` annotation doesn't reorder the output.
- **`rename_all` mode coverage**: all eight serde modes — `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE`. Cheap once we own the transform engine: all share one `split_words` implementation (the hard part — handling acronym boundaries like `HTMLParser → ["HTML", "Parser"]` and digit transitions like `parse_url_v2 → ["parse", "url", "v2"]`) plus one `match` arm per target mode (~15-20 LoC each). Property tests round-trip ~15-20 fixture idents through `serde_json::to_string` to verify wire-name equality with serde's canonical behavior; `heck` is deliberately not used because its acronym rules diverge from serde's (`heck` produces `hTMLParser` for camelCase; serde produces `htmlParser`).
- **Split-rename rejected**: `#[serde(rename(serialize = "...", deserialize = "..."))]` raises `UnsupportedSerdeAttr { type_path, attr: "split-rename" }` with a hint pointing the user at the symmetric form (`#[serde(rename = "...")]`) or `#[ontogen::ts_opaque]` if they need to keep the serde asymmetry for non-ontogen-ts consumers. Rationale: the TS wire is symmetric — `taskCreate(input: CreateTaskInput)` has one `CreateTaskInput` type definition with no place for "field is `userName` on read, `user_name` on write." Picking one direction silently masks the asymmetry the user explicitly declared; refusing is more honest. Empirically rare in real Rust web APIs; not worth phase-1 complexity. Future support would emit two TS types per split-renamed Rust type (`FooSend` / `FooReceive`) and route them at the transport-emitter layer; meaningful complication if motivated, but no real consumer asks for it today.
- **Repo location**: `crates/ontogen-ts/`. New crate lands directly under `crates/` (not at repo root next to `ontogen-core/` / `ontogen-macros/`). Existing siblings will be relocated under `crates/` too as a separate cleanup — tracked in [OF-023](./OF-023-relocate-workspace-members-under-crates.md). OF-015's implementation can proceed without OF-023 landing first, but if OF-023 ships before OF-015 the new crate slots in at the conventional path from day one.
- **Crate publication**: path-dep-only through phase 1. `crates/ontogen-ts/` is a workspace member; ontogen depends on it via `path = "crates/ontogen-ts"`. The release-plz / changelog automation skips it; no version is pushed to crates.io. After iron-log builds clean and Pumice's full long-tail is covered by phase-1's supported subset (the stabilization milestone), a `0.1.0` publish is the natural cut-over. Rationale: publishing locks the API; the surface sketched in this design pass is informed but not battle-tested, and we'll likely discover shape mistakes during implementation that are cheap as workspace edits and expensive as semver bumps. Standard pattern used by serde / tokio / most multi-crate Rust projects — workspace-private until shape settles, then publish.

## Open questions

None remaining after the 2026-05-14 design pass. All phase-1 decisions are captured in the section above; future-direction questions migrated to dedicated tickets ([OF-020](./OF-020-hierarchical-ts-bindings.md), [OF-021](./OF-021-user-defined-generics-in-ts-emitter.md), [OF-022](./OF-022-richer-external-type-renderings.md), [OF-023](./OF-023-relocate-workspace-members-under-crates.md)).

## Notes

- **What survives from OF-014's punch-list**: `Option<Option<T>>` rendering (still a schema-known emitter detail in `ts_bindings.rs`; unrelated to ontogen-ts) and BigInt configurability (now a knob on `EmitConfig`). Everything else evaporates.
- **OF-019 becomes migration debris.** The site docs, README bullets, and iron-log example workarounds that OF-019 just shipped describe a system that no longer exists once OF-015 lands. Strip them in the same release. Document the migration path so adopters who copied the OF-019 patterns know how to clean up.
- **Specta stays as a transitive dep** on Tauri consumers because the IPC layer uses it for command marshalling. Only `specta-typescript` goes away.
- **`cargo:rerun-if-changed` coverage**: ontogen-ts emits a directive for the source file of every type it reaches via the `type_pool`. This subsumes the side-car punch-list item and is structurally easier than the side-car's never-implemented version because the AST walker already knows which files it consulted.
