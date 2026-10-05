# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.9.0] - 2026-10-05

### ⚠ BREAKING CHANGES

- vaults are OKF 0.2 bundles by default (#194)

- OKF index files and provenance stamps, configured once at build time (#196)

- one id strategy on both backends, typed create errors, has_many drops, id order, i64 integer columns (#198)
  - The id strategy moves from `MarkdownIoOptions`/`MarkdownIoConfig` to the store: call `Pipeline::store_id_strategy(IdStrategy::SlugFromField("title".into()))` or set `StoreConfig { id_strategy, .. }`; without it the strategy is `Provided`. `SlugFromField` needs a `String` field on every entity, on SeaORM too. Consumer `AppError` must declare `{Entity}IdRequired(String)` and `{Entity}AlreadyExists(String)` for every entity, and `{Child}ParentRequired(String)` for a has_many child whose foreign key is not `Option`. `markdown_store::VaultHandle::new` takes `(root, layout)`, each create takes `&IdStrategy`, and a missing id is `markdown_store::Error::IdRequired` (was `InvalidId`). SeaORM create: a blank id is derived by the strategy, an id breaking the shared id rule is refused, and a duplicate is `{Entity}AlreadyExists` (was `DbError`). Markdown lists are in id byte order. SeaORM model fields for integer primitives under `OptionEnum`/`Other` are `i64` (were `i32`); `from_model`/`to_active_model` return `Result<_, AppError>`; generated SeaORM stores depend on `ontogen-core`; `load_junction_ids` must read `ORDER BY rowid`. Data migration before the first start of the new binary: on SQLite run `UPDATE <table> SET <column> = <column> + 4294967296 WHERE <column> < 0;` once per `u32` or `Option<u32>` column; on Postgres first `ALTER COLUMN ... TYPE bigint` for every `u8`/`u16`/`u32`/`i8`/`i16` column, then that `UPDATE` for `u32` columns; on MySQL widen the `u32` columns to `BIGINT`, then the `UPDATE`.

- portable ASCII ids, exact markdown lookups, per-entity id strategies over a required default, SQLite-only SeaORM marked, u64 kept (#199)
  - Ids being created must match `[a-z0-9._~-]`, be at most 200 bytes, not be `index`/`log`, not be a Windows device name (`con`, `prn`, `aux`, `nul`, `com0`-`com9`, `lpt0`-`lpt9`, whole or before the first `.`), and not start or end with `.`. Existing rows stay reachable, but such ids cannot be created again, so rename them before relying on create. Slugs of titles with diacritics, non-ASCII letters or over 190 bytes change. Markdown lookups and relation ids that only matched through filesystem case or Unicode folding are now NotFound, as are device-name lookups on Windows. An entity directory that is a device name fails the markdown build, and `markdown_store::layout::validate_segment` refuses it. `Pipeline::store_id_strategy(..)` is required whenever a store stage runs, and `IdStrategy` no longer implements `Default` (use `IdStrategy::Provided` for the old behaviour); `EntityDef` gains `id_strategy`. `markdown_store::Error::InvalidId` gains `create_rule`. A cross-entity `has_many` fails the build. A `u64` field is `u64` in DTOs and frontmatter (was `i64`), and values above `i64::MAX` are refused on both backends.

- generated CRUD routes speak JSON:API and the TS transport is their client (#200)
  - `gen_servers` and `gen_clients` take `entities: &[EntityDef]` as their first parameter; `ClientsConfig::schema_entities` is removed; `ServersConfig` gains `error_source_dir`; `ontogen::servers::Config` and `ontogen::servers::generate_transport` are no longer public. The build now fails on an entity struct with shape-changing serde attributes, a relation target that is not an entity, an id field that is not `String`, and an attribute or relationship name that is not a legal JSON:API member name. Generated servers depend on `ontogen-jsonapi`, `ontogen-core` and `serde_json` and must be mounted at the root. The HTTP wire is JSON:API: `application/vnd.api+json`, `{data}` documents, `PATCH` instead of `PUT`, `page[offset]`/`page[limit]` instead of `offset`/`limit`, unknown query parameters are `400`, a related id that does not exist is `404 related_resource_not_found` (a vault used to accept it and write a dangling wikilink), and errors are `errors[]` documents. Generated TS transports no longer emit `httpPut` for entity-backed modules and throw `JsonApiError`. See `reference/upgrading`.

- custom, junction and event routes speak JSON:API (#201)
  - custom ops respond `{"jsonapi":…,"meta":{"result":…}}` instead of the bare value. Custom `POST` bodies are `{"meta":{"args":{…}}}` keyed by Rust parameter name, and a custom `POST` accepts no query parameters (`Option` arguments moved from the query string into `meta.args`). Custom `GET` optional arguments are `opArg[name]`; bare names are `400 invalid_query_parameter`. In a module with no entity behind it, `update` is `PATCH` (was `PUT`), `create` answers `200` with `meta.result` (was `201`), and list paging is `opArg[limit]` and `opArg[offset]` (was `limit` and `offset`), read with the `page[…]` digit grammar. Junction ops read `meta.args`; a junction list answers `meta.result`, and add and remove answer `204`, scoped or not (scoped add and remove returned the bare value). SSE `data:` is a link-free resource object for an entity item and `{"meta":{"result":…}}` otherwise. A subscribe failure with an `AppError` answers that error's status instead of `500`. In builds that generate an HTTP server or client, these are a `CodegenError`: a CRUD or junction op in a singleton module; an `*Input` parameter on a `GET` or `DELETE` op; a `get_by_id`, `create`, `update` or `delete` in a module with no entity that takes arguments beyond its route's; and an `Option` argument of a bodyless op whose type has generic arguments, is a tuple or is a schema entity. Scoped non-resource lists call the store's paged `list` and `count` instead of slicing in memory, and a scoped junction list on a paginated surface answers `{items,total,limit,offset}`. `HttpTauriIpcSplit` calls the action-style scoped junction routes (`POST …/add-tag` with both arguments in `meta.args`) when a `projectId` is given. The generated `xCount()` method of a paginated module is removed from the `Transport` interface, both TypeScript HTTP clients and the IPC transport. Junction `xAddY` and `xRemoveY` are declared `Promise<null>` on `Transport` and resolve `null` over IPC too. The TypeScript `httpPut` helper is removed, `HttpTs` custom-op methods call the server's kebab-case paths, and `HttpTs` paginated lists, junction lists included, take `limit?` and `offset?` and return `PaginatedResult<T>` (was the bare first page). `ServersConfig.rustfmt_edition` is removed.

- list filters are the JSON:API filter family (#202)
  - a filtered list takes `filter[name]` instead of `name`, `page[offset]`/`page[limit]` (or `opArg[...]` with no entity) instead of `limit`/`offset`, and answers JSON:API instead of the bare list or `PaginatedResult`.
  - a hand-written `list` or `count` in a module named after an entity replaces the generated one; a paginated module's hand-written `list` needs a `count` taking the same filter. Any other name the generated module defines, written again by hand, fails the build.
  - `Pipeline` scans the transports' `api_dir`s for the api stage, and `api_scan_dirs` adds to them instead of replacing them; a missing `api_scan_dirs` entry is an error.
  - a `list` or `count` defined in both the generated and a hand-written file of one API directory (the api stage did not scan it, or the hand-written fn is stateless) is a build error naming that rule; `gen_api` no longer prints scan skip warnings.
  - a list's `*Query` struct must be reachable through `types_import_path`; re-export it from the schema module, e.g. `pub use crate::api::v1::task::ListTasksQuery;`.
  - on HTTP, a list taking two `*Query` structs, one not by value or `&`, a bare filter one query value cannot carry, or an `*Input` parameter is a codegen error.
  - every MCP tool refuses an argument its schema does not list (`Unknown argument: ...`); the list tool refuses a malformed `*Query` argument (`Invalid filter: ...`) instead of listing everything, and its input schema lists the bare filters, whose types need `JsonSchema`.
  - an MCP custom op taking an `*Input` beside other arguments takes it under its parameter name; a required string argument of the wrong type is `Invalid parameter`, not `Missing`.
  - the TS IPC client sends a list's bare filters under their own names, not as `query`; a list whose filter struct has a required field takes a required `query`; a filter struct's `Option` fields are optional in TS; `HttpTs` imports its bindings file.
  - generated IPC commands name their own bindings `ontogen_state`/`ontogen_store`; an argument named `query` beside a `*Query` struct, `limit`/`offset` on a paged junction list, `channel` on an event op, or the route prefix parameter's name on a scoped op is a build error when IPC is generated; scoped MCP refuses an op argument named like the route prefix parameter the same way.
  - `ontogen_jsonapi::QuerySpec` has a `filter_fields` field and is no longer `PartialEq`/`Eq`; `ontogen_ts::EmitConfig` has a `deserialize_only` field; the public MCP schema structs are named `Ontogen{Module}{Fn}Input` and `Ontogen{Module}{Fn}Filter`.

- relationships are served at their JSON:API endpoints (#203)
  - junction ops of a resource module are served at the relationship route `/api/{type}/{id}/relationships/{rel}` and the related route `/api/{type}/{id}/{rel}`, and their old nested routes are removed.
  - relationship objects carry `links`, and junction relationships appear in resource objects with links only.
  - a `list_X` without `add_Y` or `remove_Y` is a custom GET on HTTP, IPC, MCP and TS and returns its plain `Vec`, unpaged.
  - scoped junction ops outside a resource module take their unscoped route shape under the prefix, and action routes like the scoped `/api/projects/{project_id}/tasks/add-tag` are removed.
  - a consumer route shaped like `/api/{type}/{id}/{x}` or like `/api/{type}/{id}/relationships/{x}` conflicts with the generated relationship routes, and Axum panics when the routers are merged.
  - a junction relationship name that collides with a field or relationship (`list_tags` beside `tags`) is a build error.
  - an `add_Y` or `remove_Y` without `list_X`, a `list_X` returning neither `Vec<Target>` nor `Vec<String>`, a non-string junction id, and junction ops in a module without `get_by_id` are build errors.
  - a relationship target whose module serves no route for `get_by_id` is a build error.
  - under a route prefix, a resource module is a build error when its `list`, `create` or `update` takes a store and the module's `get_by_id` does not, or the reverse, and when an unscoped module's `get_by_id` has junction ops or relationship targets that take a store.
  - under a route prefix, a `create` or `update` that takes no store is a build error when a type it links to has a `get_by_id` that takes a store.
  - under a route prefix, `HttpRouteMeta.handler_name` of every route served under the prefix names its scoped handler, as in the name `task_get_by_id_scoped` where it was `task_get_by_id`.

- lists and gets answer include with compound documents (#205)
  - A resource module that serves no `get_by_id` no longer emits a resource `links.self`, a `Location` header on `201`, or top-level `links` on create and update documents, since no route serves the item path for it. Read the id from `data.id`, or add a module `get_by_id` to serve the links again. List documents keep their top-level links.

- lists take an order and every transport sorts (#206)
  - the generated store's `list_{plural}` takes the order as its first argument, typed `&[OrderBy<{Entity}SortField>]`. Pass an empty slice, `&[]`, for the default id order.
  - the generated API `list` takes the order after the store, so a hand-written caller adds `&[]` before any page arguments.
  - the stage functions `gen_store`, `gen_servers` and the clients stage `gen_clients` take `&SchemaOutput` where they took the entities, and the field `ClientsConfig::schema_enums` is removed. Pass the parsed schema, or `&SchemaOutput::default()` when there is none.
  - every crate that holds a generated store depends on the `ontogen-core` crate, markdown stores included.
  - `CrudOp` has a `Count` variant, and `StoreOutput` records each entity's `count_{plural}` and the order of its list.
  - both stores refuse a create or update that writes NaN to a float field, with the backend's catch-all error.
  - an order parameter that is not the last before the page of a resource module's `list`, a second one, one naming another entity's sort fields, or one in a module with no entity is a build error, from the servers and the clients stage alike.
  - an entity without an `#[ontology(id)]` field fails the store stage, since every order ends with the id. Mark the field.
  - a generated HTTP list that takes an order answers a valid `sort` by sorting. Before, every list answered 400 with the code `invalid_sort_field`, and a list that takes no order still does.
  - a list that takes an order may not have a bare filter named `sort`, which is the IPC and MCP key for its sort keys, and over MCP a filter struct field named `sort` cannot be set.
  - the TypeScript `toQueryString` writes an array as one parameter with comma-joined items and skips an empty one.
  - a sorted list's TS method takes `options` last, so a bare filter whose TS name is `options` fails the clients stage, as does any bare filter or route prefix parameter whose TS name collides with another parameter of the method.
  - with an `HttpTauriIpcSplit` client the clients stage applies the IPC wire-key rules itself, so a bare filter named `sort` on a sorted list, `query` beside a filter struct, and the other IPC collisions fail the build even when no IPC server is generated. Both stages compare arguments by the camelCased invoke key they travel under, so `sort_` collides with `sort`.
  - the clients stage reports its errors as the variant named `CodegenError::Client` where it used `CodegenError::Server`, and the message prefix reads `client codegen error:`.
  - the SeaORM store's create also refuses NaN in a skipped float field, `#[ontology(skip)]`, which it stores in a column.

- server error codes never collide with store variants (#208)
  - ontogen-jsonapi renames the ErrorCode variants RelatedResourceNotFound and RelationshipNotFound to NoSuchRelatedResource and NoSuchRelationship, whose codes are no_such_related_resource and no_such_relationship, and ErrorCode::ALL is a &'static [ErrorCode].

- generated code survives colliding names or refuses them clearly (#210)
  - the generated HTTP file no longer exports PaginatedResult. Its page type is the private OntogenPaginatedResult; code that named PaginatedResult from the HTTP module declares its own type.
  - an MCP tool reads an argument named after a Rust keyword under serde's name for it, as its input schema already said: send type, not r#type.
  - a subscribeX method's args members are named by Tauri's key, so an event op argument _kind is args.kind where it was args.Kind.
  - some names that built with 0.8.0 are now refused at build time, among them entities named Ontogen..., OrderBy, Default, Send, Sync or Clone, and fields or arguments named ontogen_.... The upgrading guide lists them; rename the item as the error suggests.

- wire and store refuse bad writes alike and write nothing (#209)
  - the consumer's SeaORM sync_junction takes the connection to write on after &self (conn: &C, generic over sea_orm's ConnectionTrait) and must run every statement on it. Generated create_* and update_* make all their writes in one transaction and pass it in, so a write that fails part-way leaves nothing written.
  - both store backends refuse a many_to_many id that names no record with {Target}NotFound, before anything is written. The markdown store used to write the dangling link, and SeaORM wrote the row before the junction's foreign key failed. Create the targets first. An update checks only the ids it adds, not those already in the record's list, so a record holding a dangling id still gains and loses links.
  - a consumer AppError declares {Child}ParentCycle(String) for the child of a has_many, which is the declaring entity (Task with subtasks declares TaskParentCycle), or the generated store does not compile. A has_many list naming the record itself is refused with it instead of making the record its own parent.
  - with a servers or clients stage, a hand-written list or count that replaces a generated one must be in a file those stages read, in their api_dir or a directory directly inside it. One found only through api_scan_dirs now fails the build; move it into a transport's api_dir, or rename it to keep the generated fn.
  - a has_many foreign_key must name the declaring entity's belongs_to to itself, typed String or Option<String>. A missing foreign_key, a plain field, a belongs_to to another entity or a field not typed String or Option<String> now fails the build with a CodegenError.
  - a markdown list, count or has_many list no longer returns a vault file whose stem is not UTF-8 or fails the lookup check (layout::validate_lookup_id), such as a:b.md, a\b.md, draft..md or draft .md. Such files were listed although no get, update or delete could reach them. Rename them to ids the create rule accepts.
  - markdown_store::IdStrategy::Uuid exists only with markdown-store's uuid feature. A markdown store using it without the feature used to build and fail every create without an id; it now fails to compile. Enable features = ["uuid"] on markdown-store.

### Added

- vaults are OKF 0.2 bundles by default (#194) **(breaking)**
- OKF index files and provenance stamps, configured once at build time (#196) **(breaking)**
- the ontogen-jsonapi runtime crate (E0004 phase 1a, runtime) (#197)
- one id strategy on both backends, typed create errors, has_many drops, id order, i64 integer columns (#198) **(breaking)**
- portable ASCII ids, exact markdown lookups, per-entity id strategies over a required default, SQLite-only SeaORM marked, u64 kept (#199) **(breaking)**
- generated CRUD routes speak JSON:API and the TS transport is their client (#200) **(breaking)**
- custom, junction and event routes speak JSON:API (#201) **(breaking)**
- list filters are the JSON:API filter family (#202) **(breaking)**
- relationships are served at their JSON:API endpoints (#203) **(breaking)**
- lists and gets answer include with compound documents (#205) **(breaking)**
- lists take an order and every transport sorts (#206) **(breaking)**

### Fixed

- a paginated SQL list orders before it takes its page (#178)
- keep the admin registry's indentation, and regenerate the examples (#179)
- server error codes never collide with store variants (#208) **(breaking)**
- generated code survives colliding names or refuses them clearly (#210) **(breaking)**
- wire and store refuse bad writes alike and write nothing (#209) **(breaking)**



## [0.8.0] - 2026-09-28

### Added

- event ops take parameters, carry a sequence, and report lag ([#184](https://github.com/sksizer/rust-ontogen/pull/184))
- generated TS subscriptions resume and report lag ([#185](https://github.com/sksizer/rust-ontogen/pull/185))



## [0.7.1] - 2026-09-23

### Added

- the registry path is an option
- relation pickers, enum-select guard, detail-link hook, 0.2.0
- a paginated list may filter, when its count filters alike
- a docs stage emits the data-model reference and JSON Schema

### Changed

- a count walks the directory instead of parsing every record

### Fixed

- inverse relation tables read a paginated target page by page
- the page goes where the transport's list signature puts it
- the picker's input handler is a method
- an emptied optional field clears the column on edit
- page args read the registry's listHasQuery flag and Enter on a closed picker does not submit the form
- custom fn params in Rust declaration order on every transport
- the enum type column separates values with commas
- escape pipes in the markdown table cells
- map undedicated integer primitives to their wire type
- match the crate-prefixed tags release-plz creates

## [0.7.0] - 2026-09-22

### Added

- a ClientsConfig built from its required inputs, with the rest defaulted

### Fixed

- emit deserialize_with for nullable Update DTO fields
- handle booleans, paginated envelopes, and null/undefined form values correctly

### merge

- main into api-surfaces
- main into pagination-pushdown
- main into schema-enums
- main into state-scoped-count



## [0.6.7] - 2026-08-17




## [0.6.6] - 2026-08-16

### Changed

- render every TypeScript type through one model



## [0.6.5] - 2026-08-16

### Fixed

- correct rust_type_to_ts precedence, maps, and spacing



## [0.6.4] - 2026-08-16



## [0.6.3] - 2026-08-16



## [0.6.2] - 2026-08-16



## [0.6.1] - 2026-08-08

### Fixed

- sort schema-dir scan for deterministic entity order



## [0.6.0] - 2026-08-08

### ⚠ BREAKING CHANGES

- **The in-process TypeScript formatter is gone.** The `biome` feature added in
  0.5.0 is removed, along with every built-in formatting pass. Generated
  TypeScript is now emitted unformatted by default. Set
  `ClientsConfig::ts_formatter` to `TsFormatter::Command(...)` to shell out to a
  formatter, or `TsFormatter::custom(...)` / `custom_with(...)` to format
  in-process with a library of your choice and pick an error policy.
  `ts_formatter` is a required field, so build scripts must add it.

  *(This landed as `fix(formatter)!: replace in-process biome with a consumer
  formatter hook` and was omitted from the generated changelog — recorded here
  after the fact. The `cliff.toml` template now surfaces breaking commits
  automatically.)*

### Added

- path-aware TS format hook with configurable error policy **(breaking)**
- #[ontogen::http::get] override, and fix POST dropping optional params



## [0.5.0] - 2026-08-05

### Added

- in-process biome TypeScript formatter, feature-gated ([#120](https://github.com/sksizer/rust-ontogen/pull/120))
  — **removed again in 0.6.0**; see that entry.



## [0.4.0] - 2026-07-14

### Added

- emit axum 0.8 brace-style route params



## [0.3.1] - 2026-07-12



## [0.3.0] - 2026-07-12

### Added

- runtime markdown vault crate with worked API examples
- byte-stable verbatim render for untouched documents
- Backend enum and markdown IR (additive)
- markdown CRUD emitter, golden conformance, and a CI-executed pilot
- iron-log-md - iron-log's schema on the markdown backend
- tasks-tracker - a planning vault over HTTP and MCP
- notes-kb - the vault as a graph

### Changed

- lift SeaORM emission behind a pub(crate) StoreBackend seam
- wikilink stripping becomes a backend policy; SeaORM goes passthrough
- retarget markdown generation onto the markdown-store runtime

### Fixed

- make bindings append idempotent across reruns
- invoke prettier via pnpm exec rooted at output dir
- keep path segment and numeric typing on custom GETs with query params
- repair iron-log build and resync generated output
- address adversarial review — atomic derived create, maintained YAML stack
- no-op read-modify-write skips the write entirely
- apply Gate G1 findings to the golden spec
- keep path segment and numeric typing on custom GETs with query params

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


