# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.7.0] - 2026-10-05

### ⚠ BREAKING CHANGES

- vaults are OKF 0.2 bundles by default (#194)

- OKF index files and provenance stamps, configured once at build time (#196)

- one id strategy on both backends, typed create errors, has_many drops, id order, i64 integer columns (#198)
  - The id strategy moves from `MarkdownIoOptions`/`MarkdownIoConfig` to the store: call `Pipeline::store_id_strategy(IdStrategy::SlugFromField("title".into()))` or set `StoreConfig { id_strategy, .. }`; without it the strategy is `Provided`. `SlugFromField` needs a `String` field on every entity, on SeaORM too. Consumer `AppError` must declare `{Entity}IdRequired(String)` and `{Entity}AlreadyExists(String)` for every entity, and `{Child}ParentRequired(String)` for a has_many child whose foreign key is not `Option`. `markdown_store::VaultHandle::new` takes `(root, layout)`, each create takes `&IdStrategy`, and a missing id is `markdown_store::Error::IdRequired` (was `InvalidId`). SeaORM create: a blank id is derived by the strategy, an id breaking the shared id rule is refused, and a duplicate is `{Entity}AlreadyExists` (was `DbError`). Markdown lists are in id byte order. SeaORM model fields for integer primitives under `OptionEnum`/`Other` are `i64` (were `i32`); `from_model`/`to_active_model` return `Result<_, AppError>`; generated SeaORM stores depend on `ontogen-core`; `load_junction_ids` must read `ORDER BY rowid`. Data migration before the first start of the new binary: on SQLite run `UPDATE <table> SET <column> = <column> + 4294967296 WHERE <column> < 0;` once per `u32` or `Option<u32>` column; on Postgres first `ALTER COLUMN ... TYPE bigint` for every `u8`/`u16`/`u32`/`i8`/`i16` column, then that `UPDATE` for `u32` columns; on MySQL widen the `u32` columns to `BIGINT`, then the `UPDATE`.

- portable ASCII ids, exact markdown lookups, per-entity id strategies over a required default, SQLite-only SeaORM marked, u64 kept (#199)
  - Ids being created must match `[a-z0-9._~-]`, be at most 200 bytes, not be `index`/`log`, not be a Windows device name (`con`, `prn`, `aux`, `nul`, `com0`-`com9`, `lpt0`-`lpt9`, whole or before the first `.`), and not start or end with `.`. Existing rows stay reachable, but such ids cannot be created again, so rename them before relying on create. Slugs of titles with diacritics, non-ASCII letters or over 190 bytes change. Markdown lookups and relation ids that only matched through filesystem case or Unicode folding are now NotFound, as are device-name lookups on Windows. An entity directory that is a device name fails the markdown build, and `markdown_store::layout::validate_segment` refuses it. `Pipeline::store_id_strategy(..)` is required whenever a store stage runs, and `IdStrategy` no longer implements `Default` (use `IdStrategy::Provided` for the old behaviour); `EntityDef` gains `id_strategy`. `markdown_store::Error::InvalidId` gains `create_rule`. A cross-entity `has_many` fails the build. A `u64` field is `u64` in DTOs and frontmatter (was `i64`), and values above `i64::MAX` are refused on both backends.

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

### Added

- vaults are OKF 0.2 bundles by default (#194) **(breaking)**
- OKF index files and provenance stamps, configured once at build time (#196) **(breaking)**
- one id strategy on both backends, typed create errors, has_many drops, id order, i64 integer columns (#198) **(breaking)**
- portable ASCII ids, exact markdown lookups, per-entity id strategies over a required default, SQLite-only SeaORM marked, u64 kept (#199) **(breaking)**
- lists take an order and every transport sorts (#206) **(breaking)**



## [0.6.1] - 2026-09-28

### Added

- event ops take parameters, carry a sequence, and report lag ([#184](https://github.com/sksizer/rust-ontogen/pull/184))



## [0.6.0] - 2026-09-23

### ⚠ BREAKING CHANGES

- `EntityDef` and `FieldDef` gain a `doc` field, so an exhaustive struct
  literal must name it
- `CodegenError` gains a `Docs` variant, so an exhaustive match must handle it

### Added

- a docs stage emits the data-model reference and JSON Schema

## [0.5.0] - 2026-09-22

### Added

- string enums reach the admin registry, with labels and the id type

### merge

- main into schema-enums



## [0.4.1] - 2026-08-17




## [0.4.0] - 2026-08-08

### ⚠ BREAKING CHANGES

- **`biome_fmt` and the `biome` feature are removed.** TypeScript formatting is
  now a caller-supplied hook: `TsFormatter::{None, Custom, Command}` in
  `utils.rs`, with `TsFormatter::custom` / `custom_with` as constructors and
  `OnFormatError` controlling what happens when a hook fails. `None` is the
  default, so unconfigured callers get unformatted output.

  *(Landed as `fix(formatter)!: replace in-process biome with a consumer
  formatter hook`; omitted from the generated changelog and recorded here after
  the fact.)*

### Added

- path-aware TS format hook with configurable error policy **(breaking)**



## [0.3.0] - 2026-08-05

### Added

- in-process biome TypeScript formatter, feature-gated ([#120](https://github.com/sksizer/rust-ontogen/pull/120))
  — **removed again in 0.4.0**; see that entry.



## [0.2.0] - 2026-07-14

### Added

- emit axum 0.8 brace-style route params


