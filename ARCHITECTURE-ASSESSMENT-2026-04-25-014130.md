# Architecture Assessment — 2026-04-25

> **Status:** Findings #1, #2, #3, #4, #5, and #6 have been addressed. See per-finding **Resolution** notes.

## Executive Summary

- **Sound pipeline design**: six composable generators with clean staged IR; the core concept is solid and well-executed.
- ~~**ServersOutput is always empty**~~ **[RESOLVED]**: `generate()` now populates `http_routes`, `ipc_commands`, and `mcp_tools` from the parsed `ApiModule` list — see Finding #1.
- ~~**Dual OpKind types with diverging semantics**~~ **[RESOLVED]**: consolidated into a single `ir::OpKind` covering CRUD + Junction + Custom + EventStream. See Finding #3.
- ~~**Several `_`-prefixed "unused enrichment" parameters**~~ **[RESOLVED via lib.rs docstrings]**: The `seaorm`, `api`, and `scan_dirs` parameters on the public generators are documented as reserved for future enrichment; consumers know to pass `None`/`&[]` today. See Finding #4.
- ~~**`StoreMethodMeta.params` is always empty**~~ **[RESOLVED]**: parameters now mirror the actual generated CRUD signatures. See Finding #2.
- ~~**Pervasive "TODO: review" banners**~~ **[RESOLVED via PR #42]**: Banners removed from 28 source and doc files. See Finding #5.

---

## Codebase Shape

```
rust-ontogen/
├── ontogen-core/          # Zero-dep crate: model types, IR structs, CodegenError, utilities
│   └── src/
│       ├── ir.rs          # Pipeline IR types (SchemaOutput → … → ServersOutput)
│       ├── model.rs       # EntityDef, FieldDef, FieldType, FieldRole, RelationInfo
│       ├── naming.rs      # to_snake_case, to_pascal_case, pluralize
│       └── utils.rs       # rustfmt, write_if_changed, clean_generated_dir, emit_rerun_directives
├── ontogen-macros/        # Proc-macro crate: #[derive(OntologyEntity)] (intentional no-op)
│   └── src/lib.rs
├── src/                   # Main ontogen generator crate
│   ├── lib.rs             # Public API: 8 functions + 8 config structs
│   ├── ir.rs              # Re-export shim for ontogen_core::ir
│   ├── schema/            # Stage 1: syn-based parsing of #[ontology(...)] annotations
│   ├── persistence/       # Stage 2: SeaORM entities, markdown I/O, DTOs
│   │   ├── seaorm/        # gen_entity.rs, gen_conversion.rs
│   │   ├── markdown/      # gen_parser.rs, gen_writer.rs, gen_fs_ops.rs
│   │   └── dto.rs         # Create/Update input type generation
│   ├── store/             # Stage 3: CRUD impl, Update structs, lifecycle hooks
│   ├── api/               # Stage 4: CRUD forwarding + scan-and-merge of hand-written modules
│   └── servers/           # Stage 5: HTTP/Axum, Tauri IPC, MCP, TypeScript clients
│       ├── classify.rs    # Internal OpKind + classify_op()
│       ├── config.rs      # Generator configuration types
│       ├── parse.rs       # syn-based API module scanning
│       ├── types.rs       # norm_type, capitalize, collect_type_import
│       └── generators/    # http.rs, ipc.rs, mcp.rs, ts_client.rs, transport.rs, admin.rs
├── justfile               # Task runner: build, lint, test, release, template sync
├── .github/workflows/     # CI: format-check + clippy + cargo test
└── examples/iron-log/     # Reference downstream project
```

---

## Findings

### 1. IR Contract — ServersOutput is always empty  **[RESOLVED]**

- **Severity:** high
- **Location:** `src/servers/mod.rs:78–80`
- **Observation:** `servers::generate()` returns `ServersOutput { http_routes: vec![], ipc_commands: vec![], mcp_tools: vec![] }` on every call. The actual routes/commands/tools are generated as side effects (written to disk), but the IR output is never populated. Line 78 has an explicit `let _ = modules; // TODO: extract route/command metadata`.
- **Why it matters:** `ServersOutput` is defined in the public IR (and fully typed with `HttpRouteMeta`, `IpcCommandMeta`, `McpToolMeta`) specifically so client generators can mirror server shapes exactly. As-is, any code consuming `ServersOutput` gets empty vecs. Future phases (typed channels, CLI generator) that depend on this IR are blocked.
- **Suggested direction:** After `generate_transport()` returns the parsed module list, iterate it to build `HttpRouteMeta`/`IpcCommandMeta`/`McpToolMeta` from the same information the generators use. Alternatively, have each generator append to a shared accumulator passed by mutable reference.
- **Resolution:** Added `extract_server_metadata` in `src/servers/mod.rs` that walks the same `modules` list the generators consume and produces `HttpRouteMeta`, `IpcCommandMeta`, and `McpToolMeta` records. HTTP routes correctly mirror prefix-scoping for store-based modules under `route_prefix`; events get both unscoped and scoped variants when prefix is set. Also changed `McpToolMeta` from `params_schema: String` (which was effectively unfillable from compile-time inputs) to `params: Vec<ParamMeta>` to match the IPC variant. Five new tests pin the IR shape (`test_extract_metadata_*` in `src/servers/tests.rs`).

---

### 2. IR Contract — StoreMethodMeta.params always empty  **[RESOLVED]**

- **Severity:** medium
- **Location:** `src/store/mod.rs:150–192` (`collect_method_meta`)
- **Observation:** Every `StoreMethodMeta` constructed in `collect_method_meta` hard-codes `params: vec![]`. The `list_*` method actually takes `(limit: Option<u64>, offset: Option<u64>)`; `get_*`, `update_*`, `delete_*` each take an `id: &str`; `create_*` takes an input struct. The IR type supports params but they are never populated.
- **Why it matters:** Downstream consumers (e.g., API generation that should use `StoreOutput` instead of re-deriving method signatures) cannot use the metadata. It also creates a gap between the IR's promise and its content.
- **Suggested direction:** Populate `params` in `collect_method_meta` to match the actual generated signatures. The information is already available from `EntityDef`.
- **Resolution:** `collect_method_meta` now emits `params` matching the real signatures: `list_*` carries `(limit, offset)`; `get_*`, `delete_*` carry `(id)`; `update_*` carries `(id, updates)`; `create_*` carries `({snake}: {Name})`. Test `method_meta_params_match_signatures` pins the exact shapes so future drift in `gen_crud` triggers a test failure.

---

### 3. Dual OpKind with Diverging Semantics  **[RESOLVED]**

- **Severity:** medium
- **Location:** `ontogen-core/src/ir.rs:188–200` vs. `src/servers/classify.rs:8–41`
- **Observation:** Two separate `OpKind` enums exist:
  - `ir::OpKind` (public): `List, GetById, Create, Update, Delete, CustomGet, CustomPost, EventStream`
  - `servers::classify::OpKind` (internal): `List, GetById, Create, UpdateById, DeleteById, JunctionList, JunctionAdd, JunctionRemove, CustomGet, CustomPost`

  The two enums share a name and purpose but diverge on variant names (`Update` vs `UpdateById`, `Delete` vs `DeleteById`) and coverage (the internal one knows about junction operations; the public one knows about `EventStream`). The API layer's `classify_op` in `api/mod.rs:91–113` also implements a third, simpler classification that returns `ir::OpKind` without junction awareness.
- **Why it matters:** Three classification sites produce three different type results for the same concept. Contributors adding a new operation kind must update all three independently, and the mismatch between the internal richer type and the public IR type means information is lost crossing the boundary.
- **Suggested direction:** Consolidate into one `OpKind` in `ir.rs` that covers all cases (including junctions and event streams). Remove the duplicate in `classify.rs` and the inline classifier in `api/mod.rs`. The extra variants add no complexity cost — they make the IR complete.
- **Resolution:** Single `ir::OpKind` (in `ontogen-core`) now covers all 11 cases: `List`, `GetById`, `Create`, `Update`, `Delete`, `JunctionList { child_segment }`, `JunctionAdd { child_segment }`, `JunctionRemove { child_segment }`, `CustomGet`, `CustomPost`, `EventStream`. Dropped `Copy` derive since junction variants carry `String`. Deleted the duplicate enum in `servers::classify` and the inline classifier in `api/mod.rs`. Single classifier `classify_by_name_and_params` is shared across both layers (with an `ApiFn` convenience wrapper). Generators were updated to match `Update`/`Delete` (renamed from `UpdateById`/`DeleteById`); generated output is byte-identical (snapshot tests pass).

---

### 4. Unused Enrichment Parameters (`_seaorm`, `_api`, `_scan_dirs`)  **[RESOLVED]**

- **Severity:** medium
- **Location:** `src/store/mod.rs:39`, `src/servers/mod.rs:35–36`
- **Observation:** `store::generate` accepts `_seaorm: Option<&SeaOrmOutput>` (line 39, underscore-prefixed). `servers::generate` accepts `_api: Option<&ApiOutput>` and `_scan_dirs: &[PathBuf]` (lines 35–36, both underscore-prefixed). These are documented as "enrichment, not requirements" but are simply ignored at runtime.
- **Why it matters:** The public API implies these parameters improve output (e.g., using exact column names from SeaOrmOutput instead of deriving them by convention). Users may pass them expecting better-quality generation and silently receive convention-based output instead. The `_scan_dirs` parameter on `gen_servers` is particularly confusing since `ServersConfig.api_dir` is already the scan target.
- **Suggested direction:** Either wire the parameters to actually influence generation (using `SeaOrmOutput.entity_tables` for column names in store CRUD), or remove the parameters from the public signature until the enrichment path is implemented. If the intent is forward-compatibility, document clearly that they are reserved/ignored.
- **Resolution:** Took the docstring path. The public `gen_store` and `gen_servers` rustdoc in `src/lib.rs` now plainly state that `seaorm` / `api` / `scan_dirs` are *reserved for future enrichment* and currently have no effect. Consumers no longer have a misleading "When `seaorm` is `Some`, uses structured metadata" promise that the implementation doesn't keep. The signatures stay stable so a future PR can wire the enrichment without another API break.

---

### 5. Pervasive Stale "TODO: review" Banners  **[RESOLVED]**

- **Severity:** low
- **Location:** `src/lib.rs:1`, `src/ir.rs:1`, `src/schema/model.rs:1`, `src/servers/mod.rs:1`, `src/servers/config.rs:1`, `src/servers/generators/http.rs:1`, `src/servers/generators/ipc.rs:1`, `src/servers/generators/mcp.rs:1`, `src/servers/generators/ts_client.rs:1`, `src/servers/generators/transport.rs:1`, `src/servers/generators/admin.rs:1`, `src/persistence/seaorm/gen_entity.rs:1`, `src/persistence/seaorm/gen_conversion.rs:1`, `src/persistence/markdown/*.rs:1`, `src/persistence/dto.rs:1`, `ontogen-core/Cargo.toml:1`, `ontogen-core/src/lib.rs:1`, `ontogen-core/src/model.rs:1`, plus `src/store/helpers.rs:1`
- **Observation:** Nearly every source file begins with `// TODO: review — <reason>` describing a refactor from "old crate names" or "autogenerated from extraction." These appear to be bookmarks from a consolidation refactor that was merged without a cleanup pass.
- **Why it matters:** Signals incomplete work to contributors and creates noise when searching for actual TODOs. The sheer volume makes it hard to distinguish stale housekeeping from genuine issues.
- **Suggested direction:** Do a single pass: remove banners where the code looks correct, replace with a real TODO if something still needs attention.
- **Resolution:** PR #42 removed every `// TODO: review — ...` and `<!-- TODO: review ... -->` banner from 22 Rust files and 6 documentation files (28 total). Spot-checked sites where the banner mentioned remaining work; none surfaced an actual outstanding TODO worth preserving. Future genuine TODOs should be written as `// TODO: <specific action>` so they don't get lost in another sweep.

---

### 6. `EntityTableMeta.columns` Always Empty  **[RESOLVED]**

- **Severity:** low
- **Location:** `src/persistence/seaorm/mod.rs:25`
- **Observation:** `EntityTableMeta` carries a `columns: Vec<ColumnMeta>` field, but it is always constructed as `columns: vec![]` with a `// TODO: populate from field metadata` comment. The field metadata is available from `EntityDef.fields` at the time of construction.
- **Why it matters:** `SeaOrmOutput.entity_tables` is part of the public IR. Consumers who use `columns` to derive column names (e.g., the unused `_seaorm` param in store generation) would get empty slices. The IR contract is unfulfilled.
- **Suggested direction:** Populate from `entity.fields`, mapping `FieldDef` → `ColumnMeta`. The mapping is straightforward from `FieldType`.
- **Resolution:** Added `pub(crate) fn column_meta_for(field: &FieldDef) -> Option<ColumnMeta>` in `gen_entity.rs` that mirrors the same skip rules `generate_model_column` uses (HasMany / ManyToMany return `None`; everything else maps to a `ColumnMeta` with the SeaORM-emitted column type and `is_primary_key` set from `FieldRole::Id`). `seaorm/mod.rs` now uses it via `e.fields.iter().filter_map(gen_entity::column_meta_for)` to populate `EntityTableMeta.columns`. New test `column_meta_skips_has_many_and_many_to_many` pins the rules.

---

### 7. String-typed Error Variants (Loss of Structure)

- **Severity:** low
- **Location:** `ontogen-core/src/lib.rs:29–41`
- **Observation:** All `CodegenError` variants except `ExternalTool` wrap a bare `String`: `Schema(String)`, `Persistence(String)`, `Store(String)`, etc. There is no way to programmatically distinguish, say, "file not found" from "parse error" within the `Persistence` variant.
- **Why it matters:** The layer tagging (Schema vs Persistence vs Store) is useful for error messages but insufficient for error handling. Callers that want to react differently to different failure modes (e.g., retry a format error vs surface a parse error to the user) cannot do so without string matching.
- **Suggested direction:** This is acceptable for a build-time library where errors are primarily for humans. If programmatic handling becomes needed (e.g., the planned CLI generator), each variant could carry a structured inner type. Low priority until a consumer needs it.

---

### 8. Public Visibility Leaks in `servers` Module

- **Severity:** low
- **Location:** `src/servers/mod.rs:7–13`
- **Observation:** `classify`, `config`, `generators`, `parse`, and `types` are all declared as `pub mod` in `servers/mod.rs`. The internal `parse::ApiFn`, `parse::Param`, `servers::classify::OpKind`, and generator-internal types are all transitively reachable from crate consumers. The public API surface in `lib.rs` does not intend to expose these.
- **Why it matters:** Accidental public surface — downstream crates could depend on internal types, making internal refactors breaking changes. The `servers::classify::OpKind` being public while `ir::OpKind` exists is especially confusing.
- **Suggested direction:** Change to `pub(crate) mod` for `classify`, `generators`, and `types`. Keep `config` and `parse` pub only for the specific types re-exported through `lib.rs`. Explicitly enumerate what is meant to be public.

---

### 9. `api::classify_op` Duplicates `servers::classify::classify_op`

- **Severity:** low
- **Location:** `src/api/mod.rs:91–113` vs. `src/servers/classify.rs:44–78`
- **Observation:** Two `classify_op` functions exist in different modules. The one in `api/mod.rs` is a simplified matcher returning `ir::OpKind`; the one in `servers/classify.rs` is richer (handles junction patterns) and returns `servers::classify::OpKind`. Both ultimately drive HTTP verb/route decisions for the same operations.
- **Why it matters:** Logic duplication; they can diverge silently. The api-layer classifier is less capable (no junction awareness) but feeds into the same IR used by the transport generators.
- **Suggested direction:** Once `OpKind` is consolidated (Finding 3), there should be one canonical classifier used by both layers.

---

### 10. `install_admin_layer` is a Misplaced String-Manipulation Utility

- **Severity:** low
- **Location:** `src/lib.rs:236–282`
- **Observation:** `install_admin_layer` performs regex-free string manipulation on a `nuxt.config.ts` file to inject an `extends` array entry. It uses `String::find` and manual index arithmetic to locate `extends:` and `[`, then splices text. It is categorised as `CodegenError::Client` but has nothing to do with client code generation.
- **Why it matters:** The approach is fragile: it will silently do nothing (and print a `cargo:warning`) for nuxt configs with unusual formatting. It belongs conceptually with the client/admin layer generator but sits in `lib.rs` alongside the core pipeline entry points, obscuring the API.
- **Suggested direction:** Move to `servers/generators/admin.rs` or a dedicated `clients/nuxt.rs`. Consider using a simple TOML/JSON manipulation approach or documenting the exact config format constraints.

---

### 11. `cruet` Listed as Dependency but Not Directly Imported

- **Severity:** low
- **Location:** `Cargo.toml:21`
- **Observation:** `cruet = "1.0"` is listed as a direct dependency of the main crate, but grepping the source shows no `use cruet::` or `extern crate cruet` usage. All naming utilities (`to_snake_case`, `to_pascal_case`, `pluralize`) are in `ontogen-core/src/naming.rs` and implemented directly.
- **Why it matters:** Unused direct dependency adds to compile time, supply-chain surface, and dependency audit scope. It may be a leftover from before the naming utilities were extracted to `ontogen-core`.
- **Suggested direction:** Verify with `cargo +nightly udeps` or `cargo machete`. If unused, remove.

---

### 12. Schema Import Path Duplicated Across Config Structs

- **Severity:** low
- **Location:** `src/lib.rs:156–186` (`StoreConfig::schema_module_path`, `ApiConfig::schema_module_path`)
- **Observation:** Both `StoreConfig` and `ApiConfig` carry an identical `schema_module_path: String` field with the same purpose and same default value (`"crate::schema"`). A build.rs must set this consistently in both places.
- **Why it matters:** Minor ergonomics: easy to forget to set one. Not a runtime risk because the defaults match.
- **Suggested direction:** Either document a canonical default prominently, or extract into a shared `PipelineConfig` that both configs embed or reference.

---

## Strengths

**Clean three-crate workspace layout.** `ontogen-core` has zero dependencies and is purely a type library. `ontogen-macros` is correctly isolated as a proc-macro crate. The main crate depends on both without creating any cycles. The dependency arrow is strictly one-directional.

**Genuinely composable generator functions.** Each stage is a standalone function accepting typed inputs and returning typed IR. Callers can run only the stages they need, pass `None` for optional enrichment, and chain outputs without wrapper types. The "enrichment not requirements" design in the IR is well-executed in concept.

**Scan-and-merge architecture for API and server layers.** Generated modules and hand-written modules are normalized into the same IR types (`ApiModule`, `ApiFnMeta`) before transport generators consume them. Adding a custom endpoint doesn't require touching generated files, and generated code never overwrites hand-written code.

**Lifecycle hook scaffold pattern.** Generating hook files exactly once (never overwriting) and always calling them from generated CRUD is an elegant way to give developers stable extension points without coupling to a framework's plugin system.

**`write_if_changed` preventing rebuild loops.** Every generated file is written only when content actually changes. This is non-trivial correctness for a build-time library and prevents Tauri/cargo from re-triggering builds unnecessarily.

**`#![forbid(unsafe_code)]` across all three crates.** Enforced at the crate attribute level, not just as a clippy lint, making it compiler-enforced.

**Thorough doc comments on IR types and public API.** `ir.rs`, `model.rs`, and the public functions in `lib.rs` all carry doc comments that explain intent, not just types. The module-level docs in `store/mod.rs` and `api/mod.rs` give an accurate summary of what's in each file.

**Snapshot tests for code generation output.** Using `insta` for snapshot-based testing of generated Rust code catches unintentional output regressions without requiring a downstream project to be compiled.

**Well-documented architecture intent.** `docs/proposal.md` explains the "why" of the three-crate split, the "why" of the IR design, and the pain points of the prior approach. This is rare and valuable.

---

## Open Questions

1. **Is `cruet` intentionally kept as a future dependency or is it a leftover?** Its presence in `Cargo.toml` without any visible import is unusual.

2. **What is the intended consumer of `ServersOutput`?** The type is defined and returned, but always empty. Is the plan for clients to consume it directly (requiring it to be populated), or will they always re-scan disk?

3. **Why is `_seaorm` accepted but ignored in `gen_store`?** The proposal docs suggest exact column names should come from `SeaOrmOutput`. Is this planned for a specific phase, or has the design moved toward pure convention-based derivation?

4. **Does the `strip_wikilink` dependency surface to all consumers, or only markdown-persistence projects?** README notes that generated store code imports `strip_wikilink` stubs unconditionally. If a project uses SeaORM persistence only, must it still provide no-op stubs?

5. **What is `schema_entities: Vec::new()` in `servers/mod.rs:69`?** The internal `Config` struct accepts `schema_entities` but it is always initialized to empty when converting from `ServersConfig`. Is this field vestigial?

6. **Is there a reason `api::classify_op` doesn't use `servers::classify::classify_op`?** They solve the same problem with different implementations.

7. **The `quality-check` justfile recipe delegates to `scripts/quality_check.sh` — what does that script do?** It's not visible in the repo root's tracked files; it may be template-provided infrastructure.
