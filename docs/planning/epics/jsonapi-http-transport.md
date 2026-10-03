---
type: epic
schema_version: "1"
id: E0004
status: proposed
title: JSON:API as the generated HTTP wire format
created: 2026-10-03
last_reviewed: 2026-10-03
tags: [servers, clients, http, jsonapi, wire-format]
---
# Epic — JSON:API as the generated HTTP wire format

**Milestone:** M4 — Standard formats ([roadmap](../../roadmap.md))
**Status:** proposed — gap analysis done, design questions settled 2026-10-03; phase 0 written (wire contract, ADR 0004, ADR 0006), maintainer decisions 7-9 added; phase 1a done ([#197](https://github.com/sksizer/rust-ontogen/pull/197), [#198](https://github.com/sksizer/rust-ontogen/pull/198)); phase 1b done ([#200](https://github.com/sksizer/rust-ontogen/pull/200)); phase 1c is next
**Spec:** [JSON:API 1.1](https://jsonapi.org/format/)
**Wire contract:** [`docs/jsonapi-wire-contract.md`](../../jsonapi-wire-contract.md) — normative; every phase implements against it
**Decision records:** [ADR 0004](../../architecture/0004-jsonapi-http-wire-format.md) (this epic's decisions),
[ADR 0006](../../architecture/0006-ordering-on-both-store-backends.md) (`OrderBy` on both store backends)
**Licence to break:** [ADR 0003](../../architecture/0003-api-design-over-backwards-compatibility.md)
— a cleaner surface takes the break; no compatibility flag, no `Plain` mode kept beside it.
**Supersedes in part:** [E0003 — consumer-controlled HTTP error responses](./http-error-mapping.md),
whose "the `{"error": string}` body stays unchanged" wire contract this epic replaces
with the JSON:API `errors[]` document. E0003's *mapping* mechanism (which `AppError`
variant becomes which status) survives; its *envelope* does not.

## Problem

The generated Axum server speaks an ad-hoc JSON dialect that exists nowhere
but here: bare entity objects, a bare array for an unpaginated list, a
home-grown `PaginatedResult { items, total, limit, offset }` envelope when
paginated, `limit`/`offset` query params, `PUT` for a partial update, and a
single `{"error": string}` body that is always `500`. Relations are foreign
keys inside the entity (`epic_id`, `tags: ["id", …]`) with nothing on the
wire saying what they point at.

Every consumer that is not the generated TS transport (a mobile client, an
integration partner, a generic admin tool, an agent reading the OpenAPI we
may one day emit) has to learn that dialect from the generated code. JSON:API
is the one widely implemented, versioned specification for exactly this
shape of API — resource-oriented CRUD with relationships, pagination,
filtering and a structured error document — and it has client libraries in
every ecosystem we care about.

## Goal

Every generated HTTP CRUD surface is a conformant JSON:API 1.1 server, and
the generated TS transport is its client. The in-process representation
(the Rust store, the IPC and MCP transports, the TS `Transport` interface the
admin layer programs against) stays flat; JSON:API is strictly the HTTP
serialization, applied and removed at the HTTP boundary.

## Gap analysis — today vs JSON:API 1.1

All "today" citations are against `src/servers/generators/http.rs` (http.rs)
and `src/clients/generators/transport.rs` (transport.rs) as of 0.8.0.

| Concern | Today | JSON:API 1.1 | Work |
|---|---|---|---|
| Media type | `application/json`; no negotiation | `application/vnd.api+json` on request and response; `415` on bad `Content-Type` params, `406` on unsatisfiable `Accept`; `Vary: Accept` | New extractor/response wrapper replacing Axum `Json` in the emitted handlers |
| Resource object | Bare entity JSON (http.rs:259, 299) | `{ type, id, attributes, relationships?, links? }`; `attributes` must not carry `id` or foreign keys | Emit a `{Entity}Resource` serializer per entity; `type` = `url_plural` (kebab, e.g. `workout-sets`) |
| Relationships | FK scalars and id arrays inside the entity (`epic_id`, `tags`) populated by `populate_{x}_relations` | `relationships.{name}.data` = resource identifier(s) `{type,id}`; `links.self` / `links.related` | Derive from `RelationInfo`: `belongs_to` → to-one, `has_many`/`many_to_many` → to-many; relationship name = field name minus `_id` |
| Single fetch | `GET /api/{plural}/{id}` → bare, 500 on miss | `{ data: resource }`; `404` on miss | Status from the E0003 mapping; envelope here |
| Collection | Bare array, or `PaginatedResult` (http.rs:126-132) | `{ data: [resource…], meta?, links? }` | One shape whether paginated or not; `meta.total` when a count exists |
| Pagination | `limit` / `offset` (http.rs:134-138), clamped by `PaginationConfig` | reserved `page` family; strategy unspecified; `links.first/prev/next/last` | `page[limit]` / `page[offset]`; clamp rules unchanged except `page[limit]=0` is `400`; emit `self` plus the four pagination links |
| Filtering | A user-authored `*Query` struct, or bare `&str` params, each a top-level query param (http.rs:199-235) | reserved `filter` family; strategy unspecified | Map `*Query` fields to `filter[field]`; the filter-aware `count` (#172) already makes `meta.total` correct |
| Sorting | None | `sort=field,-other` | Needs store support (`OrderBy` was deliberately left out by ADR 0001 amendment 4); designed in [ADR 0006](../../architecture/0006-ordering-on-both-store-backends.md), built in phase 3c |
| Create | `POST` bare `CreateXInput` → `201` bare entity; SeaORM stores the client's id verbatim, markdown fills a missing one by `IdStrategy` | `POST { data: { type, attributes, relationships? } }` → `201` + `Location`; client id allowed or `403`; `409` on conflict | Honour a client id; fill a missing one by `IdStrategy` on both backends; `409` on duplicate (decision 4) |
| Update | `PUT` bare `UpdateXInput`, `double_option` for nullable (http.rs:316) | `PATCH { data: { type, id, attributes } }`; absent = unchanged, `null` = clear → `200` resource or `204` | Method change; the `double_option` semantics already match |
| Delete | `204` | `200` / `204` | Unchanged |
| Errors | `{ error: String }`, always `500` (http.rs:101-113) | `{ errors: [{ status, code, title, detail, source }] }` | Envelope here; status mapping from E0003 phases 1-3 |
| Junction ops | `GET`/`POST /{plural}/{id}/{child}`, `DELETE …/{child}/{child_id}` (http.rs:389-436) | `GET`/`POST`/`PATCH`/`DELETE /{plural}/{id}/relationships/{rel}` with resource-identifier bodies | Re-route `list_X`/`add_X`/`remove_X` onto the relationship endpoint; phase 3 |
| Compound documents | None | `include=rel.path` → `included: [resource…]`, full linkage | Store already has `get` by id; phase 3, to-one and one-level to-many only |
| Sparse fieldsets | None | `fields[type]=a,b` | Out of scope (serde emits whole structs) |
| Custom ops | `GET`/`POST /{plural}/{action}`, bare JSON in and out | Not covered by the spec | Meta-only document `{ meta: { result } }` (decision 1) |
| SSE / event ops | `GET /api/events/{name}`, resumable (#184) | Not covered by the spec | Routes unchanged; each frame carries a resource object (decision 2) |
| `route_prefix`, extra surfaces | `/api/{segments}/{plural}` (config.rs:225-245) | URL structure is implementation-defined | Unchanged |
| TS transport | Returns server JSON as-is; `PaginatedResult` declared identically; throws `Error(body.error)` (transport.rs:184-191, 377-416) | — | Flatten resource → entity on read, build `{data}` on write, keep the flat `Transport` interface; surface `errors[]` as a typed `JsonApiError` |
| Admin layer | Detects `{items,total}` vs array (`adminListAll.ts`) | — | Unchanged if the TS transport keeps returning `PaginatedResult`; the registry flags (`paginated`, `listHasQuery`) are unaffected |

### Why the flat representation stays canonical

Three transports share one `Transport` interface in TS and one store in Rust.
IPC (Tauri) and MCP have no media type to negotiate and no URL to put a
`relationships` link on; JSON:API is meaningless there. If the entity types
themselves grew `attributes`/`relationships`, every IPC command, MCP tool,
store hook and admin-layer component would change for no benefit on those
paths. So the resource object is produced by the HTTP handler from the flat
entity and consumed back into the flat entity by the TS transport. The
`populate_{x}_relations` id arrays are exactly the resource linkage JSON:API
wants; they just move from `attributes` to `relationships` on the wire.

## Scope

In scope: the Axum HTTP generator, the TS HTTP transport, a new
`ontogen-jsonapi` runtime crate, `IdStrategy` on the SeaORM store, the error
document, the examples' checked-in generated trees and lockfiles (gated by
#180), the one HTTP insta snapshot, `src/servers/tests.rs`
assertions that pin `/api/`, `PaginatedResult` or `ErrorResponse`, the
`markdown-pilot` live router test, and the site pages `guides/server-transports`,
`guides/api-layer`, `guides/client-generation`, `guides/relationships`,
`reference/configuration`, `cookbook/custom-api-endpoints`.

Out of scope: sparse fieldsets, the atomic operations extension, `lid`
local ids, `202 Accepted` deferred processing, profiles, OpenAPI emission
(a natural follow-on once the shape is standard), and any change to IPC or
MCP payloads.

## Phases

Each phase is one or two PRs and ships green on its own. Phases 1a, 1b and
1c are released together as the breaking `0.9.0`. Later phases add to the
wire, except 3c, whose store `list_*` signature change is a further break
(ADR 0003). The [wire contract](../../jsonapi-wire-contract.md)'s §17 maps
each of its sections to a phase.

**Phase 0 — wire contract and decision records.** Done:

- [`docs/jsonapi-wire-contract.md`](../../jsonapi-wire-contract.md), with one
  worked request/response per operation against the tasks-tracker schema;
- [ADR 0004](../../architecture/0004-jsonapi-http-wire-format.md), recording
  the decisions below;
- [ADR 0006](../../architecture/0006-ordering-on-both-store-backends.md),
  designing `OrderBy`, with ADR 0001 amendment 9 pointing to it.

**Phase 1a — store and runtime prerequisites.**

- The `ontogen-jsonapi` crate: document types, link building with the
  canonical query (contract §4.3), the error document, and the media-type,
  query and path extractors that replace Axum's rejections.
- `IdStrategy` on SeaORM (decision 4), with the markdown `-2`, `-3` probe
  and a retry when a derived-id insert loses a race. One build-time source
  of truth for the strategy: `VaultHandle::new` loses its `IdStrategy`
  parameter (ADR 0004).
- The id-validity rule and slug function, shared by both backends.
- The typed store errors `{Entity}AlreadyExists` and `{Entity}IdRequired`
  (the latter detected after `before_create`), with every example's
  `AppError` updated.
- The `has_many` fix (decision 9): an update that drops a child clears its
  foreign key on both backends, or fails with `{Child}ParentRequired` when
  that foreign key is not `Option`.
- The markdown id-ascending default order, many_to_many linkage order, and
  the runtime parity fixture's default-order cases (ADR 0006 §6). SeaORM
  already orders by id since
  [#178](https://github.com/sksizer/rust-ontogen/pull/178).
- The SeaORM `i64` field for integer primitives the parser files under
  `OptionEnum` or `Other`, with `usize`, `isize`, `u128` and `i128` added
  to `is_integer_primitive`, and `from_model`/`to_active_model` returning
  `Result` (ADR 0006 §4). Today's `u32` wrap is lossless but sorts and
  filters wrongly; after the change a wrapped row fails to read. The
  release notes must carry the pre-upgrade SQLite migration
  `UPDATE t SET x = x + 4294967296 WHERE x < 0;` per `u32` column (`u8`,
  `u16` and signed types need none).
- Markdown lookups of an id that could never be created answer
  `{Entity}NotFound` instead of a `500` (contract §8.2).

**Phase 1b — CRUD over JSON:API.**

- The parsed schema as an explicit input of `gen_servers` and
  `gen_clients` (contract §5.1). Modules with no entity behind them keep
  today's handlers until 1c.
- Media type, documents, and resource objects with relationship `data`.
- Query-parameter rules, list and pagination (`page[]`, `meta`, `self` plus
  the four pagination links).
- Get, create (`201` + `Location`), `PATCH` replacing `PUT`, delete, and the
  related-resource existence checks.
- The `errors[]` document with E0003 phases 0-1 and the new suffixes, the
  `405` fallback, and scoped CRUD routes.
- TS: flatten and unflatten for CRUD methods, and `JsonApiError`.
- Snapshots and examples regenerated, with tasks-tracker paginating `task`.

**Phase 1c — the rest of the 0.9.0 wire.**

- Custom ops as meta-only documents, with `meta.args` request bodies and
  `opArg[…]` (decisions 1 and 7), and the singleton CRUD check.
- Ops served as custom ops (contract §10.4): CRUD ops with no entity
  behind them, and junction ops until 3a.
- Event frames as resource objects (decision 2).
- Scoped pagination made identical to unscoped.
- TS custom, junction and subscription methods.

**Phase 2 — filter family.**

- `*Query` struct fields become `filter[field]`, and bare list params
  likewise. This fixes the bare-param `Query<String>` defect.
- A hand-written `list` (and its `count`) replaces the generated one.
- Unknown filter members are `400`.
- TS `toQueryString` emits the bracketed form.
- `meta.total` comes from the filter-aware count.
- tasks-tracker gains a hand-written `list` with `ListTasksQuery`.

**Phase 3 — relationships, inclusion and sorting.**

- **3a.** `/relationships/{rel}` and related-resource endpoints:
  - every relation field is served from the generated store, `has_many`
    writable (decision 9);
  - junction ops in resource modules are re-routed onto the same endpoints;
    a lone `list_X` stops classifying as a junction op;
  - `links.self`/`links.related` go on every relationship;
  - scoped junction routes are fixed, and TS junction methods move;
  - tasks-tracker gains `parent_id`/`subtasks` and a `release` tag.
- **3b.** `include`: one level, to-one and to-many, with full linkage.
- **3c.** The `order` argument per ADR 0006 on both store backends, with
  `sort_{plural}` and `order_{plural}_query`, and the rest of the runtime
  parity fixture. `sort` on HTTP, and on the TS `Transport`, IPC and MCP
  (decision 8). tasks-tracker's hand-written `list` gains `order`.

**Phase 4 — docs and examples.** Site pages above, cookbook update, a
"what the wire looks like" page per example, README.

## Acceptance criteria

- A conformance run of a third-party JSON:API client (e.g. `jsona` or the
  `@jsonapi` TS packages, or a `json-api-rs` consumer) against
  `examples/tasks-tracker` performs list, get, create, patch, delete and a
  relationship fetch without custom code.
- `tests/backend_parity.rs` still proves `gen_api` / `gen_servers` /
  `gen_clients` byte-identical across SeaORM and markdown backends.
- Every error the generated server returns is an `errors[]` document with a
  non-`500` status wherever E0003 phase 1 can derive one.
- The generated TS transport's `Transport` interface is unchanged for
  CRUD except the trailing optional list `options` argument (decision 8);
  `packages/nuxt_admin_layer` tests pass with no source changes.
- iron-log, iron-log-md, notes-kb and tasks-tracker run end-to-end on the
  new wire; `markdown-pilot` live router test passes.
- The HTTP snapshot and the 27 `src/servers/tests.rs` assertions pinning the
  old shape are replaced, not deleted.

## Decisions (2026-10-03)

Settled with the maintainer; recorded in
[ADR 0004](../../architecture/0004-jsonapi-http-wire-format.md), and
specified in detail by the [wire contract](../../jsonapi-wire-contract.md).

1. **Custom ops** respond with a meta-only document, `{ meta: { result: T } }`.
   Client typing is unaffected: the TS method signature comes from the Rust
   fn at generation time, and the transport reads `.meta.result`. One media
   type, one error path.
2. **Event frames** carry a resource object (`{ type, id, attributes,
   relationships }`). Subscribers still receive flat entities through the
   transport's flattener; a JSON:API-aware client reuses its deserializer.
3. **Resource `type`** is `url_plural`, kebab-case (`workout-sets`), the same
   string as the URL segment.
4. **Ids on create.** `IdStrategy` (`Provided` / `SlugFromField` / `Uuid`)
   is extended to the SeaORM backend in phase 1. A client id in `data.id` is
   honoured; when absent the strategy fills it; `Provided` with no id is
   `400`; a duplicate is `409`. This also closes a latent hole: today the
   SeaORM create inserts whatever the client sent, including an empty
   string, unless a `before_create` hook intervenes.
5. **The resource/flat conversion** lives in a runtime crate,
   `ontogen-jsonapi` (envelope, links, errors, media-type handling),
   mirroring `markdown-store`. Generated code calls it.
6. **Scope.** In: `include` (to-one and one-level to-many) and `sort`, with
   an `OrderBy` ADR covering both store backends (ADR 0001 amendment 4
   revisited). Error-mapping epic E0003 phases 0-1 fold into phase 1 here
   (the `AppError` scan, `*NotFound` → `404`, missing junction param →
   `400`); E0003 phases 2-3 stay in E0003.
7. **Custom-op requests are fully conformant.** `POST` bodies are
   `{ meta: { args: { … } } }`, keyed by Rust parameter name, and optional
   `GET` arguments are `opArg[…]` query parameters.
8. **Sort is on the shared TS `Transport`**, as a trailing optional
   argument (`list(…, { sort? })`) that breaks no positional caller. IPC
   and MCP list handlers honour it too. `include` stays HTTP-only, since
   the flat return shape has nowhere to put included resources.
9. **`has_many` is writable.** Phase 1a fixes the store's partial apply: an
   update that drops children clears their foreign keys on both backends,
   which also fixes IPC and MCP. On the wire `has_many` takes full
   replacement by `PATCH`, and add or remove on its relationship endpoint.
   A child whose foreign key is not `Option` cannot be dropped:
   `403 {child}_parent_required`, as the spec requires for a refused
   relationship removal or replacement.

## Dependencies

- E0003 phases 0-1 (the `AppError` scan and `*NotFound → 404` mapping)
  fold into phase 1 here (decision 6).
- [#180](https://github.com/sksizer/rust-ontogen/pull/180) (examples drift
  guard) is merged, so the regenerated example trees are CI-checked.
- Phases 1a and 3c implement [ADR 0006](../../architecture/0006-ordering-on-both-store-backends.md),
  which supersedes ADR 0001 amendment 4. It builds on
  [#178](https://github.com/sksizer/rust-ontogen/pull/178) (merged), which
  already orders SeaORM lists by id.

## Tasks

Filed when phase 0 closes; one task per phase, PR-sized.

- [x] phase 0 — wire contract doc + ADR 0004 + ADR 0006
- [x] phase 1a — `ontogen-jsonapi` runtime crate, `IdStrategy` on SeaORM with one source of truth, `{Entity}AlreadyExists`/`IdRequired`, `has_many` fix, markdown default id order ([#197](https://github.com/sksizer/rust-ontogen/pull/197) runtime crate, [#198](https://github.com/sksizer/rust-ontogen/pull/198) store)
- [x] phase 1b — schema input, CRUD envelope, media type, errors, PATCH, TS flattener ([#200](https://github.com/sksizer/rust-ontogen/pull/200))
- [ ] phase 1c — custom ops, ops served as custom, event frames, scoped pagination
- [ ] phase 2 — filter family
- [ ] phase 3a — relationship endpoints, related links, junction re-route
- [ ] phase 3b — `include` compound documents
- [ ] phase 3c — `order` argument on both backends (ADR 0006) + `sort` on every transport
- [ ] phase 4 — docs and examples
