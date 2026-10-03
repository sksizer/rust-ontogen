# ADR 0004 — JSON:API as the generated HTTP wire format

## Status

**proposed** (2026-10-03).

Records the decisions of epic
[E0004](../planning/epics/jsonapi-http-transport.md), settled with the
maintainer on 2026-10-03, and the contract choices that were contested in
review. The [wire contract](../jsonapi-wire-contract.md) is the normative
detail; its §16 lists every other choice with its reason.

## Context

The generated Axum server speaks a dialect that exists only in ontogen:

- bare entity objects;
- a bare array for an unpaginated list, and a home-grown
  `PaginatedResult { items, total, limit, offset }` when paginated;
- `limit`/`offset` query parameters;
- `PUT` for a partial update;
- foreign keys inside the entity;
- one `{"error": string}` body that is always `500`.

Every consumer other than the generated TS transport has to learn that
dialect from generated code. JSON:API 1.1 is a versioned, widely
implemented specification for exactly this shape of API: resource CRUD
with relationships, pagination, filtering and a structured error document.

Four constraints shape how it can be adopted:

- **Flat entities elsewhere.** Tauri IPC, MCP and the TS `Transport`
  interface the admin layer programs against all share the flat entity.
- **Byte-identical layers.** ADR 0001 item 5 requires `gen_api`,
  `gen_servers` and `gen_clients` output to be byte-identical across store
  backends.
- **Breaks are allowed.** ADR 0003 allows a clean break before 1.0, with no
  compatibility mode.
- **The servers and clients stages never see the parsed schema.**
  `gen_servers` ignores its `ApiOutput` and rescans `api_dir`. Only
  `ClientsConfig` carries a partial `schema_entities` copy. Yet nearly every
  JSON:API rule depends on the schema.

Epic E0003 had planned a status mapping for the old `{"error": string}`
body, keeping that body as a wire contract. Its envelope is now replaced;
its mapping survives.

## Decision

**The generated HTTP server is a JSON:API 1.1 server, and the generated TS
HTTP transport is its client.** JSON:API is applied and removed at the HTTP
boundary. The Rust store and API layers, IPC and MCP stay flat, and the TS
`Transport` interface stays flat. There is no switch back to the old
dialect.

### The epic's decisions

1. **Custom ops** respond with a meta-only document,
   `{ "meta": { "result": T } }`. A `()` return is `204`.
2. **Event frames** carry a resource object. Subscribers still receive flat
   entities through the transport's flattener.
3. **Resource `type`** is `url_plural`, kebab-case (`workout-sets`), the
   same string as the URL segment.
4. **Ids on create.** `IdStrategy` is extended to SeaORM. A client id is
   honoured; when it is absent, the strategy fills it. A missing id is
   `400` and a duplicate is `409`.
5. **The conversion** lives in a runtime crate, `ontogen-jsonapi`, which
   generated code calls.
6. **Scope.** `include` (one level) and `sort` are in, with an ordering ADR
   covering both backends ([ADR 0006](0006-ordering-on-both-store-backends.md)).
   E0003 phases 0-1 fold into E0004. E0003 phases 2-3 stay in E0003.
7. **Custom-op requests are fully conformant.** `POST` bodies are
   `{"meta":{"args":{…}}}`, and optional `GET` arguments are `opArg[…]`
   query parameters.
8. **Sort is on the shared TS `Transport`**, as a trailing optional
   `options` argument (`list(…, { sort })`), which breaks no positional
   caller. IPC and MCP list handlers honour it too. `include` stays
   HTTP-only, because the flat return shape has nowhere to put included
   resources.
9. **`has_many` is writable.** Phase 1a fixes the store so that an update
   dropping a child clears the child's foreign key, on both backends. That
   also fixes the bug on IPC and MCP. On the wire `has_many` is fully
   writable: full replacement by `PATCH`, and add or remove on the
   relationship endpoint. A child whose foreign key is not `Option` cannot
   be dropped; the write is `403 {child}_parent_required`, the status the
   spec requires for a refused relationship removal or replacement.

### Choices that were contested

**Schema input.** The parsed `&[EntityDef]` becomes an explicit first
argument of `gen_servers` and `gen_clients`, as it already is for
`gen_api`. `Pipeline` passes the entities it parsed;
`ClientsConfig::schema_entities` is removed.

A module with CRUD-named ops but no entity behind it is served as custom
ops, not rejected. That is the scan-dirs-only consumer, or a standalone
caller passing `&[]`. Rejecting it would break a use case the servers stage
supports today, and without a schema there is no resource to build. The
schema input lands in phase 1b, the first phase that builds resource
objects. Serving entity-less modules as custom ops lands in phase 1c, with
the rest of the custom-op wire. Both ship in 0.9.0.

**Typed store errors, mapped by name suffix.** The E0003 scan maps
`*NotFound` to `404` and gains three more suffixes:

- **`{Entity}AlreadyExists(id)` → `409`.** Detection is atomic on both
  backends. SeaORM retries a derived id that loses a race.
- **`{Entity}IdRequired(reason)` → `400`.** The store detects it after
  `before_create` hooks run, so a hook can assign the id. The HTTP handler
  does no pre-check and does not learn the `IdStrategy`.
- **`{Child}ParentRequired(child_id)` → `403`.** Decision 9's orphan case.
  The spec requires `403` when a server refuses to remove a member or to
  replace a to-many relationship.

The store generator constructs each variant it uses, so consumer
`AppError`s must declare them, as they already declare `{Entity}NotFound`.

**One source of truth for the markdown `IdStrategy`.** Today it is set
twice: in `build.rs` (`MarkdownIoOptions.id_strategy`) and again at runtime
(`VaultHandle::new(…, IdStrategy::…)`, e.g. tasks-tracker `main.rs`). The
build-time value wins. The generated store passes it to each create, and
`VaultHandle::new` loses its `IdStrategy` parameter (phase 1a). The
generator reads the build-time value already, to emit the slug source, so
a runtime copy can only disagree with it.

**Strict media type.** Request bodies must be `application/vnd.api+json`,
and `application/json` is `415`. A client still sending the old flat body
with the old content type gets a clear error, not a confusing parse
failure.

**One identifier per relationship `POST` or `DELETE`.** More than one is
`403 relationship_batch_unsupported`. The spec allows `403` for an
unsupported relationship update. With one identifier, every relationship
write is one store or junction-op call that succeeds or fails whole. A
junction op is user code with no transaction to join, so a multi-identifier
request could otherwise be left half-applied. The TS transport sends one id
per request.

**Junction ops outside an entity module** are served as custom ops at their
current paths. They are not a build error.

**Query parameters.** A route answers `400` to a parameter it does not
accept. The spec requires that for any reserved parameter the server does
not support, and for any name that follows none of its naming rules.
Ontogen defines one implementation-specific family, `opArg`, on custom ops
only.

**Latent defects the design fixes**, each in the named phase:

- A bare list parameter is extracted as `Query<String>`, which cannot
  deserialize from a query map, so every such request fails
  (`?skill_id=abc` gives `400 invalid type: map, expected a string`).
  Phase 2 binds it as `filter[skill_id]`.
- `has_many` updates never clear a dropped child's foreign key, on any
  transport. Phase 1a (decision 9).
- SeaORM inserts whatever id the client sends, including `""`. Phase 1a
  (decision 4 and the shared id-validity rule).
- Scoped routes diverge from unscoped ones. Phase 1c fixes in-memory
  pagination, and phase 3a fixes action-style junction routes.
- Axum's extractor rejections reach clients as plain text. Phase 1b.

## Consequences

**Positive:**

- Any JSON:API client can drive a generated server without custom code
  (the epic's conformance criterion).
- Not-found, conflict, bad-request and media-type failures get real
  statuses and machine-readable codes, on HTTP. The new store errors are
  typed on IPC and MCP too.
- Relationships are visible, navigable and writable on the wire.
  Third-party clients gain `include` and `sort`, and TS, IPC and MCP gain
  `sort`.
- IPC and MCP stay flat, and the admin layer is untouched.

**Negative:**

- **Breaking for every HTTP consumer outside the generated TS transport**
  (ADR 0003): envelope, methods, query parameters, custom-op bodies and the
  error body all change. The 0.9.0 changelog carries the migration.
- **Breaking for every consumer's `AppError`.** It gains `{Entity}IdRequired`
  and `{Entity}AlreadyExists` per entity, and `{Child}ParentRequired` where
  a required-foreign-key `has_many` exists.
- **Breaking for direct callers.** `gen_servers` and `gen_clients` gain a
  parameter, and `VaultHandle::new` loses one.
- **TS changes.** `String(e)` reads `JsonApiError: …` instead of
  `Error: …`. List methods gain a trailing optional argument.
- **Cost.**
  - Create and update fetch each linked resource to check that it exists.
  - `include` and related links fetch one target per id.
  - A markdown `has_many` write touches one file per affected child,
    best-effort across files (ADR 0001 item 2).
- **Mounting and shadowing.** The generated router must be mounted at the
  root, because links come from route templates. A custom-op action still
  shadows a resource id of the same string; that is unchanged, and
  documented rather than fixed.

**Follow-on work** is the epic's phases 1a through 4, with the contract's
§17 mapping sections to phases. Beyond them: OpenAPI emission, and E0003
phases 2-3, which apply to the new envelope unchanged.

## Alternatives considered

### A. Keep the dialect and document it

Zero migration cost, but every new consumer still learns a format nobody
else speaks, and the defects above stay. Rejected: the epic exists because
the dialect is the problem.

### B. JSON:API as an opt-in mode beside the dialect

Two server emitters, two TS transports and two test matrices, to preserve a
format with no external users. Rejected by ADR 0003.

### C. Make the Rust entities JSON:API-shaped

Every IPC command, MCP tool, store hook and admin component would change
for a format they cannot use. Rejected; see the epic's "Why the flat
representation stays canonical".

### D. A different standard: OpenAPI-described REST, HAL, or JSON-LD

OpenAPI describes a dialect; it does not choose one. HAL standardises links
but not errors, pagination or filtering. JSON-LD targets linked data rather
than CRUD. Rejected in favour of the one specification that covers the
whole surface.

### E. `has_many` read-only on the wire

Refuse `has_many` writes over HTTP, because the store never clears dropped
children. That leaves the bug in place on IPC and MCP. It also forces the
TS transport to silently drop `has_many` edits. Rejected for decision 9.

### F. Check missing ids in the HTTP handler

This needs the `IdStrategy` threaded to the servers stage. It runs before
`before_create` hooks, so it rejects an id a hook would have assigned. And
it leaves IPC and MCP untyped. Rejected for `{Entity}IdRequired`.

### G. Multi-identifier relationship writes, with partial failure accepted

A junction op called once per identifier can fail part-way. That is a
standing violation of "a request MUST completely succeed or fail", for a
capability the generated client never uses. Rejected for one identifier
per request.

## Notes

- Sources:
  - the epic E0004, including its "Decisions (2026-10-03)";
  - the [wire contract](../jsonapi-wire-contract.md);
  - [E0003](../planning/epics/http-error-mapping.md) and its
    [design](../http-error-mapping-design.md);
  - [JSON:API 1.1](https://jsonapi.org/format/1.1/).
- Sorting depends on [ADR 0006](0006-ordering-on-both-store-backends.md).
  Resource ids follow the reserved-id rule of
  [ADR 0005](0005-okf-markdown-vaults.md) on both backends.
- This ADR supersedes E0003's "the `{"error": string}` body stays
  unchanged" wire contract. E0003's mapping mechanism stands.
