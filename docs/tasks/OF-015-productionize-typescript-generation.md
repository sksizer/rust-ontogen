---
status: open
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

## Open questions

- **`#[serde(rename_all = "...")]` mode coverage**: serde supports `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE`. All seven needed in phase 1, or only the common four (`camelCase`, `snake_case`, `PascalCase`, `kebab-case`)? Cheap to ship all seven once we own the transform engine.
- **`#[serde(rename(serialize = "...", deserialize = "..."))]`**: split rename (different name on each direction). HTTP wire is symmetric (we both serialize and deserialize the same shape), so this almost never appears in practice. Probably reject with a clear error in phase 1; revisit if a real consumer needs it.
- **Output determinism**: explicit topological + alphabetical ordering so `bindings.ts` doesn't churn across builds.
- **Repo location**: `ontogen-ts/` at repo root (matches `ontogen-core` / `ontogen-macros`) or under a `crates/` dir?
- **Crate publication**: keep `ontogen-ts` path-dep-only inside the workspace at first, or publish to crates.io alongside `ontogen-core` / `ontogen-macros` from day one? Publishing locks the API earlier; path-only allows ergonomic iteration. Lean path-only until the API has been used in anger.

## Notes

- **What survives from OF-014's punch-list**: `Option<Option<T>>` rendering (still a schema-known emitter detail in `ts_bindings.rs`; unrelated to ontogen-ts) and BigInt configurability (now a knob on `EmitConfig`). Everything else evaporates.
- **OF-019 becomes migration debris.** The site docs, README bullets, and iron-log example workarounds that OF-019 just shipped describe a system that no longer exists once OF-015 lands. Strip them in the same release. Document the migration path so adopters who copied the OF-019 patterns know how to clean up.
- **Specta stays as a transitive dep** on Tauri consumers because the IPC layer uses it for command marshalling. Only `specta-typescript` goes away.
- **`cargo:rerun-if-changed` coverage**: ontogen-ts emits a directive for the source file of every type it reaches via the `type_pool`. This subsumes the side-car punch-list item and is structurally easier than the side-car's never-implemented version because the AST walker already knows which files it consulted.
