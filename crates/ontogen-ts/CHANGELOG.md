# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-10-05

### ⚠ BREAKING CHANGES

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

- generated code survives colliding names or refuses them clearly (#210)
  - the generated HTTP file no longer exports PaginatedResult. Its page type is the private OntogenPaginatedResult; code that named PaginatedResult from the HTTP module declares its own type.
  - an MCP tool reads an argument named after a Rust keyword under serde's name for it, as its input schema already said: send type, not r#type.
  - a subscribeX method's args members are named by Tauri's key, so an event op argument _kind is args.kind where it was args.Kind.
  - some names that built with 0.8.0 are now refused at build time, among them entities named Ontogen..., OrderBy, Default, Send, Sync or Clone, and fields or arguments named ontogen_.... The upgrading guide lists them; rename the item as the error suggests.

### Added

- list filters are the JSON:API filter family (#202) **(breaking)**

### Fixed

- generated code survives colliding names or refuses them clearly (#210) **(breaking)**



## [0.1.6] - 2026-09-22

### Added

- string enums reach the admin registry, with labels and the id type

### merge

- main into schema-enums



## [0.1.5] - 2026-08-16

### Changed

- render every TypeScript type through one model



## [0.1.4] - 2026-08-16

### Added

- root every pool key at the crate it was scanned from



## [0.1.3] - 2026-08-16

### Fixed

- honor container-level #[serde(default)]



## [0.1.2] - 2026-08-16

### Fixed

- stop enum rename_all from renaming struct-variant fields



## [0.1.1] - 2026-08-16

### Added

- emit #[serde(flatten)] as a TS intersection


