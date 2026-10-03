# ADR 0006 — Ordering on both store backends

## Status

**proposed** (2026-10-03).

This ADR revisits [ADR 0001](0001-markdown-as-store-backend.md) contract
item 3 and amendment 4, which left `OrderBy` out of the store until "a
future ADR that changes both backends together". It is that ADR. Number
0005 is reserved for the OKF vault decision of epic
[E0005](../planning/epics/okf-markdown-vault.md).

## Context

**Why ordering is needed now.** Two consumers of the store need ordering it
does not provide:

- **JSON:API `sort`.** Epic [E0004](../planning/epics/jsonapi-http-transport.md)
  decision 6 puts `sort` in scope. The [wire contract](../jsonapi-wire-contract.md)
  §7.4 specifies it. A sort applies before a page is cut, so it cannot be
  done above the store.
- **Stable pagination.** A page is only well defined when the order under
  it is total. Today neither backend promises that.

**What the store does today.**

- **Signatures.** Each entity gets
  `list_{plural}(&self, limit: Option<u64>, offset: Option<u64>)` and
  `count_{plural}(&self)`. There is no filter and no order at the store.
  Filters live above it, in user-authored `*Query` structs. PR
  [#172](https://github.com/sksizer/rust-ontogen/pull/172) requires a
  paginated `count` to take the same filter as `list`.
- **SeaORM.** `Entity::find()` has no `ORDER BY`, then applies SQL
  `LIMIT`/`OFFSET`. Row order is whatever SQLite returns.
- **The has_many child load** in `populate_*_relations` is unordered too.
- **#178.** PR [#178](https://github.com/sksizer/rust-ontogen/pull/178)
  (open) adds `order_by_asc(id)` to the list and the has_many load. Its
  description scopes it to per-backend determinism and declines
  cross-backend parity, for two reasons. SQL text ordering follows column
  collation while the vault sorts bytewise. The vault orders by path, not
  id.
- **Markdown.** `markdown_store::walk` sorts paths by their
  extension-stripped form (`walk.rs`). The store then parses every record
  and pages in memory with skip/take. It populates relations for the page
  only. For a flat `PerEntityDir` directory, path order equals id order.
  For a nested layout it does not, so ADR 0001's "lexicographic by record
  id" is not quite what ships.
- **Nulls.** `Option` fields are SQL `NULL` on SeaORM. On markdown they are
  an absent frontmatter key, read back as `None`.
- **Field types** (`ontogen_core::model::FieldType`): `String`, `I32`,
  `I64`, `F32`, `F64`, `Bool`, their `Option` forms, `OptionEnum`,
  `VecString`, `VecStruct`, and `Other(_)`, which covers enums and other
  types. There is no date type: dates are `String`, as in
  tasks-tracker's `created`.
- **Tests.** `tests/backend_parity.rs` compares generated text across
  backends. Nothing runs both backends against the same data.

**Why ADR 0001 deferred it.** Item 5 makes `gen_api` / `gen_servers` /
`gen_clients` output byte-identical across backends. An ordering argument
changes the generated `list_*` signature that the API layer calls, so it
must change on both backends at once, with the same types. That is the only
reason ADR 0001 gave. It is satisfied by doing exactly that.

## Decision

### 1. Store API

A runtime type in `ontogen-core`, which generated code already depends on
(`ontogen_core::events`):

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
- The HTTP list handler fills `order` from `sort` (wire contract §7.4),
  using `SortField::from_name`.
- Tauri IPC and MCP list handlers pass `&[]` and expose no new argument.
  Their payloads are unchanged.
- A user-authored list opts into `sort` by declaring the same parameter.

Why a borrowed slice: an empty slice is the default order, with no
`Option` to unwrap and no allocation, and a list of keys is what both SQL
and an in-memory comparator consume.

Why a generated enum rather than field-name strings: an invalid field is
unrepresentable below the HTTP boundary. Neither backend validates names,
and a field rename is a compile error in every caller.

### 2. Sortable fields

An entity's sortable fields are its id field plus every field with role
`Plain` or `EnumField` whose type is one of:

- `String`, `I32`, `I64`, `F32`, `F64` or `Bool`, or the `Option` form of
  any of these;
- `OptionEnum`, or `Other(T)` where `T` resolves to a schema enum.

These are not sortable: the `#[ontology(body)]` field, every relation field
(a `belongs_to` foreign key included), `VecString`, `VecStruct`, `Other(T)`
for a non-enum `T`, and `Skip` fields.

- **Names.** Variant names are the PascalCase field names. `name()` returns
  the serialized field name, which is the JSON:API attribute name.
- **Body.** The body is excluded because sorting by a markdown body is
  never meaningful and would compare kilobytes per pair.
- **Foreign keys.** A foreign key is a relationship on the wire, so it is
  not an attribute there.

There is no opt-in or opt-out attribute. Every listed field costs the same
to sort as to filter (an unindexed scan on SeaORM, an in-memory compare on
markdown). A per-field switch would be configuration with nothing to
decide ([ADR 0003](0003-api-design-over-backwards-compatibility.md)).

### 3. Comparison semantics

Both backends MUST produce the same sequence of ids for the same records
and the same `order`. Each type compares as follows:

| Type | Order |
|---|---|
| `String` | by UTF-8 bytes, which is Unicode code point order. `"B" < "a"`, `"z" < "é"` |
| integers | numeric |
| floats | numeric. A NaN is treated as null, because SQLite stores NaN as `NULL`. `-0.0` equals `0.0`, as in SQLite, so the tie-break decides between them |
| `Bool` | `false < true` |
| enums | by the stored string, not by declaration order |
| null (`None`) | smaller than every value: first when ascending, last when descending |

- An empty string is a value, not null.
- **Tie-break.** After the requested keys, `id` ascending is appended unless
  `id` is already a key. The order is therefore total and pages are stable.
- **Duplicates.** A field repeated in `order` takes its first occurrence.
  Later ones are ignored. The store is lenient here; the HTTP boundary
  rejects duplicates earlier (wire contract §7.4).
- **Default.** An empty `order` means `id` ascending, on both backends.

Why nulls first: it is SQLite's native order, so the generated SQL and the
markdown comparator both state the simplest rule. Nulls-last would serve
just as well. Picking one is what matters.

Why enums sort by string: SeaORM stores enums as text, and declaration
order would need a `CASE` expression generated per enum. Sorting by the
string is what a reader of the stored data sees.

Dates are strings, so they sort chronologically only when written in
ISO 8601, as every example writes them. This ADR adds no date type.

### 4. SeaORM

For each key, the backend emits
`query = query.order_by_with_nulls(Column::X, Order::Asc | Order::Desc, NullOrdering::First | NullOrdering::Last)`.
It then emits `order_by_asc(Column::{Id})` unless the id is a key. All of
this comes before `limit` and `offset`.

- **Explicit null ordering.** It is emitted explicitly even where it
  matches SQLite's default, so the rule is in the generated code rather
  than implied by the engine.
- **Collation.** The SeaORM backend is SQLite-pinned already: its raw SQL
  helpers use `DatabaseBackend::Sqlite`. Generated columns declare no
  `COLLATE`, so SQLite's `BINARY` collation applies, and that is bytewise.
  A consumer who declares `NOCASE` on a column, or runs another engine, is
  outside the parity guarantee. The generated store does not try to detect
  that.
- **has_many.** Child ids load `ORDER BY id`, as #178 does.
- **many_to_many.** Linkage keeps its stored order. The consumer-owned
  `load_junction_ids` helper must return ids in insertion order (SQLite:
  `ORDER BY rowid`). `sync_junction` already inserts in list order, so the
  read order equals the order of the list written, as on markdown. The
  examples' copies of the helper gain the `ORDER BY`.
- **Indexes.** The generated store emits none. Sorting an unindexed column
  is a scan plus a sort. Consumers add indexes in their migrations.

### 5. Markdown

`list_*` walks and parses every record, as today, and builds the entities.
It then sorts them with a stable `sort_by`, using a comparator generated
from the keys and the §3 rules followed by the id tie-break. Then it does
skip/take, and populates relations for the page only.

- **Comparator.** The generator emits one comparison per key, by type:
  - Strings, integers and `Bool` compare with `Ord::cmp`.
  - `Option` orders `None` first, which Rust's `Option: Ord` already does.
  - Floats map NaN to `None` and `-0.0` to `0.0`, then use `total_cmp`.
  - Enums compare the stored string of each value, taken from
    `EnumDef.variants[].value`, never the enum's own `Ord`. A derived
    `Ord` would be declaration order, and the enum need not implement it.

  Descending reverses the key's result, not the whole sort.
- **Cost.** The sort adds O(N log N) to the O(N) parse every list already
  pays. N is bounded by `list_cap` (default 10 000).
- **`count`.** It still walks without parsing.
- **Default order.** The walk's path order no longer defines list order.
  The default is id order. That equals path order for a flat directory,
  and for a nested layout it makes ADR 0001's "lexicographic by record id"
  true.
- **has_many.** The derived child-id list is sorted by id.
- **many_to_many.** Linkage keeps the frontmatter list order.

### 6. Relation to #178, and phasing

#178 orders paginated SeaORM lists by id. This ADR keeps that default and
makes it a guarantee: on every list (paged or not), on both backends, and
equal across them. #178's two reasons for declining parity are answered by
§3 (bytewise collation) and §5 (sort by id, not path).

The work lands in two steps, so that no E0004 phase paginates an unordered
list.

**E0004 phase 1a** ships the default order alone:

- id ascending on every list on both backends, with the `list_*`
  signature unchanged;
- has_many child ids ordered by id;
- many_to_many linkage in stored order;
- the runtime parity fixture's default-order cases.

This subsumes #178, which can merge before it or be closed in its favour.

**E0004 phase 3c** adds the `order` argument, `{Entity}SortField`, the
comparison rules for every sortable type, and the rest of the parity
fixture.

### 7. Amendment to ADR 0001

Contract item 3 now reads: "`list()` returns records in the requested
order, then by id ascending; with no requested order, by id ascending. The
order is identical on both backends." Amendment 4 is superseded by this
ADR.

Item 5 still holds:

- `{Entity}SortField` and the new `list_*` signature are emitted
  identically for both backends.
- The API, servers and clients output above the store stays byte-identical.

### 8. Parity requirements

**`tests/backend_parity.rs`** keeps its byte-identical checks and adds
these:

- `StoreMethodMeta` records `count` and the `order` parameter of `list`,
  and the metadata compares equal across backends.
- The emitted `{Entity}SortField` source compares byte-identical across
  backends.
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
- floats including a NaN and a `-0.0`;
- enum fields;
- equal keys, which exercise the tie-break;
- every page of a `limit`/`offset` walk across a run of ties;
- has_many and many_to_many linkage order.

The fixture schema has an entity with every sortable type, a self-referential
`belongs_to`/`has_many` pair and a many_to_many.

No case may be skipped or `#[ignore]`d. A divergence is a bug in a backend,
not in the test.

## Consequences

**Positive:**

- JSON:API `sort` has a store to stand on, and its results do not depend on
  the backend. The byte-identical layers above the store could not hide a
  difference.
- Pages are stable on both backends. That fixes the unordered SeaORM page
  #178 targets, for every list rather than paged ones only.
- ADR 0001's contract item 3 describes what ships, and the first
  cross-backend runtime test exists.
- Invalid sort fields are compile errors below the HTTP boundary.

**Negative:**

- **Breaking.** `list_*` gains an argument. Every hand-written caller of a
  store `list_*` or an API `list` adds `&[]`. ADR 0003 accepts the break;
  the changelog carries the one-line migration.
- The markdown list now always sorts. That is a small cost next to the
  parse it already does.
- many_to_many order on SeaORM relies on SQLite `rowid`, the same
  SQLite-only assumption the raw-SQL helpers already make.
- Enum fields sort alphabetically by stored string, which can surprise a
  reader who expects declaration order.
- No indexes are emitted, so a large SeaORM table sorts by scan.

**Follow-on work:**

- E0004 phases 1a and 3c implement this ADR (§6); 3c wires `sort` on the HTTP list.
- A TS transport options parameter that exposes `sort` (wire contract
  §14.2) is a separate change.
- Index emission, if a consumer needs it, is its own decision.
- Sorting by a relationship attribute (`sort=epic.title`) is out of scope.

## Alternatives considered

### A. Field names as strings at the store

`list_tasks(&[("title", Direction::Asc)])`. Each backend would validate
names at runtime and need its own error variant. An invalid field would be
a runtime error everywhere instead of a compile error below HTTP. Rejected.

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
cursor, not for a `sort` the client asked for. Rejected as the contract;
kept as #178's interim fix.

### E. An opt-in `#[ontology(sortable)]` attribute

Every scalar field costs the same to sort, so the attribute would only
gate a capability that has no downside to grant. Rejected (ADR 0003).

### F. Nulls last, or enums by declaration order

Each is workable. Nulls last needs explicit `NULLS LAST` on SQLite and an
inverted `Option` comparator. Declaration order needs a generated `CASE`
per enum. Rejected for the simpler rule; see §3.

## Notes

- Sources: ADR 0001 contract item 3 and amendment 4; E0004 decision 6;
  the [wire contract](../jsonapi-wire-contract.md) §7.2 and §7.4;
  [#178](https://github.com/sksizer/rust-ontogen/pull/178);
  [#172](https://github.com/sksizer/rust-ontogen/pull/172).
- Implemented by E0004 phases 1a (default order) and 3c (the `order`
  argument), per §6. Until 3c, `list_*` keeps its current signature.
