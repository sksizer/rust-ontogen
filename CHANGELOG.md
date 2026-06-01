# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.1] - 2026-06-01

### Added

- publish ontogen-macros and re-export OntologyEntity derive
- make schema module path configurable in StoreConfig and ApiConfig
- add optional limit/offset to generated list_*() methods
- warn when #[ontology(...)] attrs are malformed
- add Pipeline builder for ergonomic build.rs orchestration
- add FieldType variants for f32 / f64
- expose schema_entities on ServersConfig; migrate iron-log to Pipeline
- emit cargo:warning for skipped pub fns in api modules (OF-001) Previously the parser silently dropped any `pub fn` in an api source file that didn't match the configured state_type / store_type substring rule, plus self-receiver fns and zero-param fns. There was no signal that any of those were missing from the generated output. Surface them as SkipRecord values flowing through ScanResult: parse_api_module: Option<ApiModule> -> ModuleParseResult scan_api_dir: Vec<ApiModule> -> ScanResult Three SkipReason variants cover the three drop paths: - FirstParamMismatch { first_param_ty, state_type, store_type } - SelfReceiver - NoParams Both lib call sites (gen_api in src/api/mod.rs, generate_transport in src/servers/mod.rs) print one cargo:warning= line per skip record via SkipRecord's Display impl. The parse module is pub(crate), so these signature changes don't affect the crate's public api surface. The 8 in-tree scan_api_dir callers in servers/tests.rs append `.modules` to preserve the prior shape. Verifies the OF-005 acceptance table empirically with three new tests (test_of005_table_accepted_rows / _rejected_rows / _store_substring_false_positive) so the docs page that follows can't drift from runtime behaviour. Closes the implementation half of OF-001. OF-005 docs page lands next.
- add file-level `// ontogen:skip` marker to opt out of api scanning (OF-012)
- first-class singleton modules (OF-002, OF-004)
- emit cargo:warning for TS bindings fallback to Record<string, unknown> (OF-006)
- add #[ontogen::stateless] for pure utility fns (OF-007)
- per-function command-name override (OF-003)
- EntityDef→TS emitter + specta side-car for long-tail types (OF-014 spike)
- drop param-import substring gate (OF-017)
- AST-aware get_* classifier and is_read_op (OF-016)
- wire iron-log for the OF-014 side-car gotchas (OF-019)
- scaffold crate with public API skeleton
- per-type emission for primitives, containers, and smart-pointer peel
- emission for named structs and enums
- rename engine mirroring serde's eight rename_all modes
- serde attribute extraction (rename / rename_all / skip)
- apply serde renames in emit_struct/emit_enum + property tests
- type collection + use-resolution + external-types + ordering
- top-level emit composition + ts_opaque/ts_name attrs (AC-8/9/10)
- replace ts_sidecar emission with ontogen-ts AST walker (AC-11)
- pool_extra_roots for workspace-sibling type discovery
- include entity field types in long-tail root set
- emit #[serde(default)] fields as TS-optional
- add #[ontogen::post] to force POST classification
- add EmitConfig::quote_style for configurable TS string-literal quotes
- flip zero-param classifier default to CustomPost; opt back into CustomGet via known-read prefix allowlist
- map Rust std string-like types to TS `string`
- classify wider Rust int set into I32/I64 (and their Option<...>) The parser only matched i32 / i64 / u64 against the typed FieldType variants. Anything else (u8 / u16 / u32 / u128 / usize / i8 / i16 / i128 / isize) fell through to FieldType::Other(...) — for bare types — or FieldType::OptionEnum(...) — for Option<...> wrappers (since the catch-all in the Option arm misclassifies any unknown inner as an enum). Both eventually emit the raw Rust ident into bindings.ts, which the consuming TS sees as an unresolved type name. Fold u8/u16/u32 into I32 (they fit), and u64/u128/usize/isize/i128 into I64, following the established u64 → i64 convention ('SQLite has no unsigned integers'). Same treatment for the Option<...> arm. The previous commit's ts_bindings.rs fallback in the Other(...) arm stays as defense in depth.
- cut 0.2.0

### Changed

- reuse ontogen_core::naming::to_snake_case
- cache rustfmt edition detection in a OnceLock
- use ItemStruct directly, avoid DeriveInput re-parse
- use unwrap_or(ch) for char.to_lowercase().next()
- move install_admin_layer to a dedicated admin module
- tighten submodule visibility to pub(crate)
- expose DEFAULT_SCHEMA_MODULE_PATH as canonical default
- relocate ontogen-core/ + ontogen-macros/ under crates/
- split client SDK codegen out of the servers module
- namespace post under http and replace force_post bool with ForcedMethod enum
- impl Default for ApiFn and Param

### Fixed

- collapse nested if into match guard for clippy 1.95
- parameterize set_parent SQL to eliminate injection risk
- propagate unknown-relation-kind as error instead of panic
- validate table/directory/type_name/prefix as identifiers
- add missing pagination field to Config initializer
- update StoreConfig and snapshot after pagination merge
- populate ServersOutput, StoreMethodMeta.params, and consolidate OpKind
- walk syn::Type AST when collecting type imports
- drive handler arg forwarding from syn::Type AST instead of name heuristics
- AST-ify param_to_owned_type for unsized-DST owned forms (OF-013)
- normalize OF-015-pr-1 to schema (drop `epic`, set ready, add impact/complexity/created)
- mark OF-015-pr-1 as closed/done (shipped in #55)
- mark OF-015 PR-2..PR-6 as closed/done
- resolve single-segment closure edges to nested-module keys
- resolve closure edges through module use imports, not blind terminal match
- import-aware long-tail root resolution
- borrow constructed Store; bind args under route_prefix; add pool_exclude_paths Three independent fixes surfaced while migrating an external consumer (a real-world SeaORM-based Tauri+Nuxt project with project-scoped route_prefix) to the new clients/ontogen-ts pipeline. None of these showed up against the iron-log example because that example is too narrow to exercise the relevant paths. ## 1. Store passed by value, not reference (IPC + HTTP) The IPC generator and the HTTP generator's no-prefix path emitted `{svc}::{op}(store, ...)` for store-based handlers, but `gen_api` emits CRUD fns taking `&Store`. The handlers construct an owned `Store` via the configured accessor (`state.store_for(...)?` / `state.store().await?`) and pass it on; the constructed value is owned, so the forward must borrow. Three emit sites changed from `store` to `&store`: - servers/generators/ipc.rs:195 (CRUD handlers) - servers/generators/ipc.rs:462 (custom / paginated handlers) - servers/generators/ipc.rs:520 (paginated junction handlers) - servers/generators/http.rs:186 (no-prefix CRUD) - servers/generators/http.rs:654 (no-prefix custom) The HTTP scoped path (used when `route_prefix` is set) was already borrowing correctly. MCP was already borrowing. Updated the three `.contains(...)` assertions in `servers::tests` that pinned the old (incorrect) string output. ## 2. MCP `args` not in scope under route_prefix The non-paginated `OpKind::List` branch in `servers/generators/mcp.rs` named the closure argument `_args` when `extraction.is_empty()`, but the route_prefix prefix/store-construction snippets reference `args.get("project_id")` unconditionally. For a store-based list under `route_prefix` with no other extracted params, the emitted body referenced an `args` binding that did not exist. Extended the condition to also require `config.route_prefix.is_none()` before downgrading to `_args` — when a prefix is configured, the body always reads `args` so the param must be bound. ## 3. `pool_exclude_paths` on `ClientsConfig` (ontogen-ts pool filter) `gen_seaorm` emits a SeaORM `Relation` enum per entity by convention. When `gen_clients`/`ontogen-ts` later builds its type pool from `CARGO_MANIFEST_DIR/src`, those per-entity `Relation` enums end up in the pool alongside the consuming crate's own `Relation` type. The long-tail resolver then reports the bare name `Relation` as `Ambiguous` (one match per generated entity) and `gen_clients` aborts before iterating the configured client generators. Added `pool_exclude_paths: Vec<PathBuf>` to `ClientsConfig` and its internal `clients::config::Config`. Each entry is rooted at `CARGO_MANIFEST_DIR` (mirroring `pool_extra_roots`); after the main+extras pool is assembled, every pool entry whose module-path segments lie under one of the exclude prefixes is dropped. `Pipeline::build` auto-populates this from the `seaorm()` stage's `entity_output` so consumers using the builder get the filter for free. Direct callers (those constructing `ClientsConfig` without `Pipeline`) set it explicitly. Both call sites guard against adding a duplicate exclude. ## Tests All 221 lib tests still pass. `servers::tests` constructors and the `ts_bindings` test helper gained the new field (`pool_exclude_paths: Vec::new()`). Existing `route_prefix` test helpers exercise the MCP `args` fix indirectly; the store-pass fix is covered by `test_ipc_handler_arg_forwarding_matrix` and `test_of013_unsized_dst_owned_form_in_ipc` whose forwarding assertions were corrected to `&store`.
- narrow transport[listMethod] via 'as unknown as ...' The Transport interface is the union of every entity's CRUD methods plus the event-subscription methods (onGraphUpdated, onEntityChanged). Directly casting transport[method] to the list-call signature fails under TypeScript's overlap check because event-subscription returns Promise<() => void>, which does not overlap with the paginated Promise<{ items, total }> shape useAdminEntity wants. TypeScript's own error message suggests the fix: route through 'unknown' first. This is a runtime-safe narrowing because the caller already constrained the value via the AdminEntityConfig contract. No behavioural change; the runtime call is identical.
- map Rust u-types to TS `number` The schema-known emitter's `field_to_ts` had typed arms for I32/I64/F32/F64 and their Option<...> forms, but anything else fell through to `FieldType::Other(name) => name.clone()` — shipping bare Rust idents like `u32` straight into bindings.ts. A real consumer's `step_index: u32` then fails TS check with 'Cannot find name u32'. Extend the `Other(name)` arm to map the wider Rust primitive set (u8/u16/u32/u128/usize and the i-/f- siblings) to `number`, and the `Option<...>` wrapper around any of those to `number | null`. Same treatment for bool. Mirrors how primitive_ts handles the same set elsewhere in ontogen-ts, just at the schema-known emission site that predates that table.
- emit junction routes in sorted order, not HashMap order

## [0.1.0] - 2026-04-07

### Added

- implement ontogen build-time code generator for ontology-driven applications
- add iron-log example project demonstrating full ontogen pipeline
- add nuxt admin layer and per-field registry generation
- restore as full project from template-tauri-nuxt
- add i64, bool, and option variants to field type handling
- add junction operations, naming improvements, and scan-mode fixes
- client generators in public API, transport import fixes
- cruet integration and entity-first naming convention
- query params threading and first-class pagination

### Changed

- extract shared types and utilities into ontogen-core crate
- use write-if-changed pattern and update schema for new entity model
- format generated files in memory before writing
- extract shared types to @ontogen/admin-types and remove project-scoping

### Fixed

- add full template-tauri-nuxt project structure to iron-log example
- resolve CI formatting and clippy failures
- resolve prettier config lookup and clean up generated output
- generate unscoped handlers for store-based modules without route_prefix
- resolve clippy warnings from newer toolchain
- junction naming consistency across transports


