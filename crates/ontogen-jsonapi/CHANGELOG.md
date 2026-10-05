# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-10-05

### ⚠ BREAKING CHANGES

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

- wire and store refuse bad writes alike and write nothing (#209)
  - the consumer's SeaORM sync_junction takes the connection to write on after &self (conn: &C, generic over sea_orm's ConnectionTrait) and must run every statement on it. Generated create_* and update_* make all their writes in one transaction and pass it in, so a write that fails part-way leaves nothing written.
  - both store backends refuse a many_to_many id that names no record with {Target}NotFound, before anything is written. The markdown store used to write the dangling link, and SeaORM wrote the row before the junction's foreign key failed. Create the targets first. An update checks only the ids it adds, not those already in the record's list, so a record holding a dangling id still gains and loses links.
  - a consumer AppError declares {Child}ParentCycle(String) for the child of a has_many, which is the declaring entity (Task with subtasks declares TaskParentCycle), or the generated store does not compile. A has_many list naming the record itself is refused with it instead of making the record its own parent.
  - with a servers or clients stage, a hand-written list or count that replaces a generated one must be in a file those stages read, in their api_dir or a directory directly inside it. One found only through api_scan_dirs now fails the build; move it into a transport's api_dir, or rename it to keep the generated fn.
  - a has_many foreign_key must name the declaring entity's belongs_to to itself, typed String or Option<String>. A missing foreign_key, a plain field, a belongs_to to another entity or a field not typed String or Option<String> now fails the build with a CodegenError.
  - a markdown list, count or has_many list no longer returns a vault file whose stem is not UTF-8 or fails the lookup check (layout::validate_lookup_id), such as a:b.md, a\b.md, draft..md or draft .md. Such files were listed although no get, update or delete could reach them. Rename them to ids the create rule accepts.
  - markdown_store::IdStrategy::Uuid exists only with markdown-store's uuid feature. A markdown store using it without the feature used to build and fail every create without an id; it now fails to compile. Enable features = ["uuid"] on markdown-store.

### Added

- generated CRUD routes speak JSON:API and the TS transport is their client (#200) **(breaking)**
- custom, junction and event routes speak JSON:API (#201) **(breaking)**
- list filters are the JSON:API filter family (#202) **(breaking)**
- relationships are served at their JSON:API endpoints (#203) **(breaking)**
- lists and gets answer include with compound documents (#205) **(breaking)**
- lists take an order and every transport sorts (#206) **(breaking)**

### Fixed

- server error codes never collide with store variants (#208) **(breaking)**
- wire and store refuse bad writes alike and write nothing (#209) **(breaking)**


