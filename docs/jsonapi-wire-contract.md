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

Not covered, and unchanged: Tauri IPC commands, MCP tools, the Rust store
and API layers (except the `order` argument from ADR 0006), and the flat TS
`Transport` interface.

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
with additions that the named phase adds to tasks-tracker. That keeps every
example in this document live once its phase lands:

| Addition | Lands in | Why |
|---|---|---|
| `pagination: Some(PaginationConfig { default_limit: 20, max_limit: 100 })`, `paginated: ["task"]` | phase 1 | §7.2 examples |
| A `ListTasksQuery { status: Option<String>, epic_id: Option<String> }` filter on `task::list`, with a matching `count` | phase 2 | §7.3 examples |
| `Task.parent_id: Option<String>` (`belongs_to Task`) and `Task.subtasks: Vec<String>` (`has_many Task`, `foreign_key = "parent_id"`), as in `crates/markdown-pilot` | phase 3a | `has_many` examples in §5.3 and §9 |

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

**Content-Type.** A request that carries a body (`POST`, `PATCH`, and
`DELETE` on a relationship endpoint) MUST have
`Content-Type: application/vnd.api+json`. The server responds:

| Request `Content-Type` | Response |
|---|---|
| `application/vnd.api+json` | processed |
| `application/vnd.api+json; profile="…"` | processed; unknown profiles are ignored, as the spec requires |
| `application/vnd.api+json; ext="…"` | `415`. Ontogen supports no extension, so every `ext` URI is unsupported |
| `application/vnd.api+json` with any other parameter (e.g. `charset=utf-8`) | `415`, as the spec requires |
| anything else, including `application/json`, or the header absent | `415` |

Why `application/json` is refused: one media type for every payload, and a
client that sends a flat entity with the old content type gets a clear error
rather than a body parse failure.

Every `415` carries `source.header: "Content-Type"` (§13). A body on `GET`
or on a resource `DELETE` is ignored and its `Content-Type` is not checked.

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

Why wildcards count at all: browsers, `curl` and `EventSource` send `*/*`,
and HTTP lets a server answer a wildcard with its only representation.

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

**`jsonapi` is emitted on every document, including errors.** Without it a
client must assume 1.0. The contract relies on 1.1 semantics (the `ext` and
`profile` negotiation rules in §3.2, and the 1.1 error and link members),
and the member is a constant string.

### 4.2 Links are relative

Every link the server emits, and the `Location` header, is a relative
reference: an absolute path plus an optional query, e.g.
`/api/tasks/ship-the-emitter`. RFC 3986 permits a URI-reference, and
RFC 9110 §10.2.2 permits a relative `Location`.

Why relative: the generated server does not know its public origin. It sits
behind proxies, Tauri dev servers and tunnels, and an absolute link built
from `Host` would be wrong in all of them.

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

### 5.1 Shape

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
- **`id`** is the entity's `#[ontology(id)]` field. It MUST be a Rust
  `String` (ADR 0001 contract item 1). The HTTP generator raises a
  `CodegenError` for any other id type.
- **`attributes`** is always present (§5.2).
- **`relationships`** is present when the type declares at least one
  relationship, and absent otherwise (`epics` and `tags` in the example).
- **`links.self`** is always present, and equals the `Location` header on
  create (§8.2), as the spec requires when both exist.
- No `meta` member.

### 5.2 Attributes

`attributes` is the entity's own serde serialization with the id field and
every relation field removed. Concretely:

- Member names are the Rust field names, in declaration order.
- Every non-relation field is present on every response, including
  `Option` fields, which serialize as `null` when `None`.
- The `#[ontology(body)]` field is an ordinary attribute named after its
  field (`body`).
- Fields with role `Skip` are not attributes.

The generator raises a `CodegenError` when an attribute name is not a legal
JSON:API member name (for example a leading or trailing `_`), or is `type`
or `id`.

It also raises one when an entity struct carries a serde attribute that
changes its serialized shape: `rename`, `rename_all`, `alias`, `flatten`,
`skip`, `skip_serializing`, `skip_serializing_if`, `serialize_with` or
`with`. Attribute names, `unknown_attribute` detection, sort-field names and
`null` for `None` all assume the field name and the serialized member are
the same, and the generator does not track the alternatives.
`#[serde(default)]` and `deserialize_with` change only deserialization and
stay allowed. They are the only serde field attributes any in-tree schema
uses.

### 5.3 Relationships

Every field with `FieldRole::Relation` leaves `attributes` and becomes a
relationship:

| Relation kind | Rust field | Relationship name | Arity | Linkage |
|---|---|---|---|---|
| `belongs_to` | `epic_id: Option<String>` | `epic` | to-one | `{"type": "epics", "id": "…"}`, or `null` when `None` |
| `belongs_to` | `workout_id: String` | `workout` | to-one | always an identifier |
| `many_to_many` | `tags: Vec<String>` | `tags` | to-many | array of identifiers, in stored order |
| `has_many` | `subtasks: Vec<String>` | `subtasks` | to-many | array of identifiers, id ascending (ADR 0006) |

Naming rule: a `belongs_to` field whose name ends in `_id` loses that
suffix. Every other relation field keeps its name. The target type is
`url_plural` of the relation's `target` entity.

The generator raises a `CodegenError` when a relationship name equals an
attribute name, `type`, `id` or `relationships`, or when two relation
fields produce the same name. The spec gives fields one namespace, and
`relationships` is a URL segment in §9.

Relationship names are also URL segments (`/relationships/{rel}`, §9),
used verbatim: `/api/tasks/{id}/relationships/subtasks`, and for a
two-word field, `…/relationships/sub_tasks`.

**`has_many` is read-only on the wire.** It appears in responses with its
derived linkage. A request that writes it fails with `403`: in a create or
update body (§8.2, §8.3), and on its relationship endpoint (§9). Why: the
store sets the foreign key of each listed child but never clears the
foreign key of a child that was dropped from the list. It cannot honour the
full replacement that JSON:API gives a to-many `PATCH`. The relationship is
changed from the child's side, through its to-one relationship. Tauri IPC
and MCP keep today's write behaviour.

A user-authored junction-op relationship (§9.1) has no field on the
entity. From phase 3a it appears in resource objects with `links` only.
Before phase 3a it does not appear at all, and it never appears in event
frames (§12), which carry no links. A relationship object with neither
`links` nor `data` would be empty, which the spec forbids.

Relationship objects have members in the order `links`, `data`:

```json
"epic": {
  "links": {
    "self": "/api/tasks/ship-the-emitter/relationships/epic",
    "related": "/api/tasks/ship-the-emitter/epic"
  },
  "data": { "type": "epics", "id": "markdown-backend" }
}
```

Phase 1 emits `data` only. Phase 3a adds `links`, together with the
endpoints they point at, because the spec requires a server to serve every
link it emits.

### 5.4 Worked example: one task

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

Each JSON:API route accepts a fixed set of query parameters. Any other
parameter, including an unknown member of an accepted family, a repeated
parameter, or a parameter with a malformed name, is `400` with
`source.parameter` naming it (code `invalid_query_parameter`). The spec
requires this for names it reserves, and ontogen defines no
implementation-specific parameters on resource routes.

| Route | Accepted parameters |
|---|---|
| `GET /api/{type}` | `filter[…]` when the list takes a filter (§7.3); `sort` (§7.4); `include` (§7.5); `page[offset]`, `page[limit]` when paginated (§7.2) |
| `GET /api/{type}/{id}` | `include` |
| `POST /api/{type}`, `PATCH`, `DELETE /api/{type}/{id}` | none |
| `GET /api/{type}/{id}/{rel}` (related link) | none |
| `/api/{type}/{id}/relationships/{rel}`, any method | none |
| custom op, `GET` | `opArg[…]` (§10.2) |
| custom op, `POST` | none |
| event stream | outside these rules (§12) |

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
- No `meta` and no pagination links. Why: the whole set is in `data`, so a
  total would repeat `data.length`.
- `page[…]` is `400` (`invalid_query_parameter`): the list is not paginated.
- Order: id ascending, unless `sort` is given (ADR 0006 §3). Phase 1a
  makes id order the store's default on both backends. `sort` arrives in
  phase 3c.

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

All of these `400`s use code `invalid_query_parameter` and
`source.parameter` naming the parameter, e.g. `"page[limit]"`.

Why `page[limit]=0` is refused, when today it returns an empty page: a
zero-size page makes `last` and `next` undefined, and a client walking
`next` would loop.

**`meta`.** `total` is the filter-aware `count` (#172), so it counts the
filtered set, not the table. `limit` and `offset` are the effective values.
Together they are exactly the fields of today's `PaginatedResult`, which
lets the TS transport rebuild it (§14).

**Links.** With effective limit `L`, offset `O` and total `T`, and
`last_offset = T == 0 ? 0 : floor((T - 1) / L) * L`:

| Link | Offset | `null` when |
|---|---|---|
| `self` | `O` | never |
| `first` | `0` | never |
| `prev` | `last_offset` when `O > last_offset`, otherwise `max(0, O - L)` | `O == 0` |
| `next` | `O + L` | `O + L >= T` |
| `last` | `last_offset` | never |

All five keys are always present, with `null` for an unavailable link. The
spec allows omitting them instead. Always-present keys give clients one
shape to read. Every link carries the request's `filter`, `sort` and
`include` in canonical form (§4.3).

An offset past the last page gets a `prev` pointing at the last page, not
at `O - L`, so a client that overshoots steps straight back to real data.
With `T = 45` and `L = 10`, `O = 45` gives `prev` offset 40.

**Ordering.** Pages are cut from the ADR 0006 order: the requested `sort`,
then id ascending as the final tie-break, or id ascending alone. Page
boundaries are therefore stable while the data is unchanged. The id-only
default lands in phase 1a, with this envelope's prerequisites, so no phase
ships pagination over an unordered list.

**Consistency.** `count` and the page query are separate store calls, as
today, so a concurrent write can make `total` disagree with the pages by
the size of that write. The contract does not promise a snapshot.

**Edge examples:**

- `GET /api/tasks` returns offset 0 and limit 20. Its `self` is
  `/api/tasks?page%5Boffset%5D=0&page%5Blimit%5D=20`, `prev` is `null`,
  `next` is `…offset%5D=20…`, and `last` is `…offset%5D=40…`.
- `GET /api/tasks?page[limit]=500` is clamped to 100. `meta.limit` is
  `100`, `next` is `null`, and `last` is `…offset%5D=0&page%5Blimit%5D=100`.
- For an empty vault, `T = 0`: `data` is `[]`, `meta.total` is `0`, `first`
  and `last` are offset 0, and `prev` and `next` are `null`.
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

Phase 2. A list whose API fn takes a user-authored `*Query` struct reads
that struct from the `filter` family. Each `filter[name]` parameter becomes
the struct field `name`, deserialized by serde from the string value
exactly as `axum::extract::Query` does today. A bare non-`Option` list
parameter (e.g. `skill_id: &str`) becomes a required `filter[skill_id]`.

With `ListTasksQuery { status: Option<String>, epic_id: Option<String> }`:

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

- Filter member names are the struct's serialized field names, not
  relationship names. The filter above is `filter[epic_id]`, not
  `filter[epic]`, because the struct is user-authored and ontogen does not
  rewrite it.
- A `filter[x]` that the struct does not deserialize is `400`
  (`invalid_query_parameter`, `source.parameter: "filter[x]"`). Serde
  ignores unknown fields by default, so the generated code detects them
  (for example with `serde_ignored`) instead of trusting the struct.
- A value serde rejects is `400`, same code, naming the parameter.
- A missing required filter (a non-`Option` field or bare parameter) is
  `400`, naming the parameter.
- Nested or array forms (`filter[x][]`, `filter[x][y]`, `filter[x.y]`) are
  `400`.
- A list without filters answers any `filter[…]` with `400`.
- The generated CRUD `list` takes no filter. Generic field filters are not
  part of this epic.

### 7.4 Sort

Phase 3c. `sort` is honoured on a list whose API fn takes an
`order: &[OrderBy<{Entity}SortField>]` argument (ADR 0006). The generated
CRUD `list` always takes one. Sort fields are the entity's sortable fields:
`id` plus every scalar attribute (ADR 0006 §2). Relationship names and
dotted paths are not sort fields.

For `tasks` the sort fields are `id`, `title`, `status` and `created`.
`body` is excluded, `epic` is a relationship, and `tags` is not a scalar.

```http
GET /api/tasks?sort=-created,title HTTP/1.1
Accept: application/vnd.api+json
```

The response is ordered by `created` descending, then `title` ascending,
then `id` ascending (the implicit final key). Its `self` is
`/api/tasks?sort=-created,title&page%5Boffset%5D=0&page%5Blimit%5D=20`.

Rules:

- Keys apply in the order given. A leading `-` means descending; otherwise
  ascending.
- `id` is appended as the last key, ascending, unless the request names
  `id`, in which case the requested direction stands.
- `null` sorts before every value in ascending order and after every value
  in descending order. Strings compare by byte. Both rules are fixed by
  ADR 0006 for parity between the backends.
- Each of these is `400` with code `invalid_sort_field` and
  `source.parameter: "sort"`:
  - an unknown field (`sort=priority`), a relationship (`sort=epic`) or a
    dotted path (`sort=epic.title`);
  - a field named twice (`sort=title,-title`);
  - an empty item (`sort=title,,status`, `sort=`).

  The `detail` names the offending item, e.g.:

```json
{
  "status": "400",
  "code": "invalid_sort_field",
  "title": "Bad Request",
  "detail": "`priority` is not a sort field of `tasks`; sort fields are: id, title, status, created",
  "source": { "parameter": "sort" }
}
```

- A list that takes no `order` argument (a user-authored list, or every
  list before phase 3c) answers any `sort` with `400`
  (`invalid_sort_field`), as the spec requires of a server that does not
  support the requested sort.

### 7.5 Include

Phase 3b. `include` is accepted on `GET /api/{type}` and
`GET /api/{type}/{id}`. Every relationship of the primary type can be
included, to-one and to-many alike, one level deep.

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
  call per distinct id. A linkage id that the store reports as not found
  (a dangling markdown wikilink) is left out of `included`. Its identifier
  stays in the linkage, and this is not an error. Why: the markdown
  backend tolerates dangling links by design (ADR 0001 amendment 5), and
  failing a list because one target was deleted would make the whole
  collection unreadable. Any other store error fails the request.
- On a paginated list, only the page's relationships are included.
- `include=` (empty value) is `"included": []`. The spec requires the
  member whenever `include` is given.
- `include=epic,epic` is the same as `include=epic`.
- Each of these is `400` with code `invalid_include_path` and
  `source.parameter: "include"`:
  - a name that is not a relationship of the primary type (`include=owner`);
  - a dotted path (`include=epic.tasks`), since nested inclusion is out of
    scope;
  - `include` on any route other than the two above.
- Before phase 3b, every route answers `include` with this `400`.

Cost: one store `get` per distinct included id. That is acceptable at the
page sizes `max_limit` allows, and the markdown backend already parses the
whole directory per list.

## 8. Single resources

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

When the id does not exist (`GET /api/tasks/nope`), the response is `404`.
The store returns `AppError::TaskNotFound("nope")`:

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
Ontogen accepts any string that is a valid id (row 11 below). Uniqueness
within the type is enforced by the `409` of row 28, and the id's format is
otherwise the consumer's choice (slugs are the norm in markdown vaults).

An id that breaks the validity rule is `400`, not `403`. The spec's `403`
is for a server that does not support client ids at all, and this server
supports them. The id itself is malformed.

The response is always `201` with the full resource, never `204`. The
server can change the resource (the id fill, `before_create` hooks), and
the TS `create` returns the created entity.

Body mapping: `data.id` becomes `CreateXInput.id`, or `""` when absent,
which is today's "derive one" signal. `attributes` become the remaining
non-relation fields. Each to-one `relationships.x.data` becomes the
foreign-key field (`epic_id`). Each to-many `relationships.x.data`
becomes the id vector (`tags`). A relationship absent from the body takes
the field's default, as today's `#[serde(default)]` does.

**Errors**, checked in this order. The first failure is the response
(§13.1). Within one row, members are checked in the order they appear in
the request body.

| # | Condition | Status | `code` | `source` |
|---|---|---|---|---|
| 1 | `Accept` not satisfiable (§3.2) | 406 | `not_acceptable` | `header: "Accept"` |
| 2 | `Content-Type` not acceptable (§3.2) | 415 | `unsupported_media_type` | `header: "Content-Type"` |
| 3 | any query parameter | 400 | `invalid_query_parameter` | `parameter` |
| 4 | body is not JSON | 400 | `invalid_document` | none |
| 5 | top level is not an object | 400 | `invalid_document` | none |
| 6 | `data` missing | 400 | `invalid_document` | `pointer: ""` |
| 7 | `data` not an object | 400 | `invalid_document` | `pointer: "/data"` |
| 8 | `data.type` missing | 400 | `invalid_document` | `pointer: "/data"` |
| 9 | `data.type` not a string | 400 | `invalid_document` | `pointer: "/data/type"` |
| 10 | `data.type` is not this collection's type (e.g. `"epics"` posted to `/api/tasks`) | 409 | `type_mismatch` | `pointer: "/data/type"` |
| 11 | `data.id` present but not a valid id (see below) | 400 | `invalid_document` | `pointer: "/data/id"` |
| 12 | `data.lid` present (unsupported) | 400 | `invalid_document` | `pointer: "/data/lid"` |
| 13 | `IdStrategy::Provided` and `data.id` absent | 400 | `missing_id` | `pointer: "/data"` |
| 14 | `attributes` present but not an object | 400 | `invalid_document` | `pointer: "/data/attributes"` |
| 15 | an attribute name the type does not have, including a relation field written as an attribute (`epic_id`) | 400 | `unknown_attribute` | `pointer: "/data/attributes/{name}"` |
| 16 | a required attribute missing | 400 | `missing_attribute` | `pointer: "/data/attributes"`, or `"/data"` when `attributes` is absent |
| 17 | an attribute value serde rejects | 400 | `invalid_attribute` | `pointer: "/data/attributes/{name}"` |
| 18 | `IdStrategy::SlugFromField(f)`, `data.id` absent, and attribute `f` slugifies to an empty string | 400 | `invalid_attribute` | `pointer: "/data/attributes/{f}"` |
| 19 | `relationships` present but not an object | 400 | `invalid_document` | `pointer: "/data/relationships"` |
| 20 | a relationship name the type does not have | 400 | `unknown_relationship` | `pointer: "/data/relationships/{name}"` |
| 21 | a relationship object that is not an object or has no `data`; `data` of the wrong arity; an identifier without string `type` and `id` | 400 | `invalid_document` | `pointer` at the offending value: `/data/relationships/{name}`, `…/data` or `…/data/{i}` |
| 22 | a `has_many` relationship present | 403 | `relationship_read_only` | `pointer: "/data/relationships/{name}"` |
| 23 | a junction-op relationship present (§9.1) | 403 | `relationship_update_unsupported` | `pointer: "/data/relationships/{name}"` |
| 24 | `data: null` on a non-`Option` to-one | 403 | `relationship_required` | `pointer: "/data/relationships/{name}/data"` |
| 25 | a non-`Option` to-one absent | 400 | `missing_relationship` | `pointer: "/data/relationships"`, or `"/data"` when `relationships` is absent |
| 26 | a linkage identifier of the wrong type | 409 | `type_mismatch` | `pointer: "/data/relationships/{name}/data"` (or `…/data/{i}`) |
| 27 | a linked resource that does not exist | 404 | `related_resource_not_found` | same as 26 |
| 28 | `data.id` already exists | 409 | `{entity}_already_exists` (e.g. `task_already_exists`) | `pointer: "/data/id"` |
| 29 | any other `AppError` | per §13.3 | §13.3 | none |

Rows 1 to 27 are decided by the handler. Rows 28 and 29 come from the
store call.

Notes on the table:

- **Pointers.** A pointer only ever names a value present in the request,
  as the spec requires. For something missing, it names the nearest
  enclosing member that exists (rows 6, 8, 13, 16, 25).
- **Row 11, valid ids.** One rule, on both backends, matching
  `markdown_store::layout::validate_id`. An id is valid when it:
  - is a non-empty string that is not whitespace-only;
  - contains no `/`, `\`, `:` or NUL;
  - does not start with `.`;
  - does not end with `.` or a space.

  Phase 1a applies the rule to SeaORM ids too, for parity. The handler
  checks it, so a bad id is a `400` here instead of a `500` from the store.
  A whitespace-only id is refused rather than treated as absent, which is
  what `IdStrategy` does with it inside the store.
- **Rows 13 and 18.** The handler checks these before calling the store.
  - The servers stage learns the store's `IdStrategy` from the Pipeline, by
    the same fill-if-unset threading E0003 uses for `error_source_dir`. A
    consumer calling `gen_servers` directly sets it on `ServersConfig`.
  - Row 18 uses the same slug function the store uses. Phase 1a shares it
    between the backends.
- **Row 15.** Unknown attributes are refused, not ignored, because the
  commonest cause is a client still sending the flat shape (`epic_id` in
  `attributes`). The `detail` says so: "`epic_id` is not an attribute of
  `tasks`; it is the `epic` relationship".
- **Row 25.** A non-`Option` `belongs_to` has no default in `CreateXInput`,
  so it cannot be omitted. Every other relationship absent from the body
  takes the field's default (`None` or empty), as today's
  `#[serde(default)]` does.
- **Row 27.** The handler checks every linked id with the target's store
  `get` before creating. The spec requires `404` for a reference to a
  resource that does not exist, and the markdown backend would otherwise
  write a dangling wikilink.
- **Row 28.** The store reports a duplicate as
  `AppError::{Entity}AlreadyExists(id)`, a new store-contract variant
  alongside `{Entity}NotFound` (ADR 0004).
  - On markdown it maps `markdown_store::Error::AlreadyExists`.
  - On SeaORM it maps a unique-constraint violation on the primary-key
    insert.
  - Detection is atomic on both backends, so two racing creates cannot
    both succeed and neither can see a `500`.
  - Derived ids never collide: markdown appends `-2`, `-3` and so on, and
    phase 1a gives SeaORM the same rule. So row 28 only arises for a
    client id, and `/data/id` always exists.

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

`PATCH /api/{type}/{id}`. `PUT` is no longer routed and answers `405`
(§13.4).

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
| to-many `data: [...]` | `tags: Some(vec)` | full replacement |
| to-many `data: []` | `tags: Some(vec![])` | cleared |
| `has_many` relationship | — | `403 relationship_read_only` |

An update with no `attributes` and no `relationships` is a no-op. It
returns `200` with the current resource, and the markdown backend performs
no write.

**Response: always `200` with the full resource.** Why 200 and not 204:
`before_update` hooks may change fields the request did not mention, the
spec then requires `200`, and the TS `update` returns the updated entity.

**Errors**, checked in this order. Row numbers refer to §8.2.

1. Rows 1 to 10.
2. `data.id` missing: `400 invalid_document`, `pointer: "/data"`.
3. Row 11 (invalid id).
4. `data.id` differs from the URL id: `409 id_mismatch`,
   `pointer: "/data/id"`.
5. Rows 12, 14, 15, 17, 19 to 24, 26 and 27.
   - Row 17 includes `null` for a non-`Option` attribute.
   - Rows 13, 16, 18, 25 and 28 do not apply to an update.
6. The store call. A missing resource is `404 {entity}_not_found`, with no
   `source`. Any other `AppError` follows §13.3.

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
- `204` and not `200` with `meta`: there is nothing to report, and it is
  today's status.

## 9. Relationship endpoints and related links

Phase 3a. For every relationship of every resource type the server serves:

| Method | Path | To-one | To-many relation field (`many_to_many`) | Junction op | To-many `has_many` |
|---|---|---|---|---|---|
| `GET` | `/api/{type}/{id}/relationships/{rel}` | linkage | linkage | linkage | linkage |
| `PATCH` | same | set or clear | replace | `403` | `403` |
| `POST` | same | `403` | add | add | `403` |
| `DELETE` | same | `403` | remove | remove | `403` |
| `GET` | `/api/{type}/{id}/{rel}` (related link) | resource or `null` | resource collection | resource collection | resource collection |

The spec defines `POST` and `DELETE` only for to-many relationships, and
requires `403` for an unsupported relationship update. Every `403` in the
table therefore uses code `relationship_update_unsupported`, except writes
to `has_many`, which use `relationship_read_only` (§5.3).

`{rel}` is the relationship name verbatim (§5.3). Both route templates
capture it, so a name that is not a relationship of `{type}` reaches the
generated handler and is `404 relationship_not_found`, not a fall-through
to the consumer's router.

Any other method is `405` with `Allow: GET, HEAD, PATCH, POST, DELETE` on
the relationship route and `Allow: GET, HEAD` on the related route. That
holds whatever `{rel}` names, because routing happens before `{rel}` is
looked up.

### 9.1 Where each endpoint's behaviour comes from

**Relation fields** (`belongs_to`, `many_to_many`, `has_many` on the
entity) are served from the generated store alone, so they work on both
backends with no user code:

- `GET …/relationships/{rel}` reads the field from `get_{entity}(id)`.
- `PATCH` writes the field through
  `update_{entity}(id, XUpdate { field: Some(…), ..Default })`, with the
  same mapping as §8.3.
- `POST` (to-many) reads the current ids, appends each requested id that is
  not already present, in request order, and writes the result through
  `update_{entity}`.
- `DELETE` (to-many) reads the current ids, removes each requested id, and
  writes the result through `update_{entity}`.

`POST` and `DELETE` are a read followed by one write, not one transaction.
A concurrent writer to the same relationship can lose an update. That
matches every other read-modify-write in the store today and is recorded
in ADR 0004.

**Junction ops** are user-authored functions in an entity module:

- `list_X(parent_id)`, classified `JunctionList`;
- `add_Y(parent_id, child_id)`, classified `JunctionAdd`;
- `remove_Y(parent_id, child_id)`, classified `JunctionRemove`;

where `Y` is the singular of `X`. Together they define a to-many
relationship named `X`, exactly as written in the fn name: `list_tags` /
`add_tag` / `remove_tag` define `tags`, and `list_sub_tasks` defines
`sub_tasks`.

Classification changes in phase 3a. A one-parameter `list_X` counts as a
junction op only when its module also has `add_Y` or `remove_Y`. Without
either, it is a custom `GET` (§10), served at `/api/{type}/{action}/{param}`.
Today any one-parameter `list_*` is a `JunctionList`, which would turn a
plain `list_by_status(status)` into a relationship named `by_status`.

The relationship's target type is `url_plural` of `list_X`'s element type
when that is an entity. Otherwise it is the entity type whose `url_plural`
is `X` kebab-cased. When neither names an entity type, the generator raises
a `CodegenError`.

Each endpoint behaves as follows:

- **`GET …/relationships/X`** calls `list_X(parent_id)`. When it returns
  entities, linkage is built from their ids. When it returns `Vec<String>`,
  linkage is built from the strings.
- **`POST`** first validates every identifier in the body: its shape, its
  type, and the target's existence. It then calls `list_X(parent_id)` once,
  and calls `add_Y(parent_id, child_id)` for each requested id that is not
  already a member, in request order, once per distinct id.
- **`DELETE`** validates the same way, reads membership the same way, and
  calls `remove_Y` for each requested id that is a member.
- **`PATCH`** is `403 relationship_update_unsupported`. No junction op
  replaces a set, and the spec allows refusing replacement with `403`.

The membership read is what makes an already-present `POST` and an
already-absent `DELETE` succeed without calling user code, as the spec
requires, whatever `add_Y` does with a duplicate.

What the contract cannot give junction ops is atomicity. If `add_Y` fails
on the second of three ids, the first stays added and the response is the
error. The ops are user code with no transaction to join. This is a
recorded deviation from the spec's "a request MUST completely succeed or
fail" (ADR 0004). Clients that need all-or-nothing send one identifier per
request.

**Pagination.** When the module is paginated, junction `GET`s keep today's
in-memory paging:

- The relationship linkage `GET` and the related-link `GET` accept
  `page[offset]` and `page[limit]` with §7.2's rules.
- They return `meta {total, limit, offset}` and the four pagination links
  at the top level, where they paginate the primary data, which is the
  relationship's members.
- Relation-field relationships are never paginated. Their linkage is
  already loaded with the resource.

Generator errors for junction ops, each a `CodegenError` with a message that
says where to move the ops:

- Junction ops in a module that is not an entity module (today's test
  fixture `destination_skills`). The relationship would hang off a `type`
  that has no resources.
- A junction-op relationship whose name equals a relation field's
  relationship name. Both names are compared as written, in snake_case.
  That relationship would have two owners.

A junction-op relationship appears in the resource object's
`relationships` with `links` only and no `data`, which the spec allows.
Linkage would mean calling user code once per resource on every `get` and
`list`. It follows that:

- it cannot be included, so `include` naming it is
  `400 invalid_include_path`;
- it cannot be written in a create or update body, which is
  `403 relationship_update_unsupported` (§8.2 row 23);
- the TS flattener leaves it out of the flat entity, as today, since the
  entity struct has no such field.

**Before phase 3a**, junction ops keep their current paths and are served
as custom ops (§10):

- `JunctionList` at `GET /api/{type}/{parent_id}/{segment}` responds
  `{"meta": {"result": …}}`. The result is today's body: the list, or for a
  paginated module the `{items, total, limit, offset}` object, with
  `limit`/`offset` read from `opArg[limit]`/`opArg[offset]`.
- `JunctionAdd` at `POST` on the same path takes
  `{"meta": {"args": {"<child param>": "<id>"}}}` and responds `204`.
- `JunctionRemove` at `DELETE /api/{type}/{parent_id}/{segment}/{child_id}`
  responds `204`.

### 9.2 Examples

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

**Fetch to-many linkage:** `GET /api/tasks/ship-the-emitter/relationships/tags`
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

**Add, remove and replace to-many:**

```http
POST /api/tasks/ship-the-emitter/relationships/tags HTTP/1.1
Content-Type: application/vnd.api+json

{ "data": [ { "type": "tags", "id": "release" } ] }
```

`204`. Posting an id already present is a no-op and still `204`. `DELETE`
with the same body removes `release`; removing an absent id is also `204`.
`PATCH` with `{"data": []}` clears the relationship.

**Responses.** Every successful mutation is `204 No Content`. Why: the
server makes no change beyond the request, so the spec allows `204`, and
the TS junction methods already return `null`.

**Errors on relationship endpoints**, checked in this order (§13.1):

| # | Condition | Status | `code` | `source` |
|---|---|---|---|---|
| 1 | a method other than `GET`, `HEAD`, `PATCH`, `POST`, `DELETE` | 405 | `method_not_allowed` | none |
| 2 | `Accept` not satisfiable | 406 | `not_acceptable` | `header: "Accept"` |
| 3 | `Content-Type` not acceptable (`PATCH`, `POST`, `DELETE`) | 415 | `unsupported_media_type` | `header: "Content-Type"` |
| 4 | `{rel}` is not a relationship of `{type}` | 404 | `relationship_not_found` | none |
| 5 | any query parameter, except `page[…]` on a paginated junction `GET` | 400 | `invalid_query_parameter` | `parameter` |
| 6 | a write the relationship does not support (table above) | 403 | `relationship_update_unsupported` or `relationship_read_only` | none |
| 7 | body not JSON, or top level not an object | 400 | `invalid_document` | none |
| 8 | `data` missing | 400 | `invalid_document` | `pointer: ""` |
| 9 | `data` of the wrong arity for the relationship | 400 | `invalid_document` | `pointer: "/data"` |
| 10 | an identifier without string `type` and `id` | 400 | `invalid_document` | `pointer: "/data"` (to-one) or `"/data/{i}"` |
| 11 | an identifier of the wrong type | 409 | `type_mismatch` | same as 10 |
| 12 | `null` on a non-`Option` to-one | 403 | `relationship_required` | `pointer: "/data"` |
| 13 | a linked resource that does not exist (`PATCH`, `POST`) | 404 | `related_resource_not_found` | same as 10 |
| 14 | parent resource missing | 404 | `{entity}_not_found` | none |
| 15 | any other `AppError` from the store or a junction op | per §13.3 | §13.3 | none |

Row 8 is where E0003 phase 1's "missing junction parameter → 400" lands: a
junction add without its child id is a body with no `data`.

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
- Related collections are not filtered or sorted, and are paginated only
  for a junction op in a paginated module (§9.1). Any other query
  parameter is `400`.

### 9.4 Route shadowing

A custom op's static segment wins over `{id}` in Axum's router. For
illustration, a custom `POST /api/workouts/start` shadows `PATCH` on a
workout whose id is `start`. A custom `GET /api/workouts/summary/{id}`
shadows the related link of every relationship on a workout with id
`summary`.

This is true today for `/{id}`. The contract records it and does not fix
it.

## 10. Custom ops and singletons

### 10.1 Responses: meta-only documents

Every custom op (`CustomGet`, `CustomPost`) responds with a meta-only
document (decision 1). iron-log's `stats::get_workout`:

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
  bare body was. It is a resource-free value even when `T` is an entity or
  a `Vec` of entities. Decision 1 keeps custom ops out of resource
  semantics, and the client types come from the Rust fn.
- A fn returning `()` responds `204 No Content`, as today.
- The status is `200`, never `201`.
- No `links` and no `data`: a meta-only document has no primary data, so
  the spec does not call for a `self` link.
- Errors use §13. `AppError`-typed fns get the status mapping. Other error
  types get `500 internal_error`.

Example: iron-log's `stats::get_workout` stays at `GET /api/stats/workout`
and now returns `{"jsonapi":…, "meta": {"result": WorkoutStats}}`.

### 10.2 Requests

Routes are unchanged: `/api/{url_for_module}/{action}`, plus one path
segment per required non-`Input` parameter on `GET`.

**`POST` bodies are meta-only documents.** Every argument that is not
state, store or a path parameter is a member of `meta.args`, keyed by its
Rust parameter name. That includes an `*Input` argument and `Option`
arguments. For illustration, a `workout::start(state, input: StartWorkoutInput)`
op, the shape `src/servers/tests.rs` uses:

```http
POST /api/workouts/start HTTP/1.1
Content-Type: application/vnd.api+json
Accept: application/vnd.api+json

{ "meta": { "args": { "input": { "template_id": "push-day" } } } }
```

Rules for `POST` bodies:

- An absent or `null` member is `None` for an `Option` argument.
- A body that is not a JSON object is `400 invalid_document` with no
  `source`.
- When `meta`, or `meta.args`, is missing or not an object, the request is
  `400 invalid_document`. The pointer names the nearest member that exists:
  `""` when `meta` is missing, `/meta` when `args` is missing. If the fn
  has no required arguments, the missing member is read as `{}` instead.
- A missing required argument is `400 invalid_document` with
  `pointer: "/meta/args"`.
- An unknown member of `meta.args` is `400 invalid_document` with
  `pointer: "/meta/args/{name}"`.
- A value serde rejects is `400 invalid_document` pointing at its member.
- Members are checked in body order. Missing required arguments are
  checked after every present member.
- A request with no body is read as `{"meta": {"args": {}}}`. It needs no
  `Content-Type`; §3.2's check applies only when a body is present. A body
  sent with any other media type is still `415`.

Why one shape: today a custom `POST` takes either a bare `*Input`, a
generated `{Fn}Body` struct, or query parameters for `Option` arguments.
A JSON:API request document needs a top-level `data`, `errors` or `meta`
member. `meta.args` gives every custom `POST` one rule that a generic
client can follow.

**`GET` optional arguments use the `opArg` family.** `Option` arguments of a
`CustomGet` are `opArg[{name}]` query parameters. For illustration,
`get_summary(state, id: &str, verbose: Option<bool>)` is served as
`GET /api/workouts/summary/w1?opArg[verbose]=true`.

- Each value is deserialized by serde from the string, as `Query` does
  today.
- An unknown `opArg[…]`, a malformed value, or any other parameter is
  `400 invalid_query_parameter`.

Why a new family: today's bare names (`?verbose=true`) are all-lowercase,
and the spec requires `400` for such a name unless the spec itself defines
it. An implementation-specific family must contain a non `a-z` character,
and the spec recommends a capital letter. `opArg` is that family, and the
only one ontogen defines.

### 10.3 Singletons

A singleton module (`// ontogen:singleton`, or
`NamingConfig::singleton_modules`) has no resource type. Its routes stay
`/api/{singular}/{action}`. Every op in it is a custom op and follows
§10.1 and §10.2.

An op in a singleton module that classifies as `List`, `GetById`,
`Create`, `Update`, `Delete` or a junction op is a `CodegenError`. It would
need a `type`, and a singleton has none.

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
  served from its own store and no document mixes scopes, so each scope
  acts as a separate API in the spec's sense, within which `(type, id)` is
  unique.
- A prefix parameter that fails to parse (e.g. a non-UUID `project_id`) is
  `400 invalid_path_parameter`, with no `source`. The spec has no source
  member for path segments.
- A failing scope accessor (`state.store_for(&project_id)`) stays
  `500 internal_error`, as E0003 phase 1 keeps it. Mapping it to `404` waits
  on E0003's store-accessor contract.

Scoped and unscoped routes MUST have identical wire behaviour. Today they
diverge in two places, and phase 1 and phase 3a remove both divergences:

- Scoped pagination slices in memory instead of calling the page-taking
  list.
- Scoped junction ops become action-style routes instead of the
  `{parent_id}/{child}` form.

### 11.2 Extra API surfaces

Routes and types come from module names exactly as for the primary
surface: no path prefix per surface, and one type per module even when the
module spans surfaces. Each surface's `store_accessor` and `pagination`
apply as today. The wire is indistinguishable from a single-surface server.

## 12. Event streams

Routes, parameters, resume and lag are unchanged from
[#184](https://github.com/sksizer/rust-ontogen/pull/184) and
[#185](https://github.com/sksizer/rust-ontogen/pull/185):

- `GET /api/events/{event-name}` plus `/{param}` per required parameter.
  `sse_route_overrides` still apply. A `route_prefix` puts the routes under
  the prefix.
- Required parameters are path segments: iron-log's `activity_for_kind`
  is `GET /api/events/activity-for-kind/{kind}`.
- `Option` parameters and `resume` are plain query parameters
  (`?resume=4`). The `Last-Event-ID` header wins over `?resume=`.
- These routes respond `text/event-stream`, not a JSON:API document, so §3
  negotiation and §6 query rules do not apply to them. `Accept` is not
  checked. `EventSource` sends `text/event-stream`.

**Frames.** Only the `data:` payload changes (decision 2). tasks-tracker has
no event ops. For illustration, a resumable `task_changed` op yielding
`Task`, with ids from `seq_id`:

```text
event: task-changed
id: 0:17
data: {"type":"tasks","id":"ship-the-emitter","attributes":{"title":"Ship the emitter","status":"closed/done","created":"2026-06-06","body":"## Goal\n…"},"relationships":{"epic":{"data":{"type":"epics","id":"markdown-backend"}},"tags":{"data":[{"type":"tags","id":"codegen"}]}}}
```

- When the event's item type `T` is a schema entity, `data:` is its
  resource object (§5): `type`, `id`, `attributes`, `relationships`, in
  that order.
- Relationship objects carry `data` only, and the frame carries no
  `links`. Junction-op relationships, which have no `data` (§9.1), are
  left out. A frame is not tied to a request URL, and links would double the
  size of every frame for no reader.
- When `T` is not an entity (iron-log's `Activity`, whose `event_id` is
  its `seq`), `data:` is `{"meta":{"result":T}}`. That is the same rule as
  custom ops (§10.1), so a client has one rule for every non-resource
  payload:

```text
event: activity-for-kind
id: 4
data: {"meta":{"result":{"seq":4,"kind":"workout","id":"w1"}}}
```

These lines are unchanged:

- `id:` is emitted for resumable ops only, with the same `seq_id` / `no_id`
  rules and the same newline guard.
- `event: lag` with `data: {"skipped":N}` keeps the stream open. It is a
  control frame, not a payload, so it gets no envelope.
- `event: error` on a serialization failure, and the `:` keep-alive
  comment every 15 s.

A subscribe call that fails before the stream opens returns a JSON:API
error document (§13) with the mapped status. `EventSource` cannot read
that body, but a `fetch`-based client and `curl` can.

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
  failure. The spec allows this, and it keeps the response status and the
  error status equal.
- **Members**, in this order:
  - `status`: the HTTP status as a decimal string (`"404"`). Always present.
  - `code`: a snake_case string from §13.2 or §13.3. Always present.
  - `title`: the status's reason phrase from RFC 9110 (`"Not Found"`,
    `"Conflict"`). Always present. It is constant per status, which
    satisfies the spec's "SHOULD NOT change from occurrence to occurrence".
  - `detail`: an occurrence-specific sentence. For an `AppError` it is the
    error's `Display` text, as today's `{"error": …}` carried. Always
    present.
  - `source`: present only when one of the three members below applies.
- **`source`** has exactly one member:
  - `pointer`: an RFC 6901 pointer into the request body. It only points at
    a value that exists in the request, as the spec requires. For a missing
    member, it points at the object that should have held it
    (`/data/attributes`).
  - `parameter`: the query parameter's name, decoded, with raw brackets
    (`"page[limit]"`).
  - `header`: `"Content-Type"` or `"Accept"`.
- Error objects carry no `id`, `links` or `meta`. Nothing ontogen knows
  would fill them.
- **`status`, `code`, `title` and `source` are normative. `detail` is
  not.** Its wording may change between releases, and clients must not
  parse it. The `detail` strings in this document are examples.
  Generator tests may pin them.

**Check order.** Every route checks in this order, and the first failure is
the response:

1. Routing: a method the route does not serve is `405` (§13.4).
2. `Accept` (§3.2): `406`.
3. `Content-Type`, when the request has a body (§3.2): `415`.
4. Path parameters: a typed prefix parameter that fails to parse is
   `400 invalid_path_parameter`. On relationship and related routes, an
   unknown `{rel}` is `404 relationship_not_found`.
5. Query parameters. A name the route does not accept comes first, the
   first such name in request order. The accepted parameters follow, in
   canonical order (§4.3): `filter[…]`, `sort`, `include`, `page[offset]`,
   `page[limit]`, then `opArg[…]` in byte order of member name.
6. Operation-level refusals decidable from the route alone: the `403`s of
   §9's table.
7. The request body, in the order of the operation's table (§8.2, §8.3,
   §9.2, §10.2).
8. Store reads the handler makes before acting: linked-resource existence
   (`404 related_resource_not_found`), then the parent resource on
   relationship routes (`404 {entity}_not_found`).
9. The operation itself: the store call or the custom op, and its
   `AppError`.

So `GET /api/tasks/nope?include=owner` is `400 invalid_include_path`, not
`404`, and `GET /api/tasks?sort=priority&page[limit]=0` is the `sort` error.
`sort` and `include` are accepted names wherever §6 lists them, in every
phase. A route that cannot yet honour them answers with their own codes
(§7.4, §7.5), not `invalid_query_parameter`.

### 13.2 Ontogen-authored errors

Errors the generated server raises itself, independent of the consumer's
`AppError`:

| Status | `code` | Raised when |
|---|---|---|
| 400 | `invalid_query_parameter` | unknown, repeated or malformed query parameter, including `page`, `filter`, `opArg` and `fields` (§6, §7, §10.2) |
| 400 | `invalid_sort_field` | bad `sort` (§7.4) |
| 400 | `invalid_include_path` | bad `include` (§7.5) |
| 400 | `invalid_path_parameter` | a typed path parameter fails to parse (§11.1) |
| 400 | `invalid_document` | the body is not JSON, or is not a valid request document for the route |
| 400 | `missing_id` | `IdStrategy::Provided` and no `data.id` (§8.2) |
| 400 | `unknown_attribute` / `missing_attribute` / `invalid_attribute` | attribute problems (§8.2, §8.3) |
| 400 | `unknown_relationship` | a relationship name the type lacks (§8.2) |
| 400 | `missing_relationship` | a non-`Option` to-one absent from a create body (§8.2) |
| 403 | `relationship_read_only` | a write to a `has_many` relationship (§5.3) |
| 403 | `relationship_required` | `null` on a non-`Option` to-one (§8.3, §9) |
| 403 | `relationship_update_unsupported` | a relationship write the relationship does not support: `PATCH` on a junction op, `POST`/`DELETE` on a to-one, or a junction-op relationship in a create or update body (§8.2, §9) |
| 404 | `related_resource_not_found` | a linked id that does not exist (§8.2, §9) |
| 404 | `relationship_not_found` | `{rel}` is not a relationship of the type (§9) |
| 405 | `method_not_allowed` | a method the route does not serve (§13.4) |
| 406 | `not_acceptable` | §3.2 |
| 409 | `type_mismatch` | a `type` that is not the endpoint's or the relationship's (§8.2, §8.3, §9) |
| 409 | `id_mismatch` | `PATCH` body id differs from the URL id (§8.3) |
| 415 | `unsupported_media_type` | §3.2 |
| 500 | `internal_error` | a store-construction or scope-accessor failure, or a custom op whose error type is not `AppError` |

The `detail` for `internal_error` is the error's `Display` text, as today.

### 13.3 `AppError`: E0003 phases 0-1, folded in

Decision 6 folds E0003 phases 0 and 1 into E0004 phase 1:

- the `ApiFn.error_type` capture;
- the `AppError` scan over the schema directory;
- the call-site routing predicate (last path segment `AppError`).

The mapping that scan produces:

| `AppError` variant | Status | `code` |
|---|---|---|
| name ends in `NotFound` (`TaskNotFound`) | 404 | variant name in snake_case: `task_not_found` |
| name ends in `AlreadyExists` (`TaskAlreadyExists`) | 409 | `task_already_exists` |
| any other variant (`Md`, `DbError`) | 500 | `md`, `db_error` |

- **`code` is the variant name**, converted to snake_case at generation
  time. Clients can branch on it without parsing `detail`, and one rule
  covers every variant, mapped or not. A variant whose snake_case name
  equals a §13.2 code (an `InvalidDocument` variant, say) is a
  `CodegenError`, so a code always means one thing.
- **`*AlreadyExists → 409` is new.** It is the twin of the `*NotFound`
  convention, for the duplicate-create case (decision 4). The store
  generator constructs `{Entity}AlreadyExists(id)` on both backends, so
  every consumer `AppError` must declare it, as it already declares
  `{Entity}NotFound`. tasks-tracker gains `TaskAlreadyExists`,
  `EpicAlreadyExists` and `TagAlreadyExists`.
- **No `AppError` in the schema directory** (the scan-dirs-only use case):
  every `AppError`-typed site maps to `500` with code `internal_error`.
  E0003's "emit today's exact shape" rule no longer applies, because the
  envelope changes regardless.
- **E0003 phases 2 and 3** stay in E0003 and slot into this document
  unchanged:
  - `#[http(status = N)]` annotations override the convention. The
    `title` becomes N's reason phrase and the `code` stays the variant
    name.
  - An `error_handler` override owns the whole response. The consumer then
    owns conformance too.

### 13.4 Statuses the spec mandates, and where each arises

| Status | Spec trigger | Where in this contract |
|---|---|---|
| 400 | unknown or unsupported query parameter; unsupported `include` or `sort` | §6, §7 |
| 403 | unsupported create or update; refused to-many replacement | §5.3, §8, §9 |
| 404 | missing resource, relationship or related resource | §8, §9 |
| 405 | (HTTP) a method the route does not serve | below |
| 406 | unsatisfiable `Accept` | §3.2 |
| 409 | duplicate client id; type or id mismatch | §8.2, §8.3, §9 |
| 415 | bad `Content-Type` | §3.2 |

**405.** A generated route that does not serve a method answers `405` with
an `Allow` header listing the methods it does serve, and an
`errors[]` body with code `method_not_allowed`. `HEAD` is listed wherever
`GET` is, because Axum serves it. This covers:
- `PUT` on `/api/tasks/{id}` (`Allow: GET, HEAD, PATCH, DELETE`);
- `PUT` on `/api/tasks` (`Allow: GET, HEAD, POST`);
- `PUT` on a relationship route (§9).

`POST` or `DELETE` on a to-one relationship is not a `405`. The route
serves those methods, and the spec makes an unsupported relationship
update `403` (§9).

The generated router installs this as its method-not-allowed fallback. It
installs no path fallback: an unmatched path falls through to the
consumer's router, which owns its own `404`.

**Axum extractor rejections.** Rejections from Axum's own extractors (path,
query, body) never reach the client in Axum's plain-text form. The
`ontogen-jsonapi` extractors replace `Json`, `Query` and `Path` in
generated handlers and produce the documents above.

## 14. TS transport mapping

The generated HTTP transport keeps the flat `Transport` interface: every
method name, parameter list and return type is unchanged. JSON:API is
applied and removed inside the transport. The admin layer needs no source
change.

### 14.1 Requests

- Every request sends `Accept: application/vnd.api+json`.
- Every request with a body sends
  `Content-Type: application/vnd.api+json`.
- The `httpPut` helper is removed, and `httpPatch` replaces it.

### 14.2 Per-operation mapping

| `Transport` method | HTTP | Flat result built from |
|---|---|---|
| `xList(query?)` (unpaginated) | `GET /api/{type}` + `filter[k]=v` per defined key of `query` | `data.map(flatten)` → `R[]` |
| `xList(query?, limit?, offset?)` (paginated) | adds `page[offset]`, `page[limit]` when defined | `{ items: data.map(flatten), total: meta.total, limit: meta.limit, offset: meta.offset }`, which is today's `PaginatedResult<R>` unchanged |
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
| `subscribeX(args, handlers)` | unchanged URL, `?resume=` and lag | entity `T`: `flatten(JSON.parse(data))`; other `T`: `.meta.result` |

The junction rows describe phase 3a. Between phases 1b and 3a, junction
methods call the custom-op forms of §9.1 ("Before phase 3a") and read
`meta.result`.

**Queries.** `toQueryString` gains a family form: `toQueryString({ filter: query, page: { offset, limit } })`
emits `filter%5Bstatus%5D=…` and so on, skipping `null` and `undefined`.
Arrays are not supported in filters, matching §7.3.

The transport sends neither `sort` nor `include`. Why: adding them to
`xList` would change the `Transport` signature that the admin layer calls
positionally. A later options-object parameter can expose them.
Third-party JSON:API clients use them directly.

### 14.3 Flatten and unflatten

The generator emits one pair of functions per entity, from the same
relationship table as the server (§5.3). For `Task`:

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

- **`unflatten(input, id?)`** builds `{ data: { type, id?, attributes, relationships } }`.
  - `id` goes in `data.id` when the argument is given (update) or when
    `input.id` is a non-empty string (create). It never goes in
    `attributes`, whatever its value, including `""`.
  - Relation fields move to `relationships`: a to-one value `v` becomes
    `{ data: v == null ? null : { type, id: v } }`, and a to-many array
    becomes an identifier array.
  - Every other key goes to `attributes`.
  - Keys whose value is `undefined` are omitted. On update that means
    "unchanged", matching `UpdateXInput`. `null` is kept and clears the
    field.
- **`has_many` fields are dropped from create and update bodies.** The
  server refuses them (§5.3), and an admin edit form that round-trips the
  whole entity would otherwise fail. This is the one behaviour change a
  caller can observe: a `has_many` edit over HTTP is now ignored by the
  transport instead of partly applied by the server. IPC is unaffected.
  Phase 1 records it in the changelog.

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
  it reads `"Error: …"`. No admin-layer test pins that prefix. The admin
  layer only renders `String(e)`.
- Callers that want the status or the code read `e.status` and
  `e.errors[0].code`. Today neither is available.
- A non-JSON error body (a proxy's HTML `502`) still throws a
  `JsonApiError`, with `errors: []`.

### 14.5 What stays identical for callers

- Every method name and signature on `Transport`.
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
generated output, so phase 1 regenerates it.

## 15. IPC and MCP

Unchanged and flat. JSON:API exists only at the HTTP boundary:

- **Tauri IPC**: same commands, same `invoke` argument objects, same flat
  entities, `PaginatedResult` for paginated lists, `String` errors, and the
  same `EventFrame` channel (`{kind:"event", id, data}` with a flat `data`,
  and `{kind:"lag", skipped}`).
- **MCP**: same tool names, the same flat argument schemas
  (`schema_for_with_str_id` for update), the same results
  (`{"success": true}` for create and update, entities for get, bare arrays
  or `{items, total, limit, offset}` for lists), and event ops still
  skipped.

Two store-level changes reach them without changing a payload:

- The new `{Entity}AlreadyExists` and the `order` argument (ADR 0006).
- IPC and MCP list handlers pass an empty order (`&[]`), so they get the
  default id-ascending order and expose no new argument.
- A duplicate create now fails with the `AlreadyExists` text instead of the
  backend's message. Both are strings on these transports.

## 16. Decision index

Each choice this contract makes where the spec or the epic left one open,
with its reason.

| # | Choice | Why |
|---|---|---|
| C1 | `jsonapi: {"version":"1.1"}` on every document | Without it clients assume 1.0, and the member costs nothing |
| C2 | `application/json` request bodies are `415` | One media type, and old clients fail loudly instead of mis-parsing |
| C3 | `*/*` and `application/*` satisfy `Accept` | Browsers, `curl` and `EventSource` send them |
| C4 | `Vary: Accept` on every response | The response depends on `Accept` (406) |
| C5 | Links and `Location` are relative | The server cannot know its public origin |
| C6 | Canonical link query order and encoding | Byte-stable links for snapshots and caches |
| C7 | `links.self` on every resource object | `Location` must match it, and clients can refetch without building URLs |
| C8 | `relationships` omitted when a type has none | Avoids an empty object on every `tags` and `epics` resource |
| C9 | Relationship name: `belongs_to` loses `_id`, others keep their field name | Matches how the field reads, and JSON:API names relationships, not keys |
| C10 | `has_many` is read-only on the wire | The store cannot do the full replacement JSON:API requires |
| C11 | Unknown attributes are `400` | Catches clients still sending the flat shape |
| C12 | Offset pagination with `page[offset]`/`page[limit]` | Same semantics as today's store signature |
| C13 | `page[limit]=0` is `400` | A zero page has no `next` or `last` |
| C14 | `meta: {total, limit, offset}` on paginated lists only | Exactly rebuilds `PaginatedResult`, and is redundant when unpaginated |
| C15 | All four pagination keys always present, `null` when unavailable | One shape to read |
| C16 | `prev` past the end points at the last page | An overshooting client can step back to data |
| C17 | Filter names are the `*Query` struct's field names | The struct is user-authored, and ontogen does not rename it |
| C18 | `id` is the implicit last sort key | A total order makes pages stable (ADR 0006) |
| C19 | Dangling linkage is skipped in `included` and related links, not an error | Markdown tolerates dangling wikilinks by design |
| C20 | A client id is honoured under every `IdStrategy`; no `403` | Decision 4 |
| C21 | `{Entity}AlreadyExists` store variant, mapped `409` by convention | Atomic duplicate detection on both backends, and typed for IPC and MCP |
| C22 | `Provided` without an id is checked in the handler | The store's `IdStrategy` is known at generation time, so no `AppError` variant is needed |
| C23 | Create is always `201` with the document | Ids and hooks change the resource, and TS returns the entity |
| C24 | `PATCH` is always `200` with the document | Hooks may change the resource, and TS returns the entity |
| C25 | Delete and relationship mutations are `204` | Nothing to report, and TS returns `null` |
| C26 | Junction ops only in entity modules, never beside a same-named relation field, and a lone `list_X` is a custom op | Every relationship needs a resource type and one owner, and a plain list must not become one |
| C27 | Junction-op relationships refuse `PATCH` with `403` | No junction op replaces a set |
| C28 | Custom `POST` bodies are `{meta:{args:{…}}}` | One rule, and a valid JSON:API request document |
| C29 | Custom `GET` optional args use the `opArg[…]` family | All-lowercase names are reserved by the spec |
| C30 | CRUD ops in a singleton module are a `CodegenError` | A singleton has no `type` |
| C31 | Non-entity event payloads and custom results are `{meta:{result}}` | One rule for every non-resource payload |
| C32 | Event frames carry no links | A frame has no request URL, and links would double its size |
| C33 | One error object per response | Status and error agree, and the server stops at the first failure |
| C34 | `code` is the `AppError` variant in snake_case, or a fixed ontogen code | Machine-readable without parsing `detail` |
| C35 | `title` is the reason phrase of the status | Constant per problem, as the spec asks |
| C36 | The TS transport does not expose `sort` or `include` | Keeps the `Transport` signature the admin layer calls positionally |
| C37 | `JsonApiError` carries `status` and `errors`, with the old message | Admin output is unchanged, and callers can branch on status |
| C38 | No path fallback; `405` via a method fallback | The generated router is merged into a consumer router it must not hijack |
| C39 | A JSON:API instance in `Accept` outranks wildcards | The spec requires `406` when every such instance is unusable |
| C40 | One id-validity rule on both backends, checked in the handler (`400`) | A malformed id is a bad request, not a store `500`, and the backends agree |
| C41 | Unsupported relationship methods (to-one `POST`/`DELETE`, junction `PATCH`) are `403`, not `405` | The spec requires `403` for an unsupported relationship update |
| C42 | Junction `POST`/`DELETE` read membership first; partial failure is a recorded deviation | Gives the spec's idempotent success without trusting user code, which has no transaction to join |
| C43 | Paginated junction lists stay paginated on the relationship routes | Keeps `xListX`'s `PaginatedResult` signature |
| C44 | One check order for every route (§13.1) | Every request has exactly one correct error |
| C45 | Shape-changing serde attributes on entities are a `CodegenError` | Attribute, sort and filter names all assume field name = member name |
| C46 | `detail` is not normative | Wording can improve without a contract change, and clients branch on `code` |

## 17. Phase mapping

| Section | Phase |
|---|---|
| §3 media type, §4 documents, §5 resource objects (relationship `data` only), §6 query rules | 1b |
| §7.1, §7.2 list and pagination | 1b; the id-ascending default order (ADR 0006 §3) in 1a |
| §7.3 filter | 2 |
| §7.4 sort | 3c (store side per ADR 0006) |
| §7.5 include | 3b |
| §8 get, create, update, delete, `missing_id`, `*AlreadyExists`, related-resource existence checks | 1b; `IdStrategy` on SeaORM, the shared id-validity rule and slug function, and the `AlreadyExists` store variant in 1a |
| §9 relationship endpoints, related links, relationship `links`, junction classification change | 3a; junction ops as custom ops in 1b |
| §10 custom ops, `meta.args`, `opArg`, singleton check | 1b |
| §11 route prefix (including removal of the scoped pagination divergence) | 1b; scoped junction routes in 3a |
| §12 event frames | 1b |
| §13 errors and the E0003 phase 0-1 scan | 1b |
| §14 TS transport | 1b; junction methods in 3a |
| `ontogen-jsonapi` runtime crate (document types, extractors, link building, error document) | 1a |
