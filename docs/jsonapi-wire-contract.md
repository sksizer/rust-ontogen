# JSON:API wire contract for the generated HTTP server

Status: normative contract for epic
[E0004](planning/epics/jsonapi-http-transport.md), recorded as
[ADR 0004](architecture/0004-jsonapi-http-wire-format.md). Sorting rests on
[ADR 0006](architecture/0006-ordering-on-both-store-backends.md). Every
phase of E0004 implements against this document. Where code and this
document disagree, the code is wrong, or this document is amended in the
same PR.

The spec is [JSON:API 1.1](https://jsonapi.org/format/1.1/). MUST, SHOULD
and MAY in this document carry their RFC 2119 meaning and bind the
*generated server* and the *generated TS transport*. Rules quoted from the
spec are not repeated unless ontogen makes a choice inside them.

"Decision N" means item N of the epic's
[Decisions (2026-10-03)](planning/epics/jsonapi-http-transport.md#decisions-2026-10-03)
list. Choices this document makes itself are listed with their reasons in
§16.

## Operation index

| Operation | Method and path | Success | Section |
|---|---|---|---|
| List | `GET /api/{type}` | `200` | §7 |
| Get | `GET /api/{type}/{id}` | `200` | §8.1 |
| Create | `POST /api/{type}` | `201` | §8.2 |
| Update | `PATCH /api/{type}/{id}` | `200` | §8.3 |
| Delete | `DELETE /api/{type}/{id}` | `204` | §8.4 |
| Fetch relationship | `GET /api/{type}/{id}/relationships/{rel}` | `200` | §9 |
| Change relationship | `PATCH`, `POST`, `DELETE` on the same path | `204` | §9 |
| Fetch related | `GET /api/{type}/{id}/{rel}` | `200` | §9.3 |
| Custom op | `GET` or `POST /api/{module}/{action}…` | `200`, or `204` for `()` | §10 |
| Event stream | `GET /api/events/{name}…` | `200` `text/event-stream` | §12 |

## Contents

1. [Scope](#1-scope)
2. [The running example](#2-the-running-example)
3. [Media type and negotiation](#3-media-type-and-negotiation)
4. [Documents](#4-documents)
5. [Resource objects](#5-resource-objects)
6. [Query parameters](#6-query-parameters)
7. [Collections: list, pagination, filter, sort, include](#7-collections)
8. [Single resources: get, create, update, delete](#8-single-resources)
9. [Relationship endpoints and related links](#9-relationship-endpoints-and-related-links)
10. [Custom ops and singletons](#10-custom-ops-and-singletons)
11. [Route prefix and extra surfaces](#11-route-prefix-and-extra-surfaces)
12. [Event streams](#12-event-streams)
13. [Errors](#13-errors)
14. [TS transport mapping](#14-ts-transport-mapping)
15. [IPC and MCP](#15-ipc-and-mcp)
16. [Decision index](#16-decision-index)
17. [Phase mapping](#17-phase-mapping)

## 1. Scope

This contract covers every route the Axum generator (`ServerGenerator::HttpAxum`)
emits, and the generated TS HTTP transport that consumes them. It replaces
the ad-hoc dialect described in the epic's gap analysis: bare entities, bare
arrays, `PaginatedResult`, `limit`/`offset`, `PUT`, and `{"error": string}`.

It also fixes the store-layer changes the wire depends on:

- the `order` argument and sort parsing (ADR 0006);
- `has_many` writes that clear dropped children (decision 9);
- the typed store errors `{Entity}AlreadyExists`, `{Entity}IdRequired` and
  `{Child}ParentRequired` (§13.4);
- the schema reaching the servers and clients stages (§5.1).

Tauri IPC and MCP payloads stay flat. They gain only the optional `sort`
argument (decision 8) and the store fixes above (§15). The TS `Transport`
interface gains one trailing optional argument on list methods (§14).

Out of scope, as the epic records: sparse fieldsets, the atomic operations
extension, `lid`, `202 Accepted`, profiles, and OpenAPI emission. Each is
rejected on the wire as described below, never silently ignored.

## 2. The running example

Resource examples use `examples/tasks-tracker` (markdown backend,
`IdStrategy::SlugFromField("title")`) and its checked-in vault.
tasks-tracker has no custom ops, event ops or junction ops, so §9.1, §10
and §12 take their examples from iron-log where one exists, and otherwise
say the example is illustrative.

| Rust entity | Module | Resource `type` | Base path |
|---|---|---|---|
| `Epic { id, title, status, body }` | `epic` | `epics` | `/api/epics` |
| `Tag { id, title }` | `tag` | `tags` | `/api/tags` |
| `Task { id, title, status, created, epic_id, tags, body }` | `task` | `tasks` | `/api/tasks` |

`Task.epic_id` is `belongs_to Epic` (an `Option<String>`). `Task.tags` is
`many_to_many Tag`. The vault holds one record of each: task
`ship-the-emitter`, epic `markdown-backend` and tag `codegen`.

The example does not yet exercise every feature, so some sections show it
with additions that the named phase makes to tasks-tracker. That keeps every
example in this document live once its phase lands:

| Addition | Lands in | Used by |
|---|---|---|
| `pagination: Some(PaginationConfig { default_limit: 20, max_limit: 100 })`, `paginated: ["task"]` | 1b | §7.2 |
| A hand-written `task::list(store, query: ListTasksQuery, limit, offset)` with `ListTasksQuery { status: Option<String>, epic_id: Option<String> }` and a matching `count`, replacing the generated `list` (§7.3) | 2 | §7.3 |
| `order: &[OrderBy<TaskSortField>]` added to that hand-written `list` | 3c | §7.4 |
| `Task.parent_id: Option<String>` (`belongs_to Task`) and `Task.subtasks: Vec<String>` (`has_many Task`, `foreign_key = "parent_id"`), as in `crates/markdown-pilot` | 3a | §5.4, §9 |
| A second tag, `release` | 3a | §9.2 |

Examples show the wire after every phase has landed. Before phase 3a:
- relationship objects carry `data` only, without the `links` shown here;
- junction-op relationships are absent altogether, since they have no
  `data` (§5.4).

The task body is abbreviated as `"## Goal\n…"` after its first appearance.
Response headers common to every response (§3.3) are shown once per
section, not on every example.

## 3. Media type and negotiation

### 3.1 Responses

- Every response that carries a body from a JSON:API route MUST have
  `Content-Type: application/vnd.api+json`, with no media type parameters.
  This includes error responses. Ontogen applies no extensions or
  profiles, so it has no parameter to state.
- A `204 No Content` response has no body and no `Content-Type`.
- Event-stream routes (§12) respond with `text/event-stream`. They are not
  JSON:API documents.

### 3.2 Requests

**Content-Type.** A request that carries a body MUST have
`Content-Type: application/vnd.api+json`. Bodies are carried by:
- `POST` to a collection;
- `PATCH` to a resource;
- a custom `POST` with a body (one may also carry none, §10.2);
- `POST`, `PATCH` and `DELETE` on a relationship endpoint.

The server responds:

| Request `Content-Type` | Response |
|---|---|
| `application/vnd.api+json` | processed |
| `application/vnd.api+json; profile="…"` | processed; unknown profiles are ignored, as the spec requires |
| `application/vnd.api+json; ext="…"` | `415`. Ontogen supports no extension, so every `ext` URI is unsupported |
| `application/vnd.api+json` with any other parameter (e.g. `charset=utf-8`) | `415`, as the spec requires |
| anything else, including `application/json`, or the header absent while a body is present | `415` |

Every `415` carries `source.header: "Content-Type"`. A body on `GET` or on
a resource `DELETE` is ignored, and its `Content-Type` is not checked.

**Accept.** Media types, parameter names and the `q` parameter are
compared case-insensitively. A range with `q=0` is "not acceptable", as
RFC 9110 defines it. The rule is evaluated in this order:

1. `Accept` absent: acceptable.
2. If `Accept` lists `application/vnd.api+json` at least once, the
   request is acceptable only when at least one of those instances:
   - has a non-zero `q`, and
   - has no parameters other than `profile` and `q`.

   Wildcards do not rescue it. `application/vnd.api+json; charset=utf-8, */*`
   and `application/vnd.api+json; ext="https://…", */*` are both `406`, as
   the spec requires when every JSON:API instance carries a disallowed
   parameter or an unsupported extension. So is
   `application/vnd.api+json;q=0, */*`.
3. Otherwise the request is acceptable when a listed range with a non-zero
   `q` is `*/*` or `application/*`.
4. Anything else is `406` with `source.header: "Accept"`.

### 3.3 Headers on every response

Every response from a JSON:API route, including `204` and every error,
carries `Vary: Accept`. The response depends on `Accept` (it can be `406`),
so a cache must key on it. The spec's SHOULD applies because the server
honours the `profile` parameter by ignoring it.

Examples below show the request line and the headers that matter. A
response written as `200 OK` implies:

```http
Content-Type: application/vnd.api+json
Vary: Accept
```

### 3.4 Query-string brackets

The server MUST accept `[` and `]` in parameter names both raw and
percent-encoded (`%5B`, `%5D`), treating them identically, as the spec
requires. Links the server emits always use the percent-encoded form
(§4.3). Examples in this document write raw brackets in request lines for
readability.

## 4. Documents

### 4.1 Top-level members

Every response document has these top-level members, in this order. Order
is fixed so that insta snapshots and example diffs are deterministic.

| Member | When present |
|---|---|
| `jsonapi` | always, exactly `{"version": "1.1"}` |
| `links` | on every document with primary data (§4.3) |
| `meta` | paginated collections (§7.2, §9.1) and custom ops (§10) |
| `data` | every success document except custom ops |
| `included` | when the request carried `include` (§7.5) |
| `errors` | error documents only (§13) |

`jsonapi` is emitted on every document, including errors. The contract
relies on 1.1 semantics: the `ext` and `profile` negotiation rules in §3.2,
and the 1.1 error and link members.

### 4.2 Links are relative

Every link the server emits, and the `Location` header, is a relative
reference: an absolute path plus an optional query, e.g.
`/api/tasks/ship-the-emitter`. RFC 3986 permits a URI-reference, and
RFC 9110 §10.2.2 permits a relative `Location`.

Links are built from the route template ontogen generated (`/api/...`), not
from the request's original URI. The generated router MUST therefore be
mounted at the root, as every example does. A consumer that nests it under
another prefix gets links without that prefix.

Ids and other path parameters are percent-encoded as RFC 3986 path
segments: every byte outside the unreserved set (`A-Z a-z 0-9 - . _ ~`) is
encoded.

### 4.3 Canonical link queries

Top-level `links.self` and the pagination links repeat the request's
JSON:API query parameters, in a canonical form, so that equal requests
produce byte-equal links:

1. Parameter order: every `filter[…]` parameter in ascending byte order of
   its member name, then `sort`, then `include`, then `page[offset]`, then
   `page[limit]`.
2. Names: `[` and `]` are written `%5B` and `%5D`.
3. Values: every byte outside the unreserved set is percent-encoded, except
   `,`, which stays literal because it separates `sort` and `include`
   items. The spec allows a value serialization that differs from
   `application/x-www-form-urlencoded` as long as it parses back.
4. `sort` and `include` are written as parsed: duplicates in `include`
   removed, request order kept.
5. Paginated collections always carry both `page` parameters, with the
   effective values after clamping (§7.2), whether or not the client sent
   them.
6. A parameter the client did not send, and that rule 5 does not add, is
   absent. A request with no JSON:API query parameters on an unpaginated
   collection has `self` equal to the bare path.

## 5. Resource objects

### 5.1 Schema input

Building a resource object needs the schema. The generator must know:

- which fields are attributes and which are relationships;
- each relationship's kind and target, and whether a to-one is `Option`;
- whether an event's item type is an entity;
- the serde attributes that §5.3 restricts.

Today neither stage sees it: `gen_servers` ignores its `ApiOutput` argument
and rescans `api_dir`, and only `ClientsConfig` carries a
`schema_entities` copy.

From phase 1b the parsed schema is an explicit first argument of both
stages, as it already is for `gen_api`:

```rust
pub fn gen_servers(entities: &[EntityDef], api: Option<&ApiOutput>, scan_dirs: &[PathBuf], config: &ServersConfig) -> Result<ServersOutput, CodegenError>;
pub fn gen_clients(entities: &[EntityDef], api: Option<&ApiOutput>, scan_dirs: &[PathBuf], config: &ClientsConfig) -> Result<(), CodegenError>;
```

- `Pipeline` passes the entities it parsed.
- Standalone callers pass `parse_schema`'s output, or `&[]`.
- `ClientsConfig::schema_entities` is removed. It was the only partial
  route by which the schema reached a stage.

A module is a **resource module** when its name is the module name of an
entity in `entities`. Its CRUD ops are served as resources (§7, §8). A
module with CRUD-classified ops (`list`, `get_by_id`, `create`, `update`,
`delete`) but no entity behind it is served entirely as custom ops (§10.4),
from phase 1c. Phase 1b gives it no new shape, and both phases ship in the
same release.
That module is what a scan-dirs-only consumer, or a standalone caller
passing `&[]`, has. The alternative, a `CodegenError`, would break the
scan-dirs-only use case the servers stage supports today, and those ops
have no schema from which to build a resource.

### 5.2 Shape

```json
{
  "type": "tasks",
  "id": "ship-the-emitter",
  "attributes": { "…": "…" },
  "relationships": { "…": { "links": { "…": "…" }, "data": "…" } },
  "links": { "self": "/api/tasks/ship-the-emitter" }
}
```

Member order is `type`, `id`, `attributes`, `relationships`, `links`.

- **`type`** is the module's `url_plural`, kebab-case: `tasks`,
  `workout-sets` (decision 3). It is the same string as the URL segment.
  Plural overrides in `NamingConfig` apply to both.
- **`id`** is the value of the entity's `#[ontology(id)]` field, whatever
  that field is called. The member is always named `id`. The field MUST be
  a Rust `String` (ADR 0001 contract item 1). The generator raises a
  `CodegenError` for any other id type.
- **`attributes`** is always present (§5.3).
- **`relationships`** is present when the type declares at least one
  relationship, and absent otherwise (`epics` and `tags` in the example).
- **`links.self`** is always present. It equals the `Location` header on
  create (§8.2), as the spec requires when both exist.
- There is no `meta` member.

### 5.3 Attributes

`attributes` is the entity's serde serialization with the id field and
every relation field removed. Concretely:

- Member names are the Rust field names, in declaration order.
- Every non-relation field is present on every response, including
  `Option` fields, which serialize as `null` when `None`.
- The `#[ontology(body)]` field is an ordinary attribute named after its
  field (`body`).
- Fields with role `Skip` are not attributes.

The generator raises a `CodegenError` when:

- an attribute name is not a legal JSON:API member name (for example, a
  leading or trailing `_`), or is `type` or `id`;
- an entity struct or field carries a serde attribute that changes its
  serialized shape: `rename`, `rename_all`, `alias`, `flatten`, `skip`,
  `skip_serializing`, `skip_serializing_if`, `serialize_with` or `with`.

`#[serde(default)]` and `deserialize_with` change only deserialization, so
they stay allowed. They are the only serde field attributes any in-tree
schema uses.

### 5.4 Relationships

Every field with `FieldRole::Relation` leaves `attributes` and becomes a
relationship:

| Relation kind | Rust field | Relationship name | Arity | Linkage |
|---|---|---|---|---|
| `belongs_to` | `epic_id: Option<String>` | `epic` | to-one | `{"type": "epics", "id": "…"}`, or `null` when `None` |
| `belongs_to` | `workout_id: String` | `workout` | to-one | always an identifier |
| `many_to_many` | `tags: Vec<String>` | `tags` | to-many | array of identifiers, in stored order |
| `has_many` | `subtasks: Vec<String>` | `subtasks` | to-many | array of identifiers, id ascending (ADR 0006 §3) |

**Naming.** A `belongs_to` field whose name ends in `_id` loses that suffix.
Every other relation field keeps its name. The target type is `url_plural`
of the relation's `target` entity.

Relationship names are also URL segments (`/relationships/{rel}`, §9),
used verbatim: `/api/tasks/{id}/relationships/subtasks`, and for a
two-word field, `…/relationships/sub_tasks`.

**Collisions.** The generator raises a `CodegenError` when:

- a relationship name equals an attribute name, `type`, `id` or
  `relationships`;
- two relationships share a name.

This applies to junction-op relationships (§9.1) as well. The spec gives
fields one namespace, and `relationships` is a URL segment.

**Writes.** Every relationship is writable on the wire except a junction-op
relationship (§9.1), which is changed only through its own endpoint.

- **to-one:** set, or cleared to `null` when the field is `Option`.
- **`many_to_many`:** full replacement through `PATCH`; add and remove
  through the relationship endpoint (§9).
- **`has_many`** (decision 9):
  - writes go through the children's foreign keys;
  - a listed child gets its foreign key set to this resource, and moves
    from any previous parent;
  - a child dropped from the list gets its foreign key cleared;
  - when the child's foreign key field is not `Option`, a child cannot be
    dropped. The write fails with `403 {child}_parent_required` (§13.4)
    before anything is written. The spec requires `403` when a server
    refuses a relationship removal or a full replacement;
  - a listed child that does not exist fails a create or update with
    `{Child}NotFound` before anything is written: the first missing id, in
    list order. Over HTTP the step-8 check (§8.2, §8.3) catches it first,
    as `404 related_resource_not_found`. The store's check covers IPC and
    MCP. When a list both names a missing child and drops a required one,
    the missing child is reported.

On the markdown backend a `has_many` write rewrites one file per affected
child. Multi-record writes are best-effort there (ADR 0001 contract
item 2). Only an I/O failure part-way can leave some children changed,
because both checks run before the first write.

**Duplicate identifiers** in a to-many `data` array are collapsed to their
first occurrence before anything else uses the array. This holds for
create and update bodies (§8.2, §8.3) and for relationship `PATCH` (§9),
and matches the add-once rule of relationship `POST`.

**Relationship objects** have members in the order `links`, `data`:

```json
"epic": {
  "links": {
    "self": "/api/tasks/ship-the-emitter/relationships/epic",
    "related": "/api/tasks/ship-the-emitter/epic"
  },
  "data": { "type": "epics", "id": "markdown-backend" }
}
```

- Phase 1b emits `data` only. Phase 3a adds `links`, together with the
  endpoints they point at, because the spec requires a server to serve
  every link it emits.
- A junction-op relationship has no field on the entity, so no `data`.
  From phase 3a it appears with `links` only. Before phase 3a it does not
  appear at all, and it never appears in event frames, which carry no
  links (§12). A relationship object with neither member would be empty,
  which the spec forbids.

### 5.5 Worked example: one task

```json
{
  "type": "tasks",
  "id": "ship-the-emitter",
  "attributes": {
    "title": "Ship the emitter",
    "status": "closed/done",
    "created": "2026-06-06",
    "body": "## Goal\n\nEmit markdown CRUD matching the golden spec. ^summary\n\n## Outcome\n\nMatched on the first conformance run.\n"
  },
  "relationships": {
    "epic": {
      "links": {
        "self": "/api/tasks/ship-the-emitter/relationships/epic",
        "related": "/api/tasks/ship-the-emitter/epic"
      },
      "data": { "type": "epics", "id": "markdown-backend" }
    },
    "tags": {
      "links": {
        "self": "/api/tasks/ship-the-emitter/relationships/tags",
        "related": "/api/tasks/ship-the-emitter/tags"
      },
      "data": [ { "type": "tags", "id": "codegen" } ]
    }
  },
  "links": { "self": "/api/tasks/ship-the-emitter" }
}
```

Below, this object is written `‹task ship-the-emitter›`. The epic and tag
are:

```json
{ "type": "epics", "id": "markdown-backend",
  "attributes": { "title": "Markdown backend", "status": "in-progress",
                  "body": "Implement ADR 0001 as a stacked-PR campaign.\n" },
  "links": { "self": "/api/epics/markdown-backend" } }

{ "type": "tags", "id": "codegen",
  "attributes": { "title": "Codegen" },
  "links": { "self": "/api/tags/codegen" } }
```

These are written `‹epic markdown-backend›` and `‹tag codegen›`.

## 6. Query parameters

Each JSON:API route accepts a fixed set of query parameters, listed below.
Anything else is `400 invalid_query_parameter` with `source.parameter`
naming it. That covers:

- a parameter the route does not accept;
- an unknown member of an accepted family;
- a repeated parameter;
- a parameter whose name is malformed.

The spec requires a `400` for any parameter it reserves that the server
does not support, and for any parameter that follows none of its naming
rules. Ontogen defines one implementation-specific family, `opArg`, and
only on custom ops (§10.2).

| Route | Accepted parameters |
|---|---|
| `GET /api/{type}` | `filter[…]` when the list takes a filter (§7.3); `sort` (§7.4); `include` (§7.5); `page[offset]`, `page[limit]` when paginated (§7.2) |
| `GET /api/{type}/{id}` | `include` (§7.5) |
| `POST /api/{type}`; `PATCH` and `DELETE /api/{type}/{id}` | none |
| `GET /api/{type}/{id}/{rel}` (related link) | `page[offset]`, `page[limit]` for a junction-op relationship in a paginated module (§9.1); otherwise none |
| `GET /api/{type}/{id}/relationships/{rel}` | as for the related link |
| `PATCH`, `POST`, `DELETE /api/{type}/{id}/relationships/{rel}` | none |
| custom op, `GET` | `opArg[…]` (§10.2) |
| custom op, `POST` | none |
| event stream | outside these rules (§12) |

`sort` and `include` are accepted names on the routes listed, in every
phase. A route that cannot honour them answers with their own codes
(§7.4, §7.5), not `invalid_query_parameter`. On every other route they
fall under the general rule.

`fields[…]` (sparse fieldsets) is not supported. It falls under the general
rule and is `400` on every route, because the spec forbids sending fields a
client excluded.

## 7. Collections

### 7.1 List, unpaginated

tasks-tracker as configured today.

```http
GET /api/tasks HTTP/1.1
Accept: application/vnd.api+json
```

```http
HTTP/1.1 200 OK
Content-Type: application/vnd.api+json
Vary: Accept

{
  "jsonapi": { "version": "1.1" },
  "links": { "self": "/api/tasks" },
  "data": [ ‹task ship-the-emitter› ]
}
```

- An empty collection is `"data": []`, still `200`.
- No `meta` and no pagination links: the whole set is in `data`.
- `page[…]` is `400 invalid_query_parameter`, because the list is not
  paginated.
- Order: id ascending, unless `sort` is given (ADR 0006 §3).

### 7.2 List, paginated

With `default_limit: 20` and `max_limit: 100` on `task`. Assume the vault
holds 45 tasks for this section.

```http
GET /api/tasks?page[offset]=20&page[limit]=10 HTTP/1.1
Accept: application/vnd.api+json
```

```http
HTTP/1.1 200 OK

{
  "jsonapi": { "version": "1.1" },
  "links": {
    "self":  "/api/tasks?page%5Boffset%5D=20&page%5Blimit%5D=10",
    "first": "/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=10",
    "prev":  "/api/tasks?page%5Boffset%5D=10&page%5Blimit%5D=10",
    "next":  "/api/tasks?page%5Boffset%5D=30&page%5Blimit%5D=10",
    "last":  "/api/tasks?page%5Boffset%5D=40&page%5Blimit%5D=10"
  },
  "meta": { "total": 45, "limit": 10, "offset": 20 },
  "data": [ "…10 resource objects…" ]
}
```

**Parameters.** The strategy is offset-based, using `page[offset]` and
`page[limit]` from the reserved `page` family. These are the same
semantics as today's `limit`/`offset`, so the store signature is unchanged.

| Input | Effective value |
|---|---|
| `page[limit]` absent | `default_limit` |
| `page[limit]` > `max_limit` | `max_limit`, silently (same clamp as today) |
| `page[limit]=0` | `400` |
| `page[offset]` absent | `0` |
| `page[offset]` ≥ total | allowed; `data` is `[]` |
| a value that is not one or more ASCII digits, or is above `4294967295` (`-1`, `+5`, empty, `1.5`, `ten`) | `400` |
| `page[number]`, `page[size]`, `page[cursor]` or any other `page[…]` | `400` |

Leading zeros are accepted (`page[offset]=020` is 20). Links always write
the effective value in plain decimal.

All of these `400`s use code `invalid_query_parameter`, with
`source.parameter` naming the parameter, e.g. `"page[limit]"`.

**`meta`.** `total` is the filter-aware `count` (https://github.com/sksizer/rust-ontogen/pull/172), so it counts the
filtered set, not the table. `limit` and `offset` are the effective values.
Together they are exactly the fields of today's `PaginatedResult`, which
lets the TS transport rebuild it (§14).

**Links.** A paginated document carries `self` plus the four pagination
links (`first`, `prev`, `next`, `last`). All five keys are always present;
a pagination link that is unavailable is `null`. Every link carries the
request's `filter`, `sort` and `include` in canonical form (§4.3).

With effective limit `L`, offset `O` and total `T`, and
`last_offset = T == 0 ? 0 : floor((T - 1) / L) * L`:

| Link | Offset | `null` when |
|---|---|---|
| `self` | `O` | never |
| `first` | `0` | never |
| `prev` | `last_offset` when `O >= T`, otherwise `max(0, O - L)` | `O == 0` |
| `next` | `O + L` | `O + L >= T` |
| `last` | `last_offset` | never |

An offset at or past the end gets a `prev` pointing at the last page, so a
client that overshoots steps straight back to real data. With `T = 45` and
`L = 10`, `O = 45` gives `prev` offset 40. An offset inside the last page
(`O = 42`) gives `prev` offset 32, the page just before it.

No link carries an offset a client cannot request, that is, one above the
`page[offset]` ceiling of `4294967295`. In a collection that large, `last`
is capped at the last multiple of `L` at or below `4294967295`, and `next`
is `null` when it would start beyond that.

**Ordering.** Pages are cut from the ADR 0006 §3 order: the requested
`sort`, then id ascending as the final tie-break, or id ascending alone.
Page boundaries are therefore stable while the data is unchanged. Phase 1a
delivers the id-ascending default on both backends (ADR 0006 §6), before
phase 1b ships this envelope.

**Consistency.** `count` and the page query are separate store calls, as
today, so a concurrent write can make `total` disagree with the pages by
the size of that write. The contract does not promise a snapshot.

**Edge examples:**

- `GET /api/tasks` returns offset 0 and limit 20.
  - `self` is `/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=20`.
  - `prev` is `null`.
  - `next` is `…offset%5D=20…` and `last` is `…offset%5D=40…`.
- `GET /api/tasks?page[limit]=500` is clamped to 100.
  - `meta.limit` is `100` and `next` is `null`.
  - `last` is `…offset%5D=0&page%5Blimit%5D=100`.
- For an empty vault, `T = 0`.
  - `data` is `[]` and `meta.total` is `0`.
  - `first` and `last` are offset 0; `prev` and `next` are `null`.
- `GET /api/tasks?page[limit]=-1` is `400`:

```json
{
  "jsonapi": { "version": "1.1" },
  "errors": [ {
    "status": "400",
    "code": "invalid_query_parameter",
    "title": "Bad Request",
    "detail": "page[limit] must be an integer between 1 and 4294967295",
    "source": { "parameter": "page[limit]" }
  } ]
}
```

### 7.3 Filter

Phase 2. Filters are hand-written: the store takes none, and the generated
CRUD `list` takes none.

**A hand-written list replaces the generated one.** A `list` written in
`api_dir/{module}.rs` replaces the generated `list` for that module, and
`gen_api` stops emitting it. When the module is paginated, the generated
`count` is replaced the same way. Today the API stage keeps the generated fn
over a scanned fn of the same name (the merge in `src/api/mod.rs`), and
phase 2 reverses that precedence for `list` and `count`. The hand-written
list's parameters are, in this order:

1. the store;
2. its filter parameters;
3. `order: &[OrderBy<XSortField>]`, from phase 3c, if it supports `sort`;
4. `limit`, `offset`, if paginated.

The matching `count` takes the same filter parameters and no `order`
(ADR 0006 §1).

**Filter parameters on the wire.** A filter parameter that is a
user-authored `*Query` struct is read from the `filter` family. Each
`filter[name]` parameter becomes the struct field `name`, deserialized by
serde from the string value, exactly as `axum::extract::Query` does today.
A bare filter parameter (e.g. `skill_id: &str`) becomes `filter[skill_id]`.

That also fixes a defect. Today a bare parameter is extracted as
`Query<String>`, which cannot deserialize from a query map, so every such
request fails: `?skill_id=abc` returns `400 invalid type: map, expected a
string`.

With tasks-tracker's phase-2 `ListTasksQuery { status: Option<String>, epic_id: Option<String> }`:

```http
GET /api/tasks?filter[status]=closed/done&filter[epic_id]=markdown-backend HTTP/1.1
Accept: application/vnd.api+json
```

```http
HTTP/1.1 200 OK

{
  "jsonapi": { "version": "1.1" },
  "links": {
    "self":  "/api/tasks?filter%5Bepic_id%5D=markdown-backend&filter%5Bstatus%5D=closed%2Fdone&page%5Boffset%5D=0&page%5Blimit%5D=20",
    "first": "/api/tasks?filter%5Bepic_id%5D=markdown-backend&filter%5Bstatus%5D=closed%2Fdone&page%5Boffset%5D=0&page%5Blimit%5D=20",
    "prev":  null,
    "next":  null,
    "last":  "/api/tasks?filter%5Bepic_id%5D=markdown-backend&filter%5Bstatus%5D=closed%2Fdone&page%5Boffset%5D=0&page%5Blimit%5D=20"
  },
  "meta": { "total": 1, "limit": 20, "offset": 0 },
  "data": [ ‹task ship-the-emitter› ]
}
```

Rules:

- Filter member names are the struct's field names, not relationship
  names. The filter above is `filter[epic_id]`, not `filter[epic]`,
  because the struct is user-authored and ontogen does not rewrite it.
- Each of these is `400 invalid_query_parameter` naming the parameter:
  - a `filter[x]` that the struct does not deserialize. Serde ignores
    unknown fields by default, so the generated code detects them (for
    example with `serde_ignored`) instead of trusting the struct;
  - a value serde rejects;
  - a missing required filter (a non-`Option` field or bare parameter);
  - a nested or array form (`filter[x][]`, `filter[x][y]`, `filter[x.y]`);
  - any `filter[…]` on a list that takes no filter.
- The generated CRUD `list` takes no filter. Generic field filters are not
  part of this epic.

### 7.4 Sort

Phase 3c. `sort` is honoured on a list whose API fn takes an
`order: &[OrderBy<{Entity}SortField>]` argument (ADR 0006 §1):

- The generated CRUD `list` always takes one.
- A hand-written list (§7.3) takes one when its author adds it, and orders
  with the store's `sort_{plural}` or `order_{plural}_query` helpers so the
  order rules hold.

tasks-tracker's phase-2 list gains it in phase 3c.

Sort fields are `id` plus every scalar attribute (ADR 0006 §2). `id` names
the `#[ontology(id)]` field whatever it is called. Relationship names and
dotted paths are not sort fields.

For `tasks` the sort fields are `id`, `title`, `status` and `created`.
`body` is excluded, `epic` is a relationship, and `tags` is not a scalar.

```http
GET /api/tasks?sort=-created,title&filter[status]=open/ready HTTP/1.1
Accept: application/vnd.api+json
```

The response holds the tasks with status `open/ready`, ordered by `created`
descending, then `title` ascending, then `id` ascending (the implicit final
key). Its `self` is
`/api/tasks?filter%5Bstatus%5D=open%2Fready&sort=-created,title&page%5Boffset%5D=0&page%5Blimit%5D=20`.

Rules:

- Keys apply in the order given. A leading `-` means descending; otherwise
  ascending.
- `id` is appended as the last key, ascending, unless the request names
  `id`, in which case the requested direction stands.
- `null` sorts before every value in ascending order and after every value
  in descending order. Strings compare by byte. ADR 0006 §3 fixes both
  rules for parity between the backends.
- The value is split on `,` and parsed by `ontogen_core::order::parse_sort`,
  the same function IPC and MCP use (decision 8).
- Each of these is `400` with code `invalid_sort_field` and
  `source.parameter: "sort"`:
  - an unknown field (`sort=priority`), a relationship (`sort=epic`) or a
    dotted path (`sort=epic.title`);
  - a field named twice (`sort=title,-title`);
  - an empty item (`sort=title,,status`, `sort=`);
  - any `sort` on a list that takes no `order` argument, including every
    list before phase 3c. The spec requires `400` from a server that does
    not support the requested sort.

```json
{
  "status": "400",
  "code": "invalid_sort_field",
  "title": "Bad Request",
  "detail": "`priority` is not a sort field of `tasks`; sort fields are: id, title, status, created",
  "source": { "parameter": "sort" }
}
```

### 7.5 Include

Phase 3b. `include` is honoured on the two routes that accept it (§6):
`GET /api/{type}` and `GET /api/{type}/{id}`. Every relationship of the
primary type except a junction-op relationship can be included, to-one and
to-many alike, one level deep.

```http
GET /api/tasks?include=epic,tags HTTP/1.1
Accept: application/vnd.api+json
```

```http
HTTP/1.1 200 OK

{
  "jsonapi": { "version": "1.1" },
  "links": { "self": "/api/tasks?include=epic,tags&page%5Boffset%5D=0&page%5Blimit%5D=20",
             "first": "…", "prev": null, "next": null, "last": "…" },
  "meta": { "total": 1, "limit": 20, "offset": 0 },
  "data": [ ‹task ship-the-emitter› ],
  "included": [ ‹epic markdown-backend›, ‹tag codegen› ]
}
```

Rules:

- `included` holds each distinct `(type, id)` once. It is ordered by
  relationship in `include` order, then by first appearance in `data`
  order.
- A resource already present in `data` is not repeated in `included` (the
  self-referential `parent` case). The spec forbids two objects for one
  `(type, id)`.
- Included resources are fetched with the target entity's store `get`, one
  call per distinct id.
- A linkage id that the store reports as not found (a dangling markdown
  wikilink) is left out of `included`. Its identifier stays in the linkage.
  This is not an error, because the markdown backend tolerates dangling
  links by design (ADR 0001 amendment 5). Any other store error fails the
  request.
- On a paginated list, only the page's relationships are included.
- `include=` (empty value) is `"included": []`. The spec requires the
  member whenever `include` is given.
- `include=epic,epic` is the same as `include=epic`.
- Each of these is `400 invalid_include_path` with
  `source.parameter: "include"`:
  - a name that is not an includable relationship of the primary type
    (`include=owner`, or a junction-op relationship);
  - a dotted path (`include=epic.tasks`), since nested inclusion is out of
    scope;
  - any `include` before phase 3b.
- On every other route, `include` is not an accepted parameter, and is
  `400 invalid_query_parameter` (§6).

Cost: one store `get` per distinct included id. That is acceptable at the
page sizes `max_limit` allows.

## 8. Single resources

Error tables in this section list, for each check, its step in the check
order of §13.2. Within a step, rows apply in table order.

### 8.1 Get

```http
GET /api/tasks/ship-the-emitter HTTP/1.1
Accept: application/vnd.api+json
```

```http
HTTP/1.1 200 OK

{
  "jsonapi": { "version": "1.1" },
  "links": { "self": "/api/tasks/ship-the-emitter" },
  "data": ‹task ship-the-emitter›
}
```

With `?include=epic`, the `self` link is
`/api/tasks/ship-the-emitter?include=epic`, and
`"included": [ ‹epic markdown-backend› ]` follows `data`.

**Errors.**

- A path `{id}` is a lookup key and is not checked against the
  id-validity rule (§8.2). An id that does not exist, including one that
  could never be created, is `404` from the store (step 9).
- A query parameter other than `include` is `400 invalid_query_parameter`;
  a bad `include` value is `400 invalid_include_path` (step 5).
- When the id does not exist (`GET /api/tasks/nope`), the store returns
  `AppError::TaskNotFound("nope")` and the response is `404` (step 9):

```json
{
  "jsonapi": { "version": "1.1" },
  "errors": [ {
    "status": "404",
    "code": "task_not_found",
    "title": "Not Found",
    "detail": "Task not found: nope"
  } ]
}
```

### 8.2 Create

`POST /api/{type}`, body is a resource object as primary data.

**Without an id** (filled by `IdStrategy`; tasks-tracker slugs the title):

```http
POST /api/tasks HTTP/1.1
Content-Type: application/vnd.api+json
Accept: application/vnd.api+json

{
  "data": {
    "type": "tasks",
    "attributes": {
      "title": "Review the stack",
      "status": "open/ready",
      "created": "2026-06-06",
      "body": "## Goal\n\nReview.\n"
    },
    "relationships": {
      "epic": { "data": { "type": "epics", "id": "markdown-backend" } },
      "tags": { "data": [ { "type": "tags", "id": "codegen" } ] }
    }
  }
}
```

```http
HTTP/1.1 201 Created
Location: /api/tasks/review-the-stack
Content-Type: application/vnd.api+json
Vary: Accept

{
  "jsonapi": { "version": "1.1" },
  "links": { "self": "/api/tasks/review-the-stack" },
  "data": {
    "type": "tasks",
    "id": "review-the-stack",
    "attributes": { "title": "Review the stack", "status": "open/ready",
                    "created": "2026-06-06", "body": "## Goal\n\nReview.\n" },
    "relationships": {
      "epic": { "links": { "…": "…" }, "data": { "type": "epics", "id": "markdown-backend" } },
      "tags": { "links": { "…": "…" }, "data": [ { "type": "tags", "id": "codegen" } ] }
    },
    "links": { "self": "/api/tasks/review-the-stack" }
  }
}
```

**With a client id**, `"id": "review-q3"` beside `"type"`, the response is
`201` with `Location: /api/tasks/review-q3`. A client id is honoured under
every `IdStrategy` (decision 4), so the server never answers `403` for a
client-generated id.

The spec asks *clients* to generate globally unique ids, preferably UUIDs.
Ontogen accepts any valid id:

- **Validity.** One rule binds both backends. The HTTP handler sits above
  the store and must be byte-identical across backends
  (`tests/backend_parity.rs`), so SeaORM ids are held to it too. It matches
  `markdown_store::layout::validate_id`. An id is valid when it:
  - is a non-empty string that is not whitespace-only;
  - contains no `/`, `\`, `:` or NUL;
  - does not start with `.`;
  - does not end with `.` or a space;
  - is not `index` or `log`, compared case-insensitively. OKF reserves
    those stems ([ADR 0005](architecture/0005-okf-markdown-vaults.md)).

  `validate_id` does not yet reject a whitespace-only id (it accepts
  `"\t"`). Phase 1a adds that rule to it, so the store and the handler
  agree.
- **Derived ids.** A derived slug that would be reserved dedupes like a
  collision, to `index-2`, per ADR 0005.
- **Scope.** The rule governs ids being created: a client `data.id`, and a
  derived or hook-assigned id, which the store checks. A path `{id}` is
  only a lookup key and is never checked against it (§8.1). The server
  must serve every link it emits, and a SeaORM row created before the rule
  (an id containing `:`, or `Index`, say) is still listed. It stays
  readable, updatable and deletable at its `links.self`. It can no longer
  be created under that id. The exception is ids `.`, `..` and `""`: they
  are listed but unreachable, because clients resolve the dot segments of
  `/tasks/.` and `/tasks/..` away (RFC 3986 §5.2.4; WHATWG URL parsing
  also decodes `%2E`), and `/tasks/` is the collection path. Such rows
  must be renamed before upgrading. ADR 0004 carries the migration note.
- **Markdown lookups.** On markdown such an id cannot exist. The store
  answers a lookup of one with `{Entity}NotFound` (phase 1a) instead of
  passing `markdown_store::Error::InvalidId` through as a `500`.
- **An invalid client id is `400`, not `403`.** The spec's `403` is for a
  server that does not support client ids, and this one does.
- **Uniqueness** within the type is the store's `409`. The id's format is
  otherwise the consumer's choice; slugs are the norm in markdown vaults.

The response is always `201` with the full resource, never `204`. The
server can change the resource (the id fill, `before_create` hooks).

**Body mapping:**

- `data.id` becomes `CreateXInput.id`, or `""` when absent, which is
  today's "derive one" signal.
- `attributes` become the remaining non-relation fields.
- Each to-one `relationships.x.data` becomes the foreign-key field
  (`epic_id`).
- Each to-many `relationships.x.data` becomes the id vector (`tags`,
  `subtasks`). A `has_many` vector sets those children's foreign keys
  (§5.4).
- A relationship absent from the body takes the field's default (`None`
  or empty), as today's `#[serde(default)]` does. A non-`Option` to-one has
  no default, so it cannot be omitted.

**Errors**, by step of §13.2:

| Step | Condition | Status | `code` | `source` |
|---|---|---|---|---|
| 2 | `Accept` not satisfiable | 406 | `not_acceptable` | `header: "Accept"` |
| 3 | `Content-Type` not acceptable | 415 | `unsupported_media_type` | `header: "Content-Type"` |
| 5 | any query parameter | 400 | `invalid_query_parameter` | `parameter` |
| 7 | body is not JSON, or its top level is not an object | 400 | `invalid_document` | none |
| 7 | `data` missing | 400 | `invalid_document` | `pointer: ""` |
| 7 | `data` not an object | 400 | `invalid_document` | `pointer: "/data"` |
| 7 | `data.type` missing | 400 | `invalid_document` | `pointer: "/data"` |
| 7 | `data.type` not a string | 400 | `invalid_document` | `pointer: "/data/type"` |
| 7 | `data.type` is not this collection's type (`"epics"` posted to `/api/tasks`) | 409 | `type_mismatch` | `pointer: "/data/type"` |
| 7 | `data.id` present but not a string, or not a valid id | 400 | `invalid_document` | `pointer: "/data/id"` |
| 7 | `data.lid` present (unsupported) | 400 | `invalid_document` | `pointer: "/data/lid"` |
| 7 | `attributes` present but not an object | 400 | `invalid_document` | `pointer: "/data/attributes"` |
| 7 | an attribute name the type does not have, including a relation field written as an attribute (`epic_id`) | 400 | `unknown_attribute` | `pointer: "/data/attributes/{name}"` |
| 7 | a required attribute missing | 400 | `missing_attribute` | `pointer: "/data/attributes"`, or `"/data"` when `attributes` is absent |
| 7 | an attribute value serde rejects | 400 | `invalid_attribute` | `pointer: "/data/attributes/{name}"` |
| 7 | `relationships` present but not an object | 400 | `invalid_document` | `pointer: "/data/relationships"` |
| 7 | a relationship name the type does not have | 400 | `unknown_relationship` | `pointer: "/data/relationships/{name}"` |
| 7 | a junction-op relationship present (§9.1) | 403 | `relationship_update_unsupported` | `pointer: "/data/relationships/{name}"` |
| 7 | a relationship object that is not an object or has no `data`; `data` of the wrong arity; an identifier without string `type` and `id` | 400 | `invalid_document` | `pointer` at the offending value: `…/{name}`, `…/{name}/data` or `…/{name}/data/{i}` |
| 7 | `data: null` on a non-`Option` to-one | 403 | `relationship_required` | `pointer: "/data/relationships/{name}/data"` |
| 7 | a non-`Option` to-one absent | 400 | `missing_relationship` | `pointer: "/data/relationships"`, or `"/data"` when `relationships` is absent |
| 7 | an identifier of the wrong type | 409 | `type_mismatch` | `pointer: "/data/relationships/{name}/data"` (or `…/data/{i}`) |
| 8 | a linked resource that does not exist | 404 | `related_resource_not_found` | the identifier's pointer |
| 9 | no id after `before_create` and the `IdStrategy` (`Provided` with no id; a slug source that slugifies to empty) | 400 | `{entity}_id_required` | none |
| 9 | the id already exists | 409 | `{entity}_already_exists` | `pointer: "/data/id"` when the request carried `data.id`; otherwise none |
| 9 | any other `AppError` | per §13.4 | §13.4 | none |

Notes:

- **Member order within step 7** follows §13.2 step 7. Within one
  declared relationship the order is its shape, then `null`, then absence,
  then identifier types.
- **Pointers** only ever name a value present in the request, as the spec
  requires. For something missing, a pointer names the nearest enclosing
  member that exists.
- **Unknown attributes are refused, not ignored.** The commonest cause is a
  client still sending the flat shape. The `detail` says so: "`epic_id` is
  not an attribute of `tasks`; it is the `epic` relationship".
- **Linked-resource checks (step 8).** The handler checks every linked id
  with the target's store `get` before creating, in step-7 order. The spec
  requires `404` for a reference to a resource that does not exist, and the
  markdown backend would otherwise write a dangling wikilink.
- **No id (step 9)** is detected by the store, after `before_create` hooks
  have run, so a hook may assign the id. The store returns
  `AppError::{Entity}IdRequired(reason)`. The handler does not know the
  `IdStrategy` and does not pre-check.
- **Duplicates (step 9)** are detected by the store as
  `AppError::{Entity}AlreadyExists(id)`:
  - on markdown, from `markdown_store::Error::AlreadyExists`;
  - on SeaORM, from a unique-constraint violation on the primary-key
    insert.

  A derived id never reaches this error:
  - markdown already probes `-2`, `-3` and so on;
  - phase 1a gives SeaORM the same probe, and makes a derived-id insert
    that loses a race retry with the next suffix.

  A client id, or an id a hook assigned, can collide. Only the first
  carries a pointer, because only it is in the request.

Example: `POST /api/tasks` with `"type": "epics"`:

```json
{
  "jsonapi": { "version": "1.1" },
  "errors": [ {
    "status": "409",
    "code": "type_mismatch",
    "title": "Conflict",
    "detail": "`epics` cannot be created at /api/tasks, which holds `tasks`",
    "source": { "pointer": "/data/type" }
  } ]
}
```

### 8.3 Update

`PATCH /api/{type}/{id}`. `PUT` is not routed and answers `405` (§13.5).

```http
PATCH /api/tasks/ship-the-emitter HTTP/1.1
Content-Type: application/vnd.api+json
Accept: application/vnd.api+json

{
  "data": {
    "type": "tasks",
    "id": "ship-the-emitter",
    "attributes": { "status": "open/ready" },
    "relationships": {
      "epic": { "data": null },
      "tags": { "data": [] }
    }
  }
}
```

```http
HTTP/1.1 200 OK

{
  "jsonapi": { "version": "1.1" },
  "links": { "self": "/api/tasks/ship-the-emitter" },
  "data": {
    "type": "tasks",
    "id": "ship-the-emitter",
    "attributes": { "title": "Ship the emitter", "status": "open/ready",
                    "created": "2026-06-06", "body": "## Goal\n…" },
    "relationships": {
      "epic": { "links": { "…": "…" }, "data": null },
      "tags": { "links": { "…": "…" }, "data": [] }
    },
    "links": { "self": "/api/tasks/ship-the-emitter" }
  }
}
```

Semantics, which map one to one onto `UpdateXInput` and its `double_option`
fields:

| In the body | `UpdateXInput` field | Effect |
|---|---|---|
| attribute absent | `None` | unchanged |
| attribute with a value | `Some(v)` / `Some(Some(v))` | set |
| attribute `null`, field is `Option<T>` | `Some(None)` | cleared |
| attribute `null`, field is not `Option` | — | `400 invalid_attribute` |
| relationship absent | `None` | unchanged |
| to-one `data: {type, id}` | `epic_id: Some(Some(id))` | set |
| to-one `data: null`, field `Option<String>` | `epic_id: Some(None)` | cleared |
| to-one `data: null`, field `String` | — | `403 relationship_required` |
| `many_to_many` `data: [...]` | `tags: Some(vec)` | full replacement; `[]` clears |
| `has_many` `data: [...]` | `subtasks: Some(vec)` | full replacement through the children's foreign keys (§5.4) |

An update with no `attributes` and no `relationships` is a no-op. It
returns `200` with the current resource, and the markdown backend performs
no write.

The response is always `200` with the full resource: `before_update` hooks
may change fields the request did not mention, and the spec then requires
`200`.

**Errors**, by step of §13.2:

| Step | Condition | Status | `code` | `source` |
|---|---|---|---|---|
| 2, 3 | as in §8.2 | | | |
| 5 | as in §8.2 | | | |
| 7 | §8.2's step-7 rows up to and including `type_mismatch` | | | |
| 7 | `data.id` missing | 400 | `invalid_document` | `pointer: "/data"` |
| 7 | `data.id` not a string | 400 | `invalid_document` | `pointer: "/data/id"` |
| 7 | `data.id` differs from the URL id | 409 | `id_mismatch` | `pointer: "/data/id"` |
| 7 | §8.2's remaining step-7 rows, except its `data.id` row (replaced by the three above), `missing_attribute` and `missing_relationship` | | | |
| 8 | a linked resource that does not exist | 404 | `related_resource_not_found` | the identifier's pointer |
| 9 | the resource does not exist | 404 | `{entity}_not_found` | none |
| 9 | a `has_many` replacement drops a child whose foreign key is not `Option` | 403 | `{child}_parent_required` | none |
| 9 | any other `AppError` | per §13.4 | §13.4 | none |

A body `id` is compared with the URL id exactly. A string that differs is
`409`, whatever it contains (`""` included), as the spec requires for an id
that does not match the endpoint. Neither the body id nor the URL `{id}` is
checked against the validity rule on update; it applies only to ids being
created.

A missing resource is detected by the store, after the body checks. So a
`PATCH` to a missing id with a malformed body gets the body's `400`, not
`404`.

Example, `PATCH /api/tasks/ship-the-emitter` with `"id": "other"`:

```json
{
  "jsonapi": { "version": "1.1" },
  "errors": [ {
    "status": "409",
    "code": "id_mismatch",
    "title": "Conflict",
    "detail": "body id `other` does not match URL id `ship-the-emitter`",
    "source": { "pointer": "/data/id" }
  } ]
}
```

### 8.4 Delete

```http
DELETE /api/tasks/ship-the-emitter HTTP/1.1
Accept: application/vnd.api+json
```

```http
HTTP/1.1 204 No Content
Vary: Accept
```

- A missing resource is `404 task_not_found`. The spec's SHOULD is taken.
- A request body is ignored.
- Query parameters are `400`.

## 9. Relationship endpoints and related links

Phase 3a. For every relationship of every resource type the server serves:

| Method | Path | To-one | `many_to_many` | `has_many` | Junction op |
|---|---|---|---|---|---|
| `GET` | `/api/{type}/{id}/relationships/{rel}` | linkage | linkage | linkage | linkage |
| `PATCH` | same | set or clear | replace | replace | `403` |
| `POST` | same | `403` | add | add | add |
| `DELETE` | same | `403` | remove | remove | remove |
| `GET` | `/api/{type}/{id}/{rel}` (related link) | resource or `null` | resource collection | resource collection | resource collection |

- **`403`s in the table.** The spec defines `POST` and `DELETE` only for
  to-many relationships, and requires `403` for an unsupported
  relationship update. Every `403` in the table uses code
  `relationship_update_unsupported`.
- **One identifier per `POST` or `DELETE`.** A body with more than one is
  `403 relationship_batch_unsupported`. `"data": []` is a successful no-op.
  The spec allows `403` for an unsupported relationship update. Allowing
  only one identifier makes every relationship write a single store or
  junction call, which either succeeds or fails, so no request can be
  partly applied. The TS transport sends one id per request.
- **`{rel}` is captured.** It is the relationship name verbatim (§5.4).
  Both route templates capture it, so a name that is not a relationship of
  `{type}` reaches the generated handler and is
  `404 relationship_not_found`. It does not fall through to the consumer's
  router.
- **Other methods** are `405`, with `Allow: GET, HEAD, PATCH, POST, DELETE`
  on the relationship route and `Allow: GET, HEAD` on the related route.
  That holds whatever `{rel}` names, because routing happens before `{rel}`
  is looked up.

### 9.1 Where each endpoint's behaviour comes from

**Relation fields** (`belongs_to`, `many_to_many`, `has_many` on the
entity) are served from the generated store alone, so they work on both
backends with no user code:

- `GET …/relationships/{rel}` reads the field from `get_{entity}(id)`.
- `PATCH` writes the field through
  `update_{entity}(id, XUpdate { field: Some(…), ..Default })`, with the
  semantics of §8.3.
- `POST` (to-many) reads the current ids. If the requested id is absent, it
  appends it and writes through `update_{entity}`. If it is present, it
  writes nothing.
- `DELETE` (to-many) reads the current ids. If the requested id is present,
  it removes it and writes through `update_{entity}`. If it is absent, it
  writes nothing. For `has_many`, removing a child clears its foreign key,
  or fails with `403 {child}_parent_required` (§5.4).

`POST` and `DELETE` are a read followed by one write, not one transaction.
A concurrent writer to the same relationship can lose an update. That
matches every other read-modify-write in the store today.

**Junction ops** are user-authored functions in a resource module:

- `list_X(parent_id)`, classified `JunctionList`;
- `add_Y(parent_id, child_id)`, classified `JunctionAdd`;
- `remove_Y(parent_id, child_id)`, classified `JunctionRemove`;

where `Y` is the singular of `X`. Together they define a to-many
relationship named `X`, exactly as written in the fn name: `list_tags` /
`add_tag` / `remove_tag` define `tags`, and `list_sub_tasks` defines
`sub_tasks`.

- **Classification.** From phase 3a, a one-parameter `list_X` counts as a
  junction op only when its module also has `add_Y` or `remove_Y`. Without
  either it is a custom `GET` (§10), served at
  `/api/{type}/{action}/{param}`. Today any one-parameter `list_*` is a
  `JunctionList`, which would turn a plain `list_by_status(status)` into a
  relationship named `by_status`.
- **Outside a resource module.** Junction ops in a module that is not a
  resource module (§5.1) have no resource type to hang a relationship on.
  This includes today's test fixture `destination_skills`. They are served
  as custom ops (§10.4).
- **Target type.** The relationship's target type is `url_plural` of
  `list_X`'s element type when that is an entity. Otherwise it is the
  entity type whose `url_plural` is `X` kebab-cased. When neither names an
  entity type, the generator raises a `CodegenError`.
- **Collisions.** The name collision rule of §5.4 applies to junction-op
  relationships.

Each junction endpoint behaves as follows:

- **`GET …/relationships/X`** calls `list_X(parent_id)`. When it returns
  entities, linkage is built from their ids. When it returns `Vec<String>`,
  linkage is built from the strings.
- **`POST`** validates the identifier's shape and type, and that its target
  exists. It then calls `list_X(parent_id)`. If the id is already a member,
  the response is `204` without calling user code; otherwise it calls
  `add_Y(parent_id, child_id)`.
- **`DELETE`** validates the identifier's shape and type only, not its
  target's existence, so a member whose target was deleted can still be
  removed. It then calls `list_X(parent_id)`. If the id is not a member, the
  response is `204` without calling user code; otherwise it calls
  `remove_Y(parent_id, child_id)`.
- **`PATCH`** is `403 relationship_update_unsupported`, because no junction
  op replaces a set.

The membership read is what makes a repeated `POST`, or a `DELETE` of an
absent member, succeed as the spec requires, whatever `add_Y` does with a
duplicate.

**Pagination.** When the module is paginated, junction `GET`s keep today's
in-memory paging:

- The relationship linkage `GET` and the related-link `GET` accept
  `page[offset]` and `page[limit]` with §7.2's rules.
- They return `meta {total, limit, offset}`, plus `self` and the four
  pagination links at the top level, where they paginate the primary data:
  the relationship's members.
- Relation-field relationships are never paginated. Their linkage is
  already loaded with the resource.

**In resource objects.** A junction-op relationship appears in the
resource object's `relationships` with `links` only and no `data`, which
the spec allows. Linkage would mean calling user code once per resource on
every `get` and `list`. It follows that:

- it cannot be included (§7.5);
- it cannot be written in a create or update body (§8.2,
  `403 relationship_update_unsupported`);
- the TS flattener leaves it out of the flat entity, as today, since the
  entity struct has no such field.

**Before phase 3a**, junction ops in a resource module keep their current
paths and are served as custom ops (§10.4).

### 9.2 Examples and errors

**Fetch to-one linkage:**

```http
GET /api/tasks/ship-the-emitter/relationships/epic HTTP/1.1
Accept: application/vnd.api+json
```

```http
HTTP/1.1 200 OK

{
  "jsonapi": { "version": "1.1" },
  "links": { "self": "/api/tasks/ship-the-emitter/relationships/epic",
             "related": "/api/tasks/ship-the-emitter/epic" },
  "data": { "type": "epics", "id": "markdown-backend" }
}
```

An empty to-one is `"data": null`, still `200`.

**Fetch to-many linkage.** `GET /api/tasks/ship-the-emitter/relationships/tags`
returns `200` with `links.self` and `links.related`, and
`"data": [ { "type": "tags", "id": "codegen" } ]`. An empty to-many is
`"data": []`.

**Set and clear to-one:**

```http
PATCH /api/tasks/ship-the-emitter/relationships/epic HTTP/1.1
Content-Type: application/vnd.api+json

{ "data": { "type": "epics", "id": "markdown-backend" } }
```

```http
HTTP/1.1 204 No Content
Vary: Accept
```

`{ "data": null }` clears it, also `204`.

**Add, remove and replace to-many.** With the phase-3a `release` tag in
the vault:

```http
POST /api/tasks/ship-the-emitter/relationships/tags HTTP/1.1
Content-Type: application/vnd.api+json

{ "data": [ { "type": "tags", "id": "release" } ] }
```

- The response is `204`. Posting an id already present is a no-op and
  still `204`.
- `DELETE` with the same body removes `release`. Removing an absent id is
  also `204`.
- `PATCH` with `{"data": []}` clears the relationship.
- `PATCH /api/tasks/ship-the-emitter/relationships/subtasks` with
  `{"data": [...]}` makes exactly those tasks the subtasks. Dropped
  subtasks get `parent_id: null`.

**Responses.** Every successful relationship mutation is `204 No Content`.
The server makes no change beyond the request, so the spec allows it.

**Errors**, by step of §13.2:

| Step | Condition | Status | `code` | `source` |
|---|---|---|---|---|
| 1 | a method other than `GET`, `HEAD`, `PATCH`, `POST`, `DELETE` | 405 | `method_not_allowed` | none |
| 2 | `Accept` not satisfiable | 406 | `not_acceptable` | `header: "Accept"` |
| 3 | `Content-Type` not acceptable (`PATCH`, `POST`, `DELETE`) | 415 | `unsupported_media_type` | `header: "Content-Type"` |
| 4 | `{rel}` is not a relationship of `{type}` | 404 | `relationship_not_found` | none |
| 5 | any query parameter not accepted by §6 | 400 | `invalid_query_parameter` | `parameter` |
| 6 | a write the relationship does not support (table above) | 403 | `relationship_update_unsupported` | none |
| 7 | body not JSON, or top level not an object | 400 | `invalid_document` | none |
| 7 | `data` missing | 400 | `invalid_document` | `pointer: ""` |
| 7 | `data` of the wrong arity for the relationship | 400 | `invalid_document` | `pointer: "/data"` |
| 7 | `POST` or `DELETE` with more than one identifier | 403 | `relationship_batch_unsupported` | `pointer: "/data"` |
| 7 | an identifier without string `type` and `id` | 400 | `invalid_document` | `pointer: "/data"` (to-one) or `"/data/{i}"` |
| 7 | `null` on a non-`Option` to-one | 403 | `relationship_required` | `pointer: "/data"` |
| 7 | an identifier of the wrong type | 409 | `type_mismatch` | the identifier's pointer |
| 8 | parent resource missing | 404 | `{entity}_not_found` | none |
| 8 | a linked resource that does not exist (`PATCH`, `POST`; never `DELETE`) | 404 | `related_resource_not_found` | the identifier's pointer |
| 9 | a `has_many` write drops a child whose foreign key is not `Option` | 403 | `{child}_parent_required` | none |
| 9 | any other `AppError` from the store or a junction op | per §13.4 | §13.4 | none |

The `data missing` row is where E0003 phase 1's "missing junction
parameter → 400" lands: a junction add without its child id is a body with
no `data`.

### 9.3 Related resource links

`GET /api/tasks/ship-the-emitter/epic` returns
`{ "jsonapi", "links": { "self": "/api/tasks/ship-the-emitter/epic" }, "data": ‹epic markdown-backend› }`.

- An empty to-one is `"data": null`.
- `GET /api/tasks/ship-the-emitter/tags` returns
  `"data": [ ‹tag codegen› ]`, ordered as the linkage.
- Resources are loaded with the target's store `get`, one call per id.
- A dangling id is skipped (§7.5). A to-one whose target is missing is
  `"data": null`.
- When the parent is missing, the response is `404 {entity}_not_found`.
- For a junction op whose `list_X` returns entities, those entities are the
  data, with no extra `get`.
- Related collections are not filtered or sorted. They are paginated only
  for a junction op in a paginated module (§9.1).

### 9.4 Route shadowing

A custom op's static segment wins over `{id}` in Axum's router. For
illustration:

- A custom `POST /api/workouts/start` shadows `PATCH` on a workout whose id
  is `start`.
- A custom `GET /api/workouts/summary/{id}` shadows the related link of
  every relationship on a workout with id `summary`.

This is true today for `/{id}`. The contract records it and does not fix
it.

## 10. Custom ops and singletons

Custom requests and responses are fully conformant JSON:API documents
(decisions 1 and 7).

### 10.1 Responses: meta-only documents

Every custom op (`CustomGet`, `CustomPost`) responds with a meta-only
document. iron-log's `stats::get_workout`, with illustrative values (the
example's handler returns zeros):

```http
GET /api/stats/workout HTTP/1.1
Accept: application/vnd.api+json
```

```http
HTTP/1.1 200 OK

{
  "jsonapi": { "version": "1.1" },
  "meta": { "result": { "total_count": 12, "total_duration_minutes": 540 } }
}
```

- `result` is the fn's `Ok` value, serialized by serde exactly as today's
  bare body was. It is a plain value even when `T` is an entity or a `Vec`
  of entities; the client types come from the Rust fn.
- A fn returning `()` responds `204 No Content`, as today.
- The status is `200`, never `201`.
- There is no `links` and no `data`. A meta-only document has no primary
  data, so it needs no `self` link.
- Errors use §13. `AppError`-typed fns get the status mapping. Other error
  types get `500 internal_error`.

### 10.2 Requests

Routes are unchanged: `/api/{url_for_module}/{action}`, plus one path
segment per required non-`Input` parameter on `GET`.

**`POST` bodies are meta-only documents.** Every argument that is not
state, store or a path parameter is a member of `meta.args`, keyed by its
Rust parameter name. That includes an `*Input` argument and `Option`
arguments. For illustration, a
`workout::start(state, input: StartWorkoutInput)` op (the shape
`src/servers/tests.rs` uses):

```http
POST /api/workouts/start HTTP/1.1
Content-Type: application/vnd.api+json
Accept: application/vnd.api+json

{ "meta": { "args": { "input": { "template_id": "push-day" } } } }
```

Rules for `POST` bodies, all within step 7 of §13.2:

- A request with no body is read as `{"meta": {"args": {}}}`. It needs no
  `Content-Type`; §3.2's check applies only when a body is present.
- A body that is not a JSON object is `400 invalid_document` with no
  `source`.
- When `meta` or `meta.args` is missing or is not an object, the request is
  `400 invalid_document`. The pointer names the nearest member that exists:
  `""` when `meta` is missing, `/meta` when `args` is missing. If the fn has
  no required arguments, a missing member is read as `{}` instead.
- Members are checked in §13.2 step 7's order.
- An unknown member of `meta.args` is `400 invalid_document` at
  `/meta/args/{name}`.
- For declared arguments:
  - a missing required argument is `400 invalid_document` at `/meta/args`;
  - a value serde rejects is `400 invalid_document` at its member;
  - an absent or `null` member is `None` for an `Option` argument.

**`GET` optional arguments use the `opArg` family.** `Option` arguments of a
`CustomGet` are `opArg[{name}]` query parameters. For illustration,
`get_summary(state, id: &str, verbose: Option<bool>)` is served as
`GET /api/workouts/summary/w1?opArg[verbose]=true`.

- Each value is deserialized by serde from the string, as `Query` does
  today.
- An unknown `opArg[…]`, a malformed value, or any other parameter is
  `400 invalid_query_parameter`.
- The spec requires `400` for an all-lowercase name it does not itself
  define, such as today's bare `?verbose=true`. An implementation-specific
  family must contain a non-`a-z` character, and the spec recommends a
  capital letter. `opArg` is that family.

### 10.3 Singletons

A singleton module (`// ontogen:singleton`, or
`NamingConfig::singleton_modules`) has no resource type. Its routes stay
`/api/{singular}/{action}`. Every op in it is a custom op and follows
§10.1 and §10.2.

An op in a singleton module that classifies as `List`, `GetById`,
`Create`, `Update`, `Delete` or a junction op is a `CodegenError`. A
singleton module is opted into explicitly, so a CRUD op there is a
mistake. A module with no entity behind it is the case §10.4 serves.

### 10.4 Ops served as custom ops

Three kinds of op have no resource to serve and are served as custom ops,
at the route their classification gives today:

- CRUD-classified ops in a module that is not a resource module (§5.1);
- junction ops outside a resource module (§9.1);
- before phase 3a, junction ops inside one.

The §10.1 and §10.2 rules apply, with these routes:

| Op | Route | Request | Response |
|---|---|---|---|
| `list` | `GET /api/{m}` | paginated: `opArg[limit]`, `opArg[offset]` | `meta.result`: the list, or `{items, total, limit, offset}` when paginated |
| `get_by_id` | `GET /api/{m}/{id}` | — | `meta.result` |
| `create` | `POST /api/{m}` | `meta.args.input` | `200`, `meta.result` |
| `update` | `PATCH /api/{m}/{id}` | `meta.args.input` | `meta.result` |
| `delete` | `DELETE /api/{m}/{id}` | — | `204` |
| `JunctionList` | `GET /api/{m}/{parent_id}/{segment}` | paginated: `opArg[limit]`, `opArg[offset]` | `meta.result`, as for `list` |
| `JunctionAdd` | `POST /api/{m}/{parent_id}/{segment}` | `meta.args.{child param}` | `204` |
| `JunctionRemove` | `DELETE /api/{m}/{parent_id}/{segment}/{child_id}` | — | `204` |

`update` uses `PATCH` here too, so that no generated route uses `PUT`.

## 11. Route prefix and extra surfaces

### 11.1 `route_prefix`

With `RoutePrefix { segments: "projects/:project_id", state_accessor: "store_for", … }`,
every route in this contract moves under the prefix: resource,
relationship, related, custom and event routes alike.

- Paths are `/api/projects/{project_id}/tasks`,
  `/api/projects/{project_id}/tasks/{id}/relationships/epic`, and so on.
- Every link and `Location` carries the prefix:
  `"self": "/api/projects/7c0e…/tasks/ship-the-emitter"`.
- The resource `type` does not carry the prefix (`tasks`). Each scope is
  served from its own store, and no document mixes scopes. Each scope
  therefore acts as a separate API in the spec's sense, and `(type, id)` is
  unique within it.
- A prefix parameter that fails to parse (e.g. a non-UUID `project_id`) is
  `400 invalid_path_parameter`, with no `source`. The spec has no source
  member for path segments.
- A failing scope accessor (`state.store_for(&project_id)`) stays
  `500 internal_error`, as E0003 phase 1 keeps it. Mapping it to `404` waits
  on E0003's store-accessor contract.

Scoped and unscoped routes MUST have identical wire behaviour. Today they
diverge in two places:

- Scoped pagination slices in memory instead of calling the page-taking
  list. Phase 1c removes this.
- Scoped junction ops become action-style routes instead of the
  `{parent_id}/{child}` form. Phase 3a removes this.

### 11.2 Extra API surfaces

Routes and types come from module names exactly as for the primary
surface: no path prefix per surface, and one type per module even when the
module spans surfaces. Each surface's `store_accessor` and `pagination`
apply as today. The wire is indistinguishable from a single-surface server.

## 12. Event streams

Routes, parameters, resume and lag are unchanged from
https://github.com/sksizer/rust-ontogen/pull/184 and
https://github.com/sksizer/rust-ontogen/pull/185:

- `GET /api/events/{event-name}` plus `/{param}` per required parameter.
  iron-log's `activity_for_kind` is `GET /api/events/activity-for-kind/{kind}`.
  `sse_route_overrides` still apply. A `route_prefix` puts the routes under
  the prefix.
- `Option` parameters and `resume` are plain query parameters
  (`?resume=4`). The `Last-Event-ID` header wins over `?resume=`.
- These routes respond `text/event-stream`, not a JSON:API document, so the
  §3 negotiation and §6 query rules do not apply to them. `Accept` is not
  checked. `EventSource` sends `text/event-stream`.

**Frames.** Only the `data:` payload changes (decision 2). tasks-tracker has
no event ops. For illustration, a resumable `task_changed` op yielding
`Task`, with ids from `seq_id`:

```text
event: task-changed
id: 0:17
data: {"type":"tasks","id":"ship-the-emitter","attributes":{"title":"Ship the emitter","status":"closed/done","created":"2026-06-06","body":"## Goal\n…"},"relationships":{"epic":{"data":{"type":"epics","id":"markdown-backend"}},"tags":{"data":[{"type":"tags","id":"codegen"}]}}}
```

When the event's item type `T` is an entity (known from the schema, §5.1),
`data:` is its resource object (§5): `type`, `id`, `attributes`,
`relationships`, in that order.

- Relationship objects carry `data` only, and the frame carries no
  `links`. A frame is not tied to a request URL, and links would double the
  size of every frame for no reader.
- Junction-op relationships, which have no `data`, are left out.

When `T` is not an entity, `data:` is `{"meta":{"result":T}}`, the same
rule as custom ops (§10.1). An example is iron-log's `Activity`, whose
`event_id` is its `seq`:

```text
event: activity-for-kind
id: 4
data: {"meta":{"result":{"seq":4,"kind":"workout","id":"w1"}}}
```

These are unchanged:

- `id:` is emitted for resumable ops only, with the same `seq_id` / `no_id`
  rules and the same newline guard.
- `event: lag` with `data: {"skipped":N}` keeps the stream open. It is a
  control frame, not a payload, so it gets no envelope.
- `event: error` on a serialization failure, and the `:` keep-alive comment
  every 15 s.

A subscribe call that fails before the stream opens returns a JSON:API
error document (§13) with the mapped status. `EventSource` cannot read that
body, but a `fetch`-based client and `curl` can.

## 13. Errors

### 13.1 The error document

```json
{
  "jsonapi": { "version": "1.1" },
  "errors": [ {
    "status": "404",
    "code": "task_not_found",
    "title": "Not Found",
    "detail": "Task not found: nope"
  } ]
}
```

- **Exactly one error object per response.** The server stops at the first
  failure in §13.2's order. The spec allows this, and it keeps the response
  status and the error status equal.
- **Members**, in this order:
  - `status`: the HTTP status as a decimal string (`"404"`). Always present.
  - `code`: a snake_case string from §13.3 or §13.4. Always present.
  - `title`: the status's reason phrase from RFC 9110 (`"Not Found"`,
    `"Conflict"`). Always present. It is constant per status, as the spec
    asks.
  - `detail`: an occurrence-specific sentence. For an `AppError` it is the
    error's `Display` text, as today's `{"error": …}` carried. Always
    present.
  - `source`: present only when one of the three members below applies.
- **`source`** has exactly one member:
  - `pointer`: an RFC 6901 pointer into the request body. It names only a
    value that exists in the request, as the spec requires. For something
    missing, it names the nearest enclosing member that exists.
  - `parameter`: the query parameter's name, decoded, with raw brackets
    (`"page[limit]"`).
  - `header`: `"Content-Type"` or `"Accept"`.
- Error objects carry no `id`, `links` or `meta`.
- **`status`, `code`, `title` and `source` are normative. `detail` is
  not.** Its wording may change between releases, and clients must not
  parse it. The `detail` strings in this document are examples; generator
  tests may pin them.

### 13.2 Check order

This is the only statement of the order. The per-operation tables (§8, §9)
refer to its steps. Every route checks in this order, and the first failure
is the response:

1. **Routing.** A method the route does not serve is `405` (§13.5).
2. **`Accept`** (§3.2): `406`.
3. **`Content-Type`**, when the request has a body (§3.2): `415`.
4. **Path parameters**, in path order:
   - a typed prefix parameter that fails to parse is
     `400 invalid_path_parameter`;
   - on relationship and related routes, an unknown `{rel}` is
     `404 relationship_not_found`.
5. **Query parameters.**
   - First, a name the route does not accept (§6): the first such name in
     request order.
   - Then the accepted parameters, in canonical order (§4.3): `filter[…]`,
     `sort`, `include`, `page[offset]`, `page[limit]`, then `opArg[…]` in
     byte order of member name.
6. **Route-level refusals** decidable without the body: the `403`s of §9's
   table.
7. **The request body**, in the order of the operation's table. A body
   larger than the server accepts is `413 content_too_large`, ahead of
   every row of the table. Members are checked in schema order, so no
   order-preserving parser is needed.
   The rule applies per member family, families in this order:
   1. unknown attribute names, in byte order;
   2. declared attributes, in declaration order;
   3. unknown relationship names, in byte order;
   4. declared relationships, in declaration order.

   Custom-op `meta.args` (§10.2) is one family: unknown names in byte
   order, then declared arguments in declaration order.
8. **Store reads the handler makes before acting.** First the parent
   resource on relationship routes (`404 {entity}_not_found`). Then each
   linked resource, in step-7 order (`404 related_resource_not_found`).
9. **The operation itself**: the store call or the custom op, and its
   `AppError`.

So:

- `GET /api/tasks/nope?include=owner` is `400 invalid_include_path`, not
  `404`.
- `GET /api/tasks?sort=priority&page[limit]=0` is the `sort` error.
- A `PATCH` to a missing id with a malformed body is the body's `400`.

### 13.3 Ontogen-authored errors

These are errors the generated server raises itself, independent of the
consumer's `AppError`:

| Status | `code` | Raised when |
|---|---|---|
| 400 | `invalid_query_parameter` | an unknown, repeated or malformed query parameter, including `page`, `filter`, `opArg` and `fields` (§6, §7, §10.2) |
| 400 | `invalid_sort_field` | a bad or unsupported `sort` (§7.4) |
| 400 | `invalid_include_path` | a bad `include` on a route that accepts it (§7.5) |
| 400 | `invalid_path_parameter` | a typed prefix parameter fails to parse (§11.1) |
| 400 | `invalid_document` | the body is not JSON, or not a valid request document for the route |
| 400 | `unknown_attribute` / `missing_attribute` / `invalid_attribute` | attribute problems (§8.2, §8.3) |
| 400 | `unknown_relationship` | a relationship name the type lacks (§8.2) |
| 400 | `missing_relationship` | a non-`Option` to-one absent from a create body (§8.2) |
| 403 | `relationship_required` | `null` on a non-`Option` to-one (§8.2, §8.3, §9) |
| 403 | `relationship_update_unsupported` | a relationship write the relationship does not support: `PATCH` on a junction op, `POST`/`DELETE` on a to-one, or a junction-op relationship in a create or update body (§8.2, §9) |
| 403 | `relationship_batch_unsupported` | a relationship `POST` or `DELETE` with more than one identifier (§9) |
| 404 | `related_resource_not_found` | a linked id that does not exist (§8.2, §8.3, §9) |
| 404 | `relationship_not_found` | `{rel}` is not a relationship of the type (§9) |
| 405 | `method_not_allowed` | a method the route does not serve (§13.5) |
| 406 | `not_acceptable` | §3.2 |
| 409 | `type_mismatch` | a `type` that is not the endpoint's or the relationship's (§8.2, §8.3, §9) |
| 409 | `id_mismatch` | a `PATCH` body id differs from the URL id (§8.3) |
| 413 | `content_too_large` | the body is larger than the server's body-size limit (Axum's `DefaultBodyLimit`: 2 MB unless the consumer's router sets another) |
| 415 | `unsupported_media_type` | §3.2 |
| 500 | `internal_error` | a store-construction or scope-accessor failure; a custom op whose error type is not `AppError`; an `AppError`-typed site in a consumer with no `AppError` in its schema directory |

The `detail` for `internal_error` is the error's `Display` text, as today.

### 13.4 `AppError`: E0003 phases 0-1, folded in

Decision 6 folds E0003 phases 0 and 1 into E0004:

- the `ApiFn.error_type` capture;
- the `AppError` scan over the schema directory;
- the call-site routing predicate (last path segment `AppError`).

The scan maps variants by name suffix:

| `AppError` variant | Constructed by | Status |
|---|---|---|
| `{Entity}NotFound(id)` | store `get`, `update`, `delete` (today) | 404 |
| `{Entity}IdRequired(reason)` | store `create`, when no id exists after `before_create` and the `IdStrategy` | 400 |
| `{Entity}AlreadyExists(id)` | store `create`, on a duplicate id | 409 |
| `{Child}ParentRequired(child_id)` | a parent's `has_many` write that would drop a child whose foreign key is not `Option` (§5.4) | 403 |
| any other variant (`Md`, `DbError`) | — | 500 |

- **`code` is the variant name**, converted to snake_case at generation
  time: `task_not_found`, `task_id_required`, `task_already_exists`,
  `task_parent_required` (the child of `Task.subtasks` is a `Task`), `md`,
  `db_error`. One rule covers every
  variant, mapped or not.
- **Declaring the variants.** The store generator constructs every
  variant in the table, so a consumer `AppError` must declare each one the
  generated store uses:
  - `NotFound`, `IdRequired` and `AlreadyExists` for every entity.
    tasks-tracker gains `TaskIdRequired`, `TaskAlreadyExists` and the
    equivalents for `Epic` and `Tag`.
  - `ParentRequired` only for an entity that is the child of a `has_many`
    whose foreign key is not `Option`. tasks-tracker needs none.
- **Code clashes.** A variant whose snake_case name equals a §13.3 code
  (an `InvalidDocument` variant, say) is a `CodegenError`, so a code always
  means one thing.
- **`source`.** `AppError`-derived errors carry no `source`, except
  `*AlreadyExists` on a create that carried `data.id` (§8.2).
- **No `AppError` in the schema directory** (the scan-dirs-only case):
  every `AppError`-typed site maps to `500 internal_error`. E0003's "emit
  today's exact shape" rule no longer applies, because the envelope
  changes regardless.
- **E0003 phases 2 and 3** stay in E0003 and slot into this document
  unchanged:
  - `#[http(status = N)]` annotations override the suffix rule. The
    `title` becomes N's reason phrase, and the `code` stays the variant
    name.
  - An `error_handler` override owns the whole response, and with it
    conformance.

### 13.5 Statuses the spec mandates, and `405`

| Status | Spec trigger | Where in this contract |
|---|---|---|
| 400 | unknown or unsupported query parameter; unsupported `include` or `sort` | §6, §7 |
| 403 | unsupported relationship update, including a refused removal or full replacement | §5.4, §8, §9 |
| 404 | missing resource, relationship or related resource | §8, §9 |
| 406 | unsatisfiable `Accept` | §3.2 |
| 409 | duplicate client id; type or id mismatch | §8.2, §8.3, §9 |
| 415 | bad `Content-Type` | §3.2 |

**405.** A generated route that does not serve a method answers `405`, with
an `Allow` header listing the methods it does serve, and an `errors[]` body
with code `method_not_allowed`. `HEAD` is listed wherever `GET` is, because
Axum serves it. Examples:

- `PUT` on `/api/tasks/{id}`: `Allow: GET, HEAD, PATCH, DELETE`.
- `PUT` on `/api/tasks`: `Allow: GET, HEAD, POST`.
- `PUT` on a relationship route: see §9.

`POST` or `DELETE` on a to-one relationship is not a `405`. The route
serves those methods, and the spec makes an unsupported relationship update
`403` (§9).

The generated router installs this as its method-not-allowed fallback. It
installs no path fallback: an unmatched path falls through to the
consumer's router, which owns its own `404`.

**Axum extractor rejections.** These never reach the client in Axum's
plain-text form. The `ontogen-jsonapi` extractors replace `Json`, `Query`
and `Path` in generated handlers and produce the documents above.

## 14. TS transport mapping

The generated HTTP transport keeps the flat `Transport` interface:

- every method name, parameter and return type is unchanged;
- list methods gain one trailing optional argument (§14.2).

JSON:API is applied and removed inside the transport. The admin layer needs
no source change.

### 14.1 Requests

- Every request sends `Accept: application/vnd.api+json`.
- Every request with a body sends
  `Content-Type: application/vnd.api+json`.
- The `httpPut` helper is removed, and `httpPatch` replaces it.

### 14.2 Per-operation mapping

| `Transport` method | HTTP | Flat result built from |
|---|---|---|
| `xList(…, options?)` (unpaginated) | `GET /api/{type}`, plus `filter[k]=v` per defined key of `query` and `sort` from `options.sort` | `data.map(flatten)` → `R[]` |
| `xList(…, limit?, offset?, …, options?)` (paginated) | adds `page[offset]`, `page[limit]` when defined | `{ items: data.map(flatten), total: meta.total, limit: meta.limit, offset: meta.offset }`, which is today's `PaginatedResult<R>` unchanged |
| `xGetById(id)` | `GET /api/{type}/{id}` | `flatten(data)` |
| `xCreate(input)` | `POST /api/{type}`, body `unflatten(input)` | `flatten(data)` (201) |
| `xUpdate(id, input)` | `PATCH /api/{type}/{id}`, body `unflatten(input, id)` | `flatten(data)` |
| `xDelete(id)` | `DELETE /api/{type}/{id}` | `null` (204) |
| `xListX(parentId)` (junction) | entities: `GET /api/{type}/{parentId}/{rel}`; ids: `GET …/relationships/{rel}` | `data.map(flatten)` or `data.map(i => i.id)` |
| `xListX(parentId, limit?, offset?)` (junction, paginated module) | as above, plus `page[offset]`, `page[limit]` | `PaginatedResult` from `data` and `meta`, as for `xList` |
| `xAddX(parentId, childId)` | `POST …/relationships/{rel}`, body `{data:[{type, id: childId}]}` | `null` (204) |
| `xRemoveX(parentId, childId)` | `DELETE …/relationships/{rel}`, same body | `null` (204) |
| custom `GET` | `GET /api/{m}/{action}/{path…}`, `Option` args as `opArg[name]` | `meta.result`, or `null` on 204 |
| custom `POST` | body `{meta:{args:{<rust_param_name>: value, …}}}` | `meta.result`, or `null` on 204 |
| op served as custom (§10.4) | its §10.4 route | `meta.result`, or `null` on 204 |
| `subscribeX(args, handlers)` | unchanged URL, `?resume=` and lag | entity `T`: `flatten(JSON.parse(data))`; other `T`: `.meta.result` |

The junction rows describe phase 3a. Between phases 1c and 3a, junction
methods call the §10.4 forms and read `meta.result`.

**Sort** (decision 8). Every list method whose Rust fn takes an `order`
argument gains a trailing optional argument, after every existing
parameter (`projectId` included):

```ts
export type TaskSortKey = 'id' | '-id' | 'title' | '-title' | 'status' | '-status' | 'created' | '-created';
export interface ListOptions<K extends string> { sort?: K[] }

taskList(query?: ListTasksQuery, limit?: number, offset?: number, options?: ListOptions<TaskSortKey>): Promise<PaginatedResult<Task>>;
```

- A trailing optional argument breaks no positional caller, so the admin
  layer is unaffected.
- The argument is on the shared `Transport` interface, so the IPC
  transport implements it too (§15).
- The HTTP transport sends `options.sort` as `sort=` the keys joined by
  `,`. An absent or empty array sends nothing.
- `include` is not exposed. The flat return shape has nowhere to put
  included resources. The shared interface is the constraint, since IPC
  has no `include` either.

**Queries.** `toQueryString` gains a family form.
`toQueryString({ filter: query, page: { offset, limit } })` emits
`filter%5Bstatus%5D=…` and so on, skipping `null` and `undefined`. Arrays
are not supported in filters, matching §7.3.

### 14.3 Flatten and unflatten

The generator emits one pair of functions per entity, from the same
relationship table as the server (§5.4). For `Task`:

```ts
const TASK_REL = { epic: { field: 'epic_id', many: false },
                   tags: { field: 'tags', many: true } } as const;

function flattenTask(r: JsonApiResource): Task {
  return {
    id: r.id,
    ...r.attributes,
    epic_id: r.relationships?.epic?.data?.id ?? null,
    tags: (r.relationships?.tags?.data ?? []).map((i) => i.id),
  } as Task;
}
```

`unflatten(input, id?)` builds `{ data: { type, id?, attributes, relationships } }`:

- **Id.** It goes in `data.id` when the argument is given (update), or when
  `input.id` is a non-empty string (create). It never goes in
  `attributes`, whatever its value, including `""`.
- **Relation fields** move to `relationships`, `has_many` included:
  - a to-one value `v` becomes `{ data: v == null ? null : { type, id: v } }`;
  - a to-many array becomes an identifier array.
- **Every other key** goes to `attributes`.
- **Undefined and null.** Keys whose value is `undefined` are omitted; on
  update that means "unchanged", matching `UpdateXInput`. `null` is kept,
  and clears the field.

### 14.4 Errors

Any non-2xx response throws a `JsonApiError`:

```ts
export class JsonApiError extends Error {
  readonly name = 'JsonApiError';
  constructor(
    readonly status: number,
    readonly errors: JsonApiErrorObject[],   // [] when the body was not a JSON:API error document
    message: string,                         // errors[0]?.detail ?? errors[0]?.title ?? statusText
  ) { super(message); }
}
```

- `message` is today's string: the server's `detail` is the old `error`
  text. `String(e)` is `"JsonApiError: Task not found: nope"`, where today
  it reads `"Error: …"`. No admin-layer test pins that prefix, and the
  admin layer only renders `String(e)`.
- Callers that want the status or the code read `e.status` and
  `e.errors[0]?.code`.
- A non-JSON error body (a proxy's HTML `502`) still throws a
  `JsonApiError`, with `errors: []`.

### 14.5 What stays identical for callers

- Every existing method name, parameter and return type on `Transport`.
- `PaginatedResult<T>`: same declaration, same fields, and same values.
  `limit` and `offset` are the effective values, as today.
- Entities in and out are flat, with the same field names, `null` for
  cleared optionals, and id arrays for to-many relations.
- `null` from delete, junction add and remove, and `()` custom ops.
- `SubscriptionHandlers<T>`: `onEvent` receives a flat `T`, and `onLag`,
  `onOpen` and `onError` are unchanged.
- The admin registry flags (`paginated`, `defaultLimit`, `maxLimit`,
  `listHasQuery`).

The admin layer's `tests/fixtures/event-transport.generated.ts` is a copy of
generated output, so phase 1b regenerates it.

## 15. IPC and MCP

Payloads stay flat. JSON:API exists only at the HTTP boundary.

- **Tauri IPC**: same commands, same `invoke` argument objects, same flat
  entities, `PaginatedResult` for paginated lists, `String` errors, and the
  same `EventFrame` channel (`{kind:"event", id, data}` with a flat `data`,
  and `{kind:"lag", skipped}`).
- **MCP**: same tool names, the same flat argument schemas
  (`schema_for_with_str_id` for update), and the same results:
  `{"success": true}` for create and update, entities for get, and bare
  arrays or `{items, total, limit, offset}` for lists. Event ops are still
  skipped.

Three changes reach them, none of which changes a payload's shape:

1. **`sort` on list** (decision 8). The IPC list command gains an optional
   `sort: Option<Vec<String>>` argument, and the TS IPC transport passes
   `options.sort` into it. The MCP list tool schema gains an optional
   `sort` array whose items enumerate the sort keys. Both parse with
   `ontogen_core::order::parse_sort` (ADR 0006 §1) and return its error
   text on a bad key.
2. **`has_many` writes clear dropped children** (decision 9). Today an
   update that drops a child leaves the child's foreign key set, on every
   transport. The store fix in phase 1a corrects IPC and MCP as well as
   HTTP. A listed child that does not exist is `{Child}NotFound`, and
   nothing is written (§5.4).
3. **New typed store errors.** `{Entity}AlreadyExists`, `{Entity}IdRequired`
   and `{Child}ParentRequired` replace backend messages. On these
   transports they are still strings.

## 16. Decision index

Choices this contract makes where the spec or the epic left one open, with
the section that states each and its reason.

| Choice | § | Why |
|---|---|---|
| `jsonapi: {"version":"1.1"}` on every document | 4.1 | Without it clients assume 1.0, and the member costs nothing |
| `application/json` request bodies are `415` | 3.2 | One media type, and a client sending the old flat body fails loudly instead of mis-parsing |
| A JSON:API instance in `Accept` outranks wildcards; otherwise `*/*` and `application/*` satisfy it | 3.2 | The spec requires `406` when every JSON:API instance is unusable, and browsers, `curl` and `EventSource` send `*/*` |
| `Vary: Accept` on every response | 3.3 | The response depends on `Accept` (406) |
| Links and `Location` are relative | 4.2 | The server cannot know its public origin behind proxies, dev servers and tunnels |
| Canonical link query order and encoding | 4.3 | Byte-stable links for snapshots and caches |
| The schema is an explicit input of the servers and clients stages | 5.1 | Nearly every rule needs it, and today neither stage sees it |
| CRUD ops with no entity behind them are served as custom ops | 5.1, 10.4 | Keeps the scan-dirs-only use case working, and without a schema there is no resource to build |
| `links.self` on every resource object | 5.2 | `Location` must match it, and clients can refetch without building URLs |
| `relationships` omitted when a type has none | 5.2 | Avoids an empty object on every `tags` and `epics` resource |
| Shape-changing serde attributes on entities are a `CodegenError` | 5.3 | Attribute, sort and filter names all assume field name = member name |
| `belongs_to` loses `_id`; other relation fields keep their name | 5.4 | Matches how the field reads; JSON:API names relationships, not keys |
| Duplicate identifiers collapse to their first occurrence | 5.4 | Matches the add-once rule of relationship `POST` |
| Offset pagination with `page[offset]`/`page[limit]` | 7.2 | Same semantics as today's store signature |
| `page[limit]=0` is `400` | 7.2 | A zero page has no `next` or `last` |
| `meta: {total, limit, offset}` on paginated lists only | 7.2 | Exactly rebuilds `PaginatedResult`, and is redundant when unpaginated |
| `self` plus the four pagination links, always present, `null` when unavailable | 7.2 | One shape to read |
| `prev` at or past the end points at the last page | 7.2 | An overshooting client steps back to data |
| A hand-written `list` replaces the generated one; `order` goes after filters, before page params | 7.3 | Filter and sort must combine, and the store has no filter |
| Filter names are the `*Query` struct's field names | 7.3 | The struct is user-authored, and ontogen does not rename it |
| `id` is the implicit last sort key | 7.4 | A total order makes pages stable (ADR 0006) |
| Dangling linkage is skipped in `included` and related links, not an error | 7.5 | Markdown tolerates dangling wikilinks by design |
| One id-validity rule on both backends, applied to ids being created; an invalid one is `400` | 8.2 | A malformed id is a bad request, not a store `500`, and the backends agree |
| A path `{id}` is a lookup key, never validated | 8.1 | Every row the store lists stays servable at its `links.self`, including SeaORM rows that predate the rule |
| Unknown attributes are `400` | 8.2 | Catches clients still sending the flat shape |
| Body members are checked in schema order, unknown names in byte order | 8.2, 13.2 | Deterministic without an order-preserving parser |
| Missing ids are detected by the store, after hooks, as `{Entity}IdRequired` | 8.2, 13.4 | Hooks may assign the id, and the handler need not know the `IdStrategy` |
| Duplicates are `{Entity}AlreadyExists`, and SeaORM retries a derived id that loses a race | 8.2, 13.4 | Atomic detection on both backends, typed for every transport |
| Create is always `201` with the document | 8.2 | Ids and hooks change the resource, and TS returns the entity |
| `PATCH` is always `200` with the document | 8.3 | Hooks may change the resource, and TS returns the entity |
| A `PATCH` body id is compared exactly; any difference is `409` | 8.3 | The spec requires `409` for an id that does not match the endpoint |
| Delete and relationship mutations are `204` | 8.4, 9 | Nothing to report, and TS returns `null` |
| `has_many` is writable; dropping a required-foreign-key child is `403` | 5.4, 9 | Decision 9; the child cannot be orphaned, and the spec requires `403` for a refused removal or replacement |
| Unsupported relationship updates are `403`, not `405` | 9 | The spec requires `403` for an unsupported relationship update |
| One identifier per relationship `POST`/`DELETE` | 9 | Every write is one call, so no request is partly applied; TS sends one id |
| A lone `list_X` is a custom op | 9.1 | A plain filtered list must not become a relationship |
| Junction `DELETE` skips the target-existence check | 9.1 | A member whose target was deleted must still be removable |
| Paginated junction lists stay paginated on the relationship routes | 9.1 | Keeps `xListX`'s `PaginatedResult` signature |
| Custom `POST` bodies are `{meta:{args:{…}}}` | 10.2 | Decision 7; one rule, and a valid JSON:API request document |
| Custom `GET` optional args use the `opArg[…]` family | 10.2 | Decision 7; the spec reserves all-lowercase names |
| CRUD ops in a singleton module are a `CodegenError` | 10.3 | A singleton is opted into, so a CRUD op there is a mistake |
| Non-entity event payloads and custom results are `{meta:{result}}` | 10.1, 12 | One rule for every non-resource payload |
| Event frames carry no links | 12 | A frame has no request URL, and links would double its size |
| One error object per response, first failure in §13.2 order | 13.1, 13.2 | Every request has exactly one correct error |
| A body over the size limit is `413`, not `400` | 13.2, 13.3 | RFC 9110 defines the status for it, and a client can tell a body that is too big from one that is malformed |
| `code` is the `AppError` variant in snake_case, or a fixed ontogen code | 13.3, 13.4 | Machine-readable without parsing `detail` |
| `title` is the reason phrase of the status | 13.1 | Constant per problem, as the spec asks |
| `detail` is not normative | 13.1 | Wording can improve without a contract change |
| No path fallback; `405` via a method fallback | 13.5 | The generated router is merged into a consumer router it must not hijack |
| `sort` is a trailing optional on the shared `Transport`; `include` stays HTTP-only | 14.2 | Decision 8; the shared interface has no flat shape for included resources |
| `JsonApiError` carries `status` and `errors`, with the old message | 14.4 | Admin output is unchanged, and callers can branch on status |

## 17. Phase mapping

Phases 1a, 1b and 1c ship together as `0.9.0`.

| Phase | Sections |
|---|---|
| 1a | Store and runtime prerequisites. The `ontogen-jsonapi` crate (documents, link building, error document, extractors). The id-validity rule and slug function shared by both backends. `IdStrategy` on SeaORM, with one build-time source of truth and derived-id retry. `{Entity}AlreadyExists` and `{Entity}IdRequired` (§13.4). The `has_many` fix and `{Child}ParentRequired` (§5.4). The markdown id-ascending default order, many_to_many order and the parity fixture's default cases (ADR 0006 §6). The SeaORM `i64` field for integer primitives under `OptionEnum`/`Other` (ADR 0006 §4). Markdown lookups of an uncreatable id answer `{Entity}NotFound` (§8.2) |
| 1b | CRUD over JSON:API. Schema input (§5.1). §3 media type, §4 documents, §5 resource objects (relationship `data` only), §6 query rules, §7.1–§7.2 list and pagination, §8 get, create, update and delete, §13 errors with the E0003 phase 0-1 scan, §13.5 `405`. §14 for CRUD methods, `JsonApiError`. Scoped CRUD routes. Modules with no entity behind them are left unchanged until 1c |
| 1c | Everything else on the 0.9.0 wire. §10 custom ops (`meta.args`, `opArg`, singleton check, §10.4 ops served as custom, junction ops included). §12 event frames. §11.1 scoped pagination. §14 for custom, junction and subscription methods |
| 2 | §7.3 filter, including the hand-written-list precedence and the bare-parameter fix |
| 3a | §9 relationship endpoints, related links and relationship `links`. The junction classification change. Scoped junction routes. TS junction methods |
| 3b | §7.5 include |
| 3c | §7.4 sort, the `order` argument (ADR 0006), and `sort` on TS, IPC and MCP (§14.2, §15) |
