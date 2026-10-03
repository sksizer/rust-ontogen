# ADR 0006 — Ordering on both store backends

## Status

**proposed** (2026-10-03).

This ADR revisits [ADR 0001](0001-markdown-as-store-backend.md) contract
item 3 and amendment 4, which left `OrderBy` out of the store until "a
future ADR that changes both backends together". It is that ADR. Number
0005 is reserved for the OKF vault decision of epic
[E0005](../planning/epics/okf-markdown-vault.md).

It builds on PR [#178](https://github.com/sksizer/rust-ontogen/pull/178)
(merged), which orders SeaORM lists and has_many loads by id.

## Context

**Why ordering is needed now.** Two consumers of the store need ordering it
does not provide:

- **`sort` on every transport.** Epic [E0004](../planning/epics/jsonapi-http-transport.md)
  decision 6 puts JSON:API `sort` in scope, and decision 8 exposes the
  same sort on Tauri IPC and MCP. The [wire contract](../jsonapi-wire-contract.md)
  §7.4 specifies the HTTP form. A sort applies before a page is cut, so it
  cannot be done above the store.
- **Stable pagination across backends.** A page is only well defined when
  the order under it is total. Each backend is deterministic today, but
  the two orders differ, so the same page request can return different
  records by backend.

**What the store does today.**

- **Signatures.** Each entity gets
  `list_{plural}(&self, limit: Option<u64>, offset: Option<u64>)` and
  `count_{plural}(&self)`. There is no filter and no order at the store.
  Filters live above it, in hand-written API functions and their `*Query`
  structs. PR [#172](https://github.com/sksizer/rust-ontogen/pull/172)
  requires a paginated `count` to take the same filter as `list`.
- **SeaORM.** Since #178, `list_*` emits `order_by_asc(Column::{Id})`
  before `LIMIT`/`OFFSET` (`src/store/backends/seaorm/gen_crud.rs:68`), and
  the has_many child load in `populate_*_relations` orders by id too
  (`gen_crud.rs:345`). #178 scopes this to per-backend determinism and
  declines cross-backend parity, for two reasons. SQL text ordering follows
  column collation while the vault sorts bytewise. The vault orders by
  path, not id.
- **SeaORM many_to_many.** Linkage comes from the consumer-owned
  `load_junction_ids` helper. The iron-log example's copy selects with no
  `ORDER BY` (`examples/iron-log/src-tauri/src/store/mod.rs:100`), so its
  order is whatever SQLite returns.
- **Markdown.** `markdown_store::walk` sorts paths by their
  extension-stripped form (`crates/markdown-store/src/walk.rs:82`). The
  store then parses every record and pages in memory with skip/take. It
  populates relations for the page only. For a flat `PerEntityDir`
  directory, path order equals id order. For a nested layout it does not,
  so ADR 0001's "lexicographic by record id" is not quite what ships.
  many_to_many linkage is the frontmatter list, in written order.
- **Nulls.** `Option` fields are SQL `NULL` on SeaORM. On markdown they are
  an absent frontmatter key, read back as `None`.
- **Field types** (`ontogen_core::model::FieldType`): `String`, `I32`,
  `I64`, `F32`, `F64`, `Bool`, their `Option` forms, `OptionEnum`,
  `VecString`, `VecStruct`, and `Other(_)`. The schema parser
  (`src/schema/parse.rs:423`) classifies every `Option<T>` whose `T` is not
  one of the six primitives as `OptionEnum(T)`, so `Option<u32>` and
  `Option<SomeStruct>` are `OptionEnum` as well as `Option<SomeEnum>`. A
  bare type it does not recognise, `u32` included, is `Other(T)`. There is
  no date type: dates are `String`, as in tasks-tracker's `created`.
- **Tests.** `tests/backend_parity.rs` compares generated text across
  backends. Nothing runs both backends against the same data.

**Why ADR 0001 deferred it.** Item 5 makes `gen_api` / `gen_servers` /
`gen_clients` output byte-identical across backends. An ordering argument
changes the generated `list_*` signature that the API layer calls, so it
must change on both backends at once, with the same types. That is the only
reason ADR 0001 gave. It is satisfied by doing exactly that.

## Decision

### 1. Store API

A runtime module in `ontogen-core`, which generated code already depends
on. Unlike `ontogen_core::events`, which is behind the `events` feature
(`crates/ontogen-core/Cargo.toml`, `crates/ontogen-core/src/lib.rs:14`),
`ontogen_core::order` is not feature-gated: every generated store uses it,
so a feature would always be on.

```rust
// ontogen_core::order
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction { Asc, Desc }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderBy<F> { pub field: F, pub direction: Direction }

impl<F> OrderBy<F> {
    pub const fn asc(field: F) -> Self { Self { field, direction: Direction::Asc } }
    pub const fn desc(field: F) -> Self { Self { field, direction: Direction::Desc } }
}

/// An entity's sortable fields. Generated; never implemented by hand.
pub trait SortField: Copy + Eq + 'static {
    const ALL: &'static [Self];
    fn name(self) -> &'static str;
    fn from_name(name: &str) -> Option<Self>;
}

/// A sort key that names no sortable field, repeats one, or is empty.
/// Carries the offending key as the caller sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortError {
    UnknownField(String),
    DuplicateField(String),
    EmptyKey,
}

impl std::fmt::Display for SortError { /* "unknown sort field `x`", … */ }
impl std::error::Error for SortError {}

/// Parses transport sort keys: `name` is ascending, `-name` descending.
pub fn parse_sort<F: SortField>(
    keys: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<OrderBy<F>>, SortError>;
```

The store stage emits one enum per entity, beside `{Entity}Update`, from a
backend-independent emitter so its text is identical on both backends:

```rust
// crate::store::task
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskSortField { Id, Title, Status, Created }

impl ontogen_core::order::SortField for TaskSortField {
    const ALL: &'static [Self] = &[Self::Id, Self::Title, Self::Status, Self::Created];
    fn name(self) -> &'static str { match self { Self::Id => "id", Self::Title => "title", Self::Status => "status", Self::Created => "created" } }
    fn from_name(name: &str) -> Option<Self> { Self::ALL.iter().copied().find(|f| f.name() == name) }
}
```

`list` gains the order as its first argument after `&self`. `count` is
unchanged, because order does not change a count:

```rust
pub async fn list_tasks(
    &self,
    order: &[OrderBy<TaskSortField>],
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<Task>, AppError>;

pub async fn count_tasks(&self) -> Result<u64, AppError>;
```

The generated API forwarders (`src/api/gen_crud.rs`) pass the order
through:

```rust
pub async fn list(store: &Store, order: &[OrderBy<TaskSortField>]) -> Result<Vec<Task>, AppError>;
// paginated modules:
pub async fn list(store: &Store, order: &[OrderBy<TaskSortField>], limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Task>, AppError>;
```

The servers stage recognises a list parameter whose type is
`&[OrderBy<…>]` the way it recognises page parameters today. That gives a
`takes_order()` predicate beside `takes_page()`:

- An `order` parameter is not a filter parameter. The #172 rule that
  `count` takes the same filters as `list` ignores it.
- Every transport exposes it as an optional list of sort keys, `name` for
  ascending and `-name` for descending, and builds `order` with
  `parse_sort`. An absent sort, or an empty list, is `&[]`, the default
  order (§3).
- **HTTP** reads the `sort` query parameter (wire contract §7.4), splits
  its value on `,`, and passes the parts to `parse_sort`. A `SortError`
  is a 400 `invalid_sort_field`.
- **Tauri IPC.** The list command gains an optional
  `sort: Option<Vec<String>>` argument.
- **MCP.** The list tool's input schema gains an optional `sort` array
  property whose items are enumerated from the entity's sort keys.
- IPC and MCP return a `SortError`'s `Display` text as the error string.
  Their payloads gain one optional field and stay flat.
- **TS clients.** The clients stage emits a string-literal union per
  entity, `TaskSortKey = 'id' | '-id' | 'title' | '-title' | …`, and the
  `Transport` list methods gain a trailing optional options argument
  `{ sort?: TaskSortKey[] }`. HTTP sends it as `sort=a,-b`; IPC and MCP
  send it as the `sort` array.
- A hand-written list opts into sort by declaring the same parameter
  (§1.1).

Why one parser: the three transports accept the same keys and reject the
same mistakes, so the rule lives in one function rather than three
generated copies.

Why a borrowed slice: an empty slice is the default order, with no
`Option` to unwrap and no allocation, and a list of keys is what both SQL
and an in-memory comparator consume.

Why a generated enum rather than field-name strings: an invalid field is
unrepresentable below the transport boundary. Neither backend validates names,
and a field rename is a compile error in every caller.

### 1.1 Filtered lists

The store has no filter. A filtered list is a hand-written API function in
`api_dir/{module}.rs`. For its order to follow the same rules as `list_*`,
it needs the parameter shape the servers stage recognises and the
comparator the store uses.

**Parameter order.** A hand-written list declares

```rust
pub async fn list(
    store: &Store,
    /* filter params… */
    order: &[OrderBy<TaskSortField>],
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<Task>, AppError>;
```

`order` comes after the filter parameters and before the page parameters.
The paginated `count` takes the filter parameters but not `order`, since
the #172 rule ignores `order`.

**`sort_{plural}`.** The store stage emits, from the backend-independent
emitter and so byte-identical across backends, a free function beside
`{Entity}SortField`:

```rust
pub fn sort_tasks(items: &mut [Task], order: &[OrderBy<TaskSortField>]);
```

It applies exactly the §3 comparator followed by the id tie-break. The
markdown `list_*` sorts with it (§5). A hand-written list that filters in
memory calls it before cutting its page.

**`order_{plural}_query`.** The SeaORM backend also emits

```rust
pub fn order_tasks_query(
    query: Select<task::Entity>,
    order: &[OrderBy<TaskSortField>],
) -> Select<task::Entity>;
```

It applies the same `ORDER BY` clauses as `list_*` (§4), so a hand-written
list that filters in SQL orders by the same rules. `list_*` itself calls
it. The markdown backend has no counterpart, since it has no query to
order.

Hand-written code is backend-specific by nature, so it picks the helper for
its backend. The parity guarantee covers a hand-written list only when it
orders through one of these two helpers.

**A hand-written `list` replaces the generated one.** A hand-written `list`
in `api_dir/{module}.rs` replaces the generated `list`, and the generated
`count`, for that module. Today the merge does the opposite: when a scanned
function has the same name as a generated one, the generated one is kept
and the scanned one is dropped (`merge_scanned_module`,
`src/api/mod.rs:165-170`). E0004 phase 2 reverses that precedence for
`list` and `count`, and `gen_api` stops emitting either for such a module.
Without this, a filtered list could not be the module's list, and the
generated unfiltered `count` would disagree with it.

### 2. Sortable fields

An entity's sortable fields are its id field plus every field with role
`Plain` or `EnumField` whose type is one of:

- `String`, `I32`, `I64`, `F32`, `F64` or `Bool`, or the `Option` form of
  any of these;
- `OptionEnum(T)` or `Other(T)`, where `T` resolves to a schema `EnumDef`.

These are not sortable: the `#[ontology(body)]` field, every relation field
(a `belongs_to` foreign key included), `VecString`, `VecStruct`,
`OptionEnum(T)` and `Other(T)` for any `T` that is not a schema enum, and
`Skip` fields.

- **The id field.** The entity's `#[ontology(id)]` field always has sort
  name `id` and variant `Id`, whatever the Rust field is called. `id` is
  the JSON:API resource id, and §3's tie-break names it.
- **Names.** Every other variant is the PascalCase field name. `name()`
  returns the serialized field name, which is the JSON:API attribute name.
- **Body.** The body is excluded because sorting by a markdown body is
  never meaningful and would compare kilobytes per pair.
- **Foreign keys.** A foreign key is a relationship on the wire, so it is
  not an attribute there.
- **Integers misclassified as enums.** Because the schema parser
  (`src/schema/parse.rs:423`) classifies `Option<u32>` as `OptionEnum`,
  the enum rule would sort it by string, where `"10" < "9"`. E0004 phase
  3c fixes the classification: `Option<T>` and bare `T` for an integer
  type the parser does not yet recognise (`u32`, `i16` and the like) map
  to the integer field types and sort numerically. `Option<SomeStruct>`
  and `Other(SomeStruct)` stay unsortable.

There is no opt-in or opt-out attribute. Every listed field costs the same
to sort as to filter (an unindexed scan on SeaORM, an in-memory compare on
markdown). A per-field switch would be configuration with nothing to
decide ([ADR 0003](0003-api-design-over-backwards-compatibility.md)).

### 3. Comparison semantics

This section is the ordering rule. Both backends MUST produce the same
sequence of ids for the same records and the same `order`. Each type
compares as follows:

| Type | Order |
|---|---|
| `String` | by UTF-8 bytes, which is Unicode code point order. `"B" < "a"`, `"z" < "é"` |
| integers | numeric |
| floats | numeric. `-0.0` equals `0.0`, as in SQLite, so the tie-break decides between them. A NaN never reaches storage (below) |
| `Bool` | `false < true` |
| enums | by the stored string, not by declaration order |
| null (`None`) | smaller than every value: first when ascending, last when descending |

- An empty string is a value, not null.
- **Tie-break.** After the requested keys, `id` ascending is appended unless
  `id` is already a key. The order is therefore total and pages are stable.
- **Duplicates.** A field repeated in `order` takes its first occurrence.
  Later ones are ignored. The store is lenient here; `parse_sort` rejects
  duplicates at every transport before they reach it.
- **Default.** An empty `order` means `id` ascending, on both backends, for
  every list, paged or not. The has_many child-id list of a record is in id
  ascending order. many_to_many linkage is in the order it was written.
- **NaN.** Both stores reject a NaN float on create and update, after the
  `before_*` hook and before touching storage. Every transport's input is
  JSON, which cannot encode NaN, so a NaN can only come from server-side
  Rust: a hook or a direct store caller. That is a server bug, so it
  surfaces as the backend's existing error variant (`DbError` on SeaORM,
  `Md` on markdown), which maps to 500, not as a new typed 400 variant.
  Without the check, a non-`Option` float NaN reads back from SQLite as
  `NULL` and fails to decode, so no ordering rule could make the two
  backends agree.

Why nulls first: it is SQLite's native order, so the generated SQL and the
markdown comparator both state the simplest rule. Nulls-last would serve
just as well. Picking one is what matters.

Why enums sort by string: SeaORM stores enums as text, and declaration
order would need a `CASE` expression generated per enum. Sorting by the
string is what a reader of the stored data sees.

Dates are strings, so they sort chronologically only when written in
ISO 8601, as every example writes them. This ADR adds no date type.

### 4. SeaORM

`order_{plural}_query` (§1.1) emits, for each key,
`query = query.order_by_with_nulls(Column::X, Order::Asc | Order::Desc, NullOrdering::First | NullOrdering::Last)`.
It then emits `order_by_asc(Column::{Id})` unless the id is a key, which
with an empty `order` is the `ORDER BY` #178 already emits. `list_*`
calls it before `limit` and `offset`.

- **Explicit null ordering.** It is emitted explicitly even where it
  matches SQLite's default, so the rule is in the generated code rather
  than implied by the engine.
- **Collation.** The SeaORM backend is SQLite-pinned already: its raw SQL
  helpers use `DatabaseBackend::Sqlite`. Generated columns declare no
  `COLLATE`, so SQLite's `BINARY` collation applies, and that is bytewise.
  A consumer who declares `NOCASE` on a column, or runs another engine, is
  outside the parity guarantee. The generated store does not try to detect
  that.
- **NaN.** The NaN check (§3) is generated into `create_*` and `update_*`
  and returns `AppError::DbError` before the statement runs.
- **has_many.** Child ids load `ORDER BY id`, as they do since #178.
- **many_to_many.** Linkage keeps its stored order. The consumer-owned
  `load_junction_ids` helper must return ids in insertion order (SQLite:
  `ORDER BY rowid`). `sync_junction` already inserts in list order, so the
  read order equals the order of the list written, as on markdown. The
  examples' copies of the helper gain the `ORDER BY`.
- **Indexes.** The generated store emits none. Sorting an unindexed column
  is a scan plus a sort. Consumers add indexes in their migrations.

### 5. Markdown

`list_*` walks and parses every record, as today, and builds the entities.
It then calls `sort_{plural}` (§1.1), which sorts with a stable `sort_by`
and a comparator generated from the §3 rules followed by the id
tie-break. Then it does skip/take, and populates relations for the page
only.

- **Comparator.** The generator emits one comparison per key, by type:
  - Strings, integers and `Bool` compare with `Ord::cmp`.
  - `Option` orders `None` first, which Rust's `Option: Ord` already does.
  - Floats map `-0.0` to `0.0`, then use `total_cmp`. A NaN entered by
    hand-editing a vault file is outside the parity guarantee;
    `total_cmp` still gives it a fixed place, so the order stays total.
  - Enums compare the stored string of each value, taken from
    `EnumDef.variants[].value`, never the enum's own `Ord`. A derived
    `Ord` would be declaration order, and the enum need not implement it.

  Descending reverses the key's result, not the whole sort.
- **Cost.** The sort adds O(N log N) to the O(N) parse every list already
  pays. N is bounded by `list_cap` (default 10 000).
- **`count`.** It still walks without parsing.
- **NaN.** The NaN check (§3) runs in `create_*` and `update_*` before the
  file is written and returns `AppError::Md`.
- **Default order.** The walk's path order does not define list order. The
  default is id order. That equals path order for a flat directory, and
  for a nested layout it makes ADR 0001's "lexicographic by record id"
  true.
- **has_many.** The derived child-id list is sorted by id.
- **many_to_many.** Linkage keeps the frontmatter list order.

### 6. Phasing

This section is the phasing; §3 is the rule each phase implements. #178
(merged) already orders SeaORM lists and has_many loads by id. This ADR
keeps that default and makes it a guarantee: on every list, on both
backends, and equal across them. #178's two reasons for declining parity
are answered by §3 (bytewise collation) and §5 (sort by id, not path).

The work lands in two steps, so that no E0004 phase paginates a list whose
order differs by backend.

**E0004 phase 1a** ships the §3 default order alone, with the `list_*`
signature unchanged:

- markdown lists in id order instead of walk path order;
- many_to_many linkage in written order on both backends: the SeaORM
  junction read in insertion (`rowid`) order, markdown in frontmatter
  order;
- the runtime parity fixture's default-order cases (§8).

**E0004 phase 2** makes a hand-written `list` replace the generated `list`
and `count` (§1.1).

**E0004 phase 3c** adds the `order` argument, `{Entity}SortField`,
`parse_sort`, `sort_{plural}`, `order_{plural}_query`, the comparison
rules for every sortable type, the NaN check, the integer classification
fix (§2), sort on all three transports and the TS `Transport` options
argument (§1), and the rest of the parity fixture.

### 7. Amendment to ADR 0001

ADR 0001 carries amendment 9, which points here. It supersedes contract
item 3 and amendment 4: `list()` returns records in the requested order,
then by id ascending; with no requested order, by id ascending. The order
is identical on both backends.

Item 5 still holds:

- `{Entity}SortField`, `sort_{plural}` and the new `list_*` signature are
  emitted identically for both backends. `order_{plural}_query` is
  SeaORM-only and sits below the store boundary, where the backends
  already differ.
- The API, servers and clients output above the store stays byte-identical.

### 8. Parity requirements

**`tests/backend_parity.rs`** keeps its byte-identical checks and adds
these:

- `StoreMethodMeta` records `count` and the `order` parameter of `list`,
  and the metadata compares equal across backends.
- The emitted `{Entity}SortField` and `sort_{plural}` source compares
  byte-identical across backends.
- The paginated path (`paginated` non-empty) is exercised. Today the
  fixture leaves it empty.

**A runtime parity test** is required, because none exists today. A
workspace fixture crate generates one schema twice, once against SQLite
(in memory) and once against a temporary vault. It loads identical
records into both and asserts identical id sequences for each of these:

- the default order;
- every sortable field, ascending and descending;
- a multi-key order with mixed directions;
- `Option` fields holding nulls, and empty strings beside nulls;
- strings that differ only in case, and non-ASCII strings (`"B"`, `"a"`,
  `"é"`, `"z"`), which pin byte order. This is the case #178 says breaks;
- a float `-0.0` beside `0.0`;
- an `Option<u32>` field, which pins numeric order (`9 < 10`);
- enum fields;
- equal keys, which exercise the tie-break;
- every page of a `limit`/`offset` walk across a run of ties;
- has_many and many_to_many linkage order;
- `sort_{plural}` applied to the SeaORM backend's unordered records, which
  must equal the order `list_*` returns.

It also asserts that a NaN write is rejected on both backends, on create
and on update, and that nothing is stored.

The fixture schema has an entity with every sortable type, a self-referential
`belongs_to`/`has_many` pair and a many_to_many.

No case may be skipped or `#[ignore]`d. A divergence is a bug in a backend,
not in the test.

## Consequences

**Positive:**

- Sort is available on HTTP, Tauri IPC and MCP alike, through one parser,
  and its results do not depend on the backend. The byte-identical layers above the store could not hide a
  difference.
- Pages are stable on both backends and equal across them, extending what
  #178 gives SeaORM alone.
- A hand-written filtered list orders by the same rules as the generated
  one when it uses `sort_{plural}` or `order_{plural}_query`.
- ADR 0001's contract item 3 describes what ships, and the first
  cross-backend runtime test exists.
- Invalid sort fields are compile errors below the transport boundary,
  and every transport reports them the same way (`SortError`).

**Negative:**

- **Breaking.** `list_*` gains an argument. Every hand-written caller of a
  store `list_*` or an API `list` adds `&[]`. ADR 0003 accepts the break;
  the changelog carries the one-line migration.
- **Breaking.** A hand-written `list` that today is silently shadowed by
  the generated one becomes the module's list, and its module loses the
  generated `count`.
- **Breaking.** A NaN float that a hook or direct caller writes is an
  error where it was stored before.
- Tauri IPC list commands and MCP list tools gain one optional `sort`
  field, and TS `Transport` list methods a trailing optional argument.
  Existing calls still compile and payloads stay flat.
- The markdown list always sorts. That is a small cost next to the parse
  it already does.
- many_to_many order on SeaORM relies on SQLite `rowid`, the same
  SQLite-only assumption the raw-SQL helpers already make.
- Enum fields sort alphabetically by stored string, which can surprise a
  reader who expects declaration order.
- No indexes are emitted, so a large SeaORM table sorts by scan.
- A hand-written list that orders by other means is outside the parity
  guarantee.

**Follow-on work:**

- E0004 phases 1a, 2 and 3c implement this ADR (§6); 3c wires sort on
  every transport.
- Index emission, if a consumer needs it, is its own decision.
- Sorting by a relationship attribute (`sort=epic.title`) is out of scope.

## Alternatives considered

### A. Field names as strings at the store

`list_tasks(&[("title", Direction::Asc)])`. Each backend would validate
names at runtime and need its own error variant. An invalid field would be
a runtime error everywhere instead of a compile error below the transport
boundary. Rejected.

### B. Sort above the store

Sort the `Vec` in the API or HTTP layer. That cannot work with pagination:
the sort must precede the page, so SeaORM would have to load every row.
Rejected.

### C. Order on SeaORM only

Markdown would ignore `order`, or approximate it. That breaks ADR 0001
item 5's intent: the same API call would return different results by
backend. Rejected.

### D. Per-backend determinism only (#178's position)

Each backend is deterministic but they may differ. Acceptable for a page
cursor, not for a `sort` the client asked for. Rejected as the contract.
#178's SeaORM `ORDER BY id` is the SeaORM half of the §3 default.

### E. An opt-in `#[ontology(sortable)]` attribute

Every scalar field costs the same to sort, so the attribute would only
gate a capability that has no downside to grant. Rejected (ADR 0003).

### F. Nulls last, or enums by declaration order

Each is workable. Nulls last needs explicit `NULLS LAST` on SQLite and an
inverted `Option` comparator. Declaration order needs a generated `CASE`
per enum. Rejected for the simpler rule; see §3.

### G. Treat NaN as null

SQLite stores a NaN as `NULL`, so the comparator could map NaN to `None`.
That makes the order agree but not the data: a non-`Option` float field
reads back from SQLite as `NULL` and fails to decode, while markdown
returns the NaN. Since no transport can send a NaN, rejecting it on write
costs no client anything. Rejected.

## Notes

- Sources: ADR 0001 contract item 3 and amendment 4; E0004 decision 6;
  E0004 decision 8 (sort on every transport); the
  [wire contract](../jsonapi-wire-contract.md) §7.2 and §7.4;
  [#178](https://github.com/sksizer/rust-ontogen/pull/178);
  [#172](https://github.com/sksizer/rust-ontogen/pull/172).
- Implemented by E0004 phases 1a (default order), 2 (hand-written `list`
  precedence) and 3c (the `order` argument), per §6. Until 3c, `list_*`
  keeps its current signature.
