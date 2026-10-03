# ADR 0004 — JSON:API as the generated HTTP wire format

## Status

**proposed** (2026-10-03).

Records the decisions of epic
[E0004](../planning/epics/jsonapi-http-transport.md), settled with the
maintainer on 2026-10-03. Also records the major choices of the
[wire contract](../jsonapi-wire-contract.md) written in its phase 0. The
contract is the normative detail. This ADR is the why.

## Context

The generated Axum server speaks a dialect that exists only in ontogen:

- bare entity objects;
- a bare array for an unpaginated list, and a home-grown
  `PaginatedResult { items, total, limit, offset }` when paginated;
- `limit`/`offset` query parameters;
- `PUT` for a partial update;
- foreign keys inside the entity;
- one `{"error": string}` body that is always `500`.

Every consumer that is not the generated TS transport has to learn that
dialect from generated code. Examples are a mobile client, an integration
partner, a generic admin tool, or an agent.

JSON:API 1.1 is a versioned, widely implemented specification for exactly
this shape of API: resource CRUD with relationships, pagination, filtering
and a structured error document.

Three constraints shape how it can be adopted:

- **Flat entities elsewhere.** Tauri IPC, MCP and the TS `Transport`
  interface the admin layer programs against all share the flat entity. No
  media type or URL exists on those paths for JSON:API to describe.
- **Byte-identical layers.** ADR 0001 item 5 requires `gen_api`,
  `gen_servers` and `gen_clients` output to be byte-identical across store
  backends.
- **Breaks are allowed.** ADR 0003 allows a clean break before 1.0, with no
  compatibility mode.

Epic E0003 had planned a status mapping for the old `{"error": string}`
body, keeping that body as a wire contract. Its envelope is now replaced;
its mapping survives.

## Decision

**The generated HTTP server is a JSON:API 1.1 server and the generated TS
HTTP transport is its client.** JSON:API is applied and removed at the HTTP
boundary. The Rust store and API layers, IPC, MCP and the TS `Transport`
interface stay flat. There is no switch back to the old dialect.

### The epic's decisions

1. **Custom ops** respond with a meta-only document, `{ "meta": { "result": T } }`.
   A `()` return is `204`. The TS method signature still comes from the
   Rust fn, and the transport reads `.meta.result`.
2. **Event frames** carry a resource object (`type`, `id`, `attributes`,
   `relationships`). Subscribers still receive flat entities through the
   transport's flattener.
3. **Resource `type`** is `url_plural`, kebab-case (`workout-sets`), the
   same string as the URL segment.
4. **Ids on create.** `IdStrategy` (`Provided` / `SlugFromField` / `Uuid`)
   is extended to the SeaORM backend.
   - A client id in `data.id` is honoured. When it is absent, the strategy
     fills it.
   - `Provided` with no id is `400`, and a duplicate is `409`.
   - This closes the hole where SeaORM inserted whatever id the client sent,
     including `""`.
5. **The conversion** lives in a runtime crate, `ontogen-jsonapi`
   (documents, links, errors, media-type handling, extractors), which
   generated code calls. It mirrors `markdown-store`.
6. **Scope.** In scope: `include` (to-one and to-many, one level deep) and
   `sort`, with an ordering ADR covering both store backends
   ([ADR 0006](0006-ordering-on-both-store-backends.md)). E0003 phases 0-1
   (the `AppError` scan, `*NotFound → 404`, missing junction parameter →
   `400`) fold into E0004 phase 1. E0003 phases 2-3 stay in E0003.

### The contract's major choices

Each has a one-line reason in the contract's decision index (§16).

- **Strict media type.**
  - Request bodies must be `application/vnd.api+json`; `application/json`
    is `415`.
  - `Accept` is honoured, with `*/*` satisfying it.
  - `Vary: Accept` goes on every response.
  - `jsonapi: {"version": "1.1"}` goes on every document.
- **Relative links.** Every link and `Location` is a path. `links.self` is
  on every resource object and equals `Location` on create. The server
  cannot know its public origin.
- **Relationship mapping.**
  - A `belongs_to` field loses its `_id` suffix (`epic_id` → `epic`).
    `many_to_many` and `has_many` keep their field names.
  - Relation fields leave `attributes`.
  - `has_many` is read-only on the wire. The store sets listed children's
    foreign keys but never clears dropped ones, so it cannot honour
    JSON:API's full replacement. IPC and MCP keep today's write.
- **Pagination.**
  - `page[offset]`/`page[limit]` with today's clamp, except
    `page[limit]=0`, which is `400`.
  - `meta: {total, limit, offset}`, exactly the old `PaginatedResult`
    fields.
  - All four pagination links always present, `null` when unavailable.
  - Every link uses a canonical, percent-encoded query.
- **Every unknown query parameter is `400`**, including `fields[…]`. This
  is what the spec requires. Custom `GET` optional arguments move to an
  `opArg[…]` family, because all-lowercase names are reserved by the spec.
- **Custom `POST` bodies** are `{ "meta": { "args": { … } } }`, keyed by
  Rust parameter names: one rule in place of today's three binding shapes.
- **Statuses.** Create is `201` with the document. `PATCH` is `200` with
  the document. Delete and relationship mutations are `204`. `PUT` is
  `405`.
- **A new store-contract variant, `{Entity}AlreadyExists(id)`**, is
  constructed by both backends' create on a duplicate id. The E0003 scan
  maps it to `409` by convention, as `*NotFound` maps to `404`.
  - Duplicate detection is atomic on both backends.
  - Consumers declare the variant beside `{Entity}NotFound`.
- **Ids** follow one validity rule on both backends (markdown's path-safe
  rule), checked by the handler, so a bad client id is `400`, not a store
  `500`.
- **One check order for every route**, so each bad request has exactly one
  correct error. `detail` text is not normative.
- **Related resources** named in a request body must exist (`404`), as the
  spec requires. Dangling linkage in *responses* (a deleted target of a
  markdown wikilink) is tolerated and simply not included.
- **Relationship endpoints** are served from the generated store for every
  relation field, so they work with no user code. An unsupported
  relationship update (to-one `POST`/`DELETE`, junction `PATCH`) is `403`,
  as the spec requires, not `405`.
  - User-authored junction ops (`list_X`/`add_X`/`remove_X`) define a
    to-many relationship on their entity module.
  - Junction ops outside an entity module, or colliding with a relation
    field, are a `CodegenError`. A lone `list_X` without an `add` or
    `remove` partner is a custom op.
  - Junction `POST`/`DELETE` read membership first, so repeats succeed
    without calling user code. They are not atomic across several
    identifiers. That is a recorded deviation from the spec's "completely
    succeed or fail", because user code has no transaction to join.
- **Errors.**
  - One error object per response.
  - `code` is the `AppError` variant in snake_case, or a fixed ontogen
    code.
  - `title` is the status reason phrase.
  - `detail` is the old `error` text.
  - `source.pointer`, `parameter` or `header` where one applies.
- **The TS transport keeps `Transport` unchanged.**
  - It flattens on read and unflattens on write, and rebuilds
    `PaginatedResult` from `meta`.
  - It throws `JsonApiError { status, errors }`, whose message is the old
    text.
  - It sends neither `sort` nor `include`, because the admin layer calls
    `list` positionally.

## Consequences

**Positive:**

- Any JSON:API client can drive a generated server without custom code
  (the epic's conformance criterion).
- Not-found, conflict, bad-request and media-type failures get real
  statuses and machine-readable codes.
- Relationships are visible and navigable on the wire. Third-party clients
  gain `include` and `sort`.
- IPC, MCP and the admin layer are untouched. The flat model stays
  canonical.
- The contract closes latent defects found while writing it:
  - SeaORM's unchecked client ids;
  - bare list parameters bound as `Query<String>`, which serde does not
    appear able to fill from a query map;
  - scoped routes that diverge from unscoped ones (in-memory pagination,
    action-style junction routes);
  - Axum's plain-text extractor rejections.

**Negative:**

- **Breaking for every HTTP consumer outside the generated TS transport**
  (ADR 0003): envelope, methods, query parameters and error body all
  change. The 0.9.0 changelog carries the migration.
- **Consumer `AppError`s gain one variant per entity**
  (`{Entity}AlreadyExists`).
- **TS behaviour changes.**
  - The TS transport stops sending `has_many` edits over HTTP.
  - `String(e)` reads `JsonApiError: …` instead of `Error: …`.
- **Junction writes.** A junction `POST`/`DELETE` naming several ids can
  fail part-way and leave the earlier ids applied.
- **Cost.**
  - Create and update fetch each linked resource to check it exists.
  - `include` and related links fetch one target per id.
  - Relationship `POST`/`DELETE` are a read and a write, not one
    transaction, as every read-modify-write in the store already is.
- **Mounting.** The generated router must be mounted at the root, because
  links come from route templates.
- **Shadowing.** A custom-op action still shadows a resource id of the same
  string. That is unchanged and documented, not fixed.

**Follow-on work** is the epic's phases 1a through 4, as amended by the
contract's phase mapping (§17). Beyond them:

- a TS options parameter exposing `sort` and `include`;
- OpenAPI emission, which the epic leaves out of scope;
- E0003 phases 2-3, which apply to the new envelope unchanged.

## Alternatives considered

### A. Keep the dialect and document it

Write the dialect down and stop. Zero migration cost, but every new
consumer still learns a format nobody else speaks, and the defects above
stay. Rejected: the epic exists because the dialect is the problem.

### B. JSON:API as an opt-in mode beside the dialect

A `wire_format` switch. Two server emitters, two TS transports and two test
matrices, all to preserve a format with no external users. Rejected by
ADR 0003.

### C. Make the Rust entities JSON:API-shaped

Give entities `attributes` and `relationships`. Every IPC command, MCP tool,
store hook and admin component would change for a format they cannot use.
Rejected; see the epic's "Why the flat representation stays canonical".

### D. A different standard: OpenAPI-described REST, HAL, or JSON-LD

- OpenAPI describes a dialect; it does not choose one.
- HAL standardises links but not errors, pagination or filtering.
- JSON-LD targets linked data rather than CRUD.

JSON:API is the one specification that covers the whole surface, with
client libraries in every target ecosystem. Rejected in its favour.

### E. Lenient negotiation: accept `application/json` bodies

This would ease `curl` use. But a client sending the old flat body with the
old content type would get a confusing parse error, not a clear `415`.
Rejected.

### F. `has_many` writable by full replacement

Clear the foreign key of dropped children. That fails for a required
(non-`Option`) foreign key, and it changes store semantics for IPC and MCP
too. Rejected for this epic; the child's to-one relationship is the write
path.

## Notes

- Sources:
  - the epic E0004, including its "Decisions (2026-10-03)";
  - the [wire contract](../jsonapi-wire-contract.md);
  - [E0003](../planning/epics/http-error-mapping.md) and its
    [design](../http-error-mapping-design.md);
  - [JSON:API 1.1](https://jsonapi.org/format/1.1/).
- Sorting depends on [ADR 0006](0006-ordering-on-both-store-backends.md).
  Number 0005 is reserved for the OKF vault decision of
  [E0005](../planning/epics/okf-markdown-vault.md).
- E0003's "the `{"error": string}` body stays unchanged" wire contract is
  superseded by this ADR. Its mapping mechanism is not.
