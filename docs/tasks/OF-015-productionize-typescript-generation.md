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

6. **Decide the OF-006 `FallbackRecord` warning's fate.** With ontogen-ts validating against a supported subset, the "type not found in bindings.ts" failure mode changes shape: it becomes "type is outside the supported subset" or "type is unresolved." Three options:
   - **Remove the warning**: ontogen-ts hard-errors on unsupported types; build fails fast and loud. Cleanest.
   - **Keep as belt-and-braces**: catch any type that *somehow* still slips through.
   - **Make strictness configurable** (`strict_unsupported: bool` on `EmitConfig`): hard error in strict mode, cargo:warning in lenient mode for incremental adoption.
   - Decision-driver: how Pumice's long tail actually looks. If it has a small number of types outside the subset, strict + `#[ontogen::ts_opaque]` is the right answer. If many, lenient mode buys migration headroom.

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
| `#[ontogen::ts_opaque]` proc-macro in `ontogen-macros`                  | ½ day   |
| External-types table + defaults                                         | ½ day   |
| Ontogen wiring (`gen_servers`, root collection, `type_pool`)            | 1 day   |
| Delete side-car infrastructure + iron-log cleanup                       | ½ day   |
| User-facing docs (new guide + revisions to existing pages)              | 1 day   |
| Pumice integration validation                                           | 1 day   |
| **Total**                                                               | ~8 days |

## Open questions

- **External-types defaults**: ship sensible defaults (`Uuid`, `DateTime<_>`, `NaiveDate`, `OffsetDateTime`, `Url` → `string`) or require explicit per-project declaration? Defaults are friendlier; explicit is more honest. Probably ship a small default set + allow per-project override and addition.
- **`#[serde(rename_all = "...")]` mode coverage**: serde supports `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE`. All seven needed in phase 1, or only the common four (`camelCase`, `snake_case`, `PascalCase`, `kebab-case`)? Cheap to ship all seven once we own the transform engine.
- **`#[serde(rename(serialize = "...", deserialize = "..."))]`**: split rename (different name on each direction). HTTP wire is symmetric (we both serialize and deserialize the same shape), so this almost never appears in practice. Probably reject with a clear error in phase 1; revisit if a real consumer needs it.
- **Migration semantics**: hard-cutover (delete side-car when ontogen-ts ships) or parallel-strategy via a `BindingsStrategy` enum on `ServersConfig` (consumers opt into the new path)? Hard-cutover is cleaner; parallel adds complexity that only helps if we have unresolved subset gaps when shipping. Probably hard-cutover with an upgrade-notes section in the changelog.
- **Crate publication**: keep `ontogen-ts` path-dep-only inside the workspace at first, or publish to crates.io alongside `ontogen-core` / `ontogen-macros` from day one? Publishing locks the API earlier; path-only allows ergonomic iteration. Lean path-only until the API has been used in anger.

## Notes

- **What survives from OF-014's punch-list**: `Option<Option<T>>` rendering (still a schema-known emitter detail in `ts_bindings.rs`; unrelated to ontogen-ts) and BigInt configurability (now a knob on `EmitConfig`). Everything else evaporates.
- **OF-019 becomes migration debris.** The site docs, README bullets, and iron-log example workarounds that OF-019 just shipped describe a system that no longer exists once OF-015 lands. Strip them in the same release. Document the migration path so adopters who copied the OF-019 patterns know how to clean up.
- **Specta stays as a transitive dep** on Tauri consumers because the IPC layer uses it for command marshalling. Only `specta-typescript` goes away.
- **`cargo:rerun-if-changed` coverage**: ontogen-ts emits a directive for the source file of every type it reaches via the `type_pool`. This subsumes the side-car punch-list item and is structurally easier than the side-car's never-implemented version because the AST walker already knows which files it consulted.
