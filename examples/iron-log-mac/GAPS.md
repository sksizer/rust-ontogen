# Where ontogen falls short of a real training log

The schema here is 17 entities taken from `determined-fitness` (about 50 entities,
already an ontogen consumer), picked for the shapes they stress. It runs on SQLite and
on markdown from one schema and one domain module, with one Swift app over either.

Each gap's evidence is one of:

- **tested**: pinned by [`shared/tests/scenario.rs`](shared/tests/scenario.rs) or a
  backend's `tests/scenario.rs`
- **hit**: stopped this build
- **read**: seen in ontogen's source or in a Fitness workaround, not reproduced here

## What held up

All of this passes unchanged on both backends:

- A self-referential tree, readable and writable from both ends
- `many_to_many`, including clearing it
- `Vec<Cue>` and `#[ontology(body)]`
- `i64` beyond `i32`, and exact `f64`
- Optional enums and optional relations, including clearing through Swift's `T??`
- Sort and paging

`src/native/generated.rs` comes out byte-identical for the two backends, so the Swift
API doesn't change with storage. The joins and rollups are written once, against the
generated store.

## Gaps, worst first

### 1. Lists can't filter, so every join loads whole tables

A generated `list_*` takes an order, a limit and an offset, with no filter by relation
or anything else. `session_detail` wants 20 sets and has to load all 20,000 (timings
below). The cost is small on SQLite and decides whether the app is usable on markdown.
Fitness hits the same wall and writes raw SQL in `domain/sql.rs`, which then has no
markdown form. *Tested (timings), read (Fitness).*

### 2. Nothing can express an aggregate or a join portably

`volume_by_muscle` is about 40 lines of hand-written hash joins, and takes 12x longer
than the SQL it stands in for on the same database. A backend-neutral query could
compile to SQL on one side and to a scan on the other. *Tested.*

### 3. The same delete does different things per backend

| Call | SQLite | Markdown |
|---|---|---|
| Create a row with a dangling reference | `db_error` | succeeds |
| Delete a referenced exercise, parent muscle, or tag in use | `db_error` | succeeds; references dangle |
| Delete a session with tags | `db_error` | succeeds |
| Later read through a dangling tag | can't happen | `session_detail` fails with `tag_not_found` |

There's no `on_delete` to declare, and the error a client sees is a raw SQLite message
under a generic code, not "still referenced". The third row is the most surprising: on
SQLite, a generated delete can't delete a record that has rows in its *own* junction
table, so the caller has to clear them first. *Tested.*

### 4. An implicit junction name can collide with an entity table, silently

`Muscle.groups → MuscleGroup` derives the junction name `muscle_groups`, which is also
`MuscleGroup`'s table. The build passes, then every `create_muscle` fails at runtime
with `no such column: muscle_id`. The `junction = "..."` override avoids it, but only
once you know. A build-time check would catch this. *Hit.*

### 5. A bad stored value panics or vanishes

- An enum column holding an unknown value **panics the read** on SQLite
  (`enum should have a fallback variant`). In this app that's inside an FFI call.
- A corrupt JSON column (`Vec<Cue>`) reads as an **empty list with no error**, and the
  next save writes that empty list back.
- On markdown, the same bad enum gives a proper error, but one bad file also fails
  every `list_*` over that directory.

*Tested.*

### 6. The schema can't state its rules

Each of these is accepted on both backends:

- A second `ExerciseMuscle` with the same (exercise, muscle) pair: no composite unique
- A `contribution` of 7.0 on a 0-to-1 field: no range check
- A `Note` about an exercise that doesn't exist: polymorphic pointers are inexpressible
- A `RepsFacet` with no effort: shared primary keys are undeclarable
- A cycle in the muscle tree

Fitness keeps these in hand-written `INDEXES`, `SUBJECT_KIND_CHECK` and `create_facet`
migration code, and they exist only on SQLite. *Tested (acceptance), read (Fitness).*

### 7. No multi-entity transaction

`log_set` writes three records and undoes earlier ones by hand when a later one fails.
A crash between writes still leaves partial data. Each write also emits its own change
event, so the app reloads three times per set. *Tested (the undo path); the crash case
follows from the code and wasn't run.*

### 8. No table creation or migrations

ontogen emits entities but nothing that creates them.
[`create_tables`](sqlite/src/persistence/db/mod.rs) is a hand-kept list of 19, and
forgetting one compiles and then fails at first query. It also emits no indexes: the
rollup above ran on none. Fitness's `create_all!` macro and `INDEXES` list are the same
workaround at larger scale. *Hit, read (Fitness).*

### 9. Boilerplate every consumer repeats

Each is hand-written and could be generated:

- 51 of `AppError`'s variants, three per entity, which the generated store requires
- `sync_junction`, `load_junction_ids`, `emit_change` and `EntityChange`
- `enum_to_string` and `decode_json_vec`, whose required signature is only discoverable
  from compile errors

*Hit.*

### 10. Smaller edges

- **Cross-entity `has_many` is refused.** `Person.sessions` is a build error (backlog
  B-XEHM), so "this person's sessions" falls through to gap 1. *Read
  (`src/store/has_many.rs`).*
- **`macro_rules!` enums are invisible.** The AST scan can't see them, so this schema
  writes ten enums out in full. *Hit.*
- **Foreign keys can't be sort keys.** `EffortSortField` has no `session_id` or
  `person_id`. *Hit.*
- **The generated `*_list` API method has no paging,** although the store's `list_*`
  does. From Swift, `effortList` always loads everything. *Hit.*
- **Markdown's default `list_cap` is 10,000.** Past that a list fails outright, so this
  example raises it to 100,000. A year or two of sets reaches the default. *Read.*
- **Plurals are mechanical:** `list_persons` on a table called `people`, and
  `list_equipments`. *Hit.*
- **Generated markdown code trips clippy on `Copy` enums:** 11 `clone_on_copy` warnings
  in this schema, so a consumer with `-D warnings` has to allow them. *Hit.*
- **Sharing a schema across backends has no first-class form.** This example uses
  symlinks plus a manual `rerun-if-changed`. *Hit.*
- **Markdown change events miss edits on disk.** A hand edit shows on the next read but
  fires no event, so an open app stays stale until something else changes. *Tested.*

Not exercised here, though Fitness builds its own: full-text search, seed data beyond
this catalog, and the docs stage.

## Timings

20,000 logged sets (20,000 efforts, each with lifting and reps facets), measured once on
one Mac in release builds. Treat them as orders of magnitude.

| Operation | SQLite | Markdown |
|---|---|---|
| `log_set`, per set (three records) | 5 ms | 48 ms |
| `get_effort` | 65 µs | 1 ms |
| `count_efforts` | 3 ms | 137 ms |
| `list_efforts`, newest 50 | 21 ms | 9.7 s |
| `session_detail`, 20 sets | 159 ms | 34.6 s |
| `volume_by_muscle` | 320 ms | 26.5 s |
| `best_sets` | 305 ms | 26.9 s |
| `volume_by_muscle` as raw SQL | 26 ms | no equivalent |

Markdown is fine for single records and a small log, but reads that touch a directory
parse every file in it, so its limit is the number of records and not the API. Reproduce
with:

```sh
cargo test --release -p iron-log-sqlite --test scenario -- --ignored --nocapture perf
cargo test --release -p iron-log-markdown --test scenario -- --ignored --nocapture perf
```

## Where to start

1. **Filtered lists** (gap 1), at least equality on a `belongs_to`, pushed down to SQL
   and a scan on markdown. This removes the most hand-written code and raises the
   practical ceiling on both backends.
2. **A build-time check for table-name collisions** (gap 4). It's small and turns a
   confusing runtime failure into a clear build error.
3. **Stop panicking on stored data** (gap 5). Report it as an error, and the same for
   corrupt JSON. This is also small.
4. **Declared delete behaviour and junction cleanup** (gap 3), so the backends agree on
   what a delete means.
