---
type: backlog
schema_version: '2'
id: B-SQLT
tags:
- seaorm
- sqlite
- multi-engine
- store
last_reviewed: '2026-10-04'
---

# Support Postgres and MySQL on the SeaORM backend

The SeaORM backend is SQLite-only (maintainer decision D12, 2026-10-03; ADR 0006 §4, and "Database support" in the SeaORM guide). Each SQLite-specific location carries a `sqlite-only` comment that says why. This item lists every one, with what a multi-engine store would do instead. Supporting another engine means replacing all of them and running the runtime parity suite (`crates/parity`) against that engine as well as SQLite.

Regenerate the list with:

```sh
rg -n 'sqlite-only:' -g '!docs/**' -g '!**/generated/**' -g '!*.snap' .
```

That prints the 16 locations below. The generator emits four of its markers into the code it writes, so every generated SeaORM store module (`**/store/generated/*.rs`) and the store snapshots in `src/snapshots/` carry copies of them; dropping the globs shows those too.

## Generator, emitted into the generated store

1. **`src/store/backends/seaorm/gen_crud.rs`, `generate_order_query`: string sort keys.** `order_{plural}_query`, which `list_*` and hand-written SQL lists order through, sorts every string key (the id tie-break and any sortable `String` field) and promises byte order, the markdown backend's order (ADR 0006 §3). Generated columns declare no `COLLATE`, so this holds only under SQLite's default `BINARY` collation; a Postgres locale collation or MySQL's case-insensitive default orders `alpha` before `Zeta`. Its explicit `NULLS FIRST`/`NULLS LAST` is portable: sea-query emulates it on MySQL. Replacement: declare a bytewise collation on id and sortable string columns per engine (`COLLATE "C"` on Postgres, `utf8mb4_bin` on MySQL), or order by an explicit `COLLATE` expression in the query.
2. **`src/store/backends/seaorm/gen_crud.rs`, `generate_list`: `LIMIT` for an offset alone.** SQLite rejects `OFFSET` without `LIMIT`, so an offset alone sends `LIMIT i64::MAX`. Replacement: per engine, omit the limit (Postgres accepts `OFFSET` alone) or use the engine's "all rows" limit (MySQL documents `18446744073709551615`; `i64::MAX` also works there). The `i64::MAX` clamp on both values above it is about sea-query binding, not SQLite, and stays.
3. **`src/store/backends/seaorm/gen_crud.rs`, `generate_populate_relations`: `has_many` child order.** The child-id load orders by id and promises byte order, the same collation assumption as item 1. Replacement: as item 1.
4. **`src/store/backends/seaorm/gen_crud.rs`, `generate_try_insert_helper`: the probe after a duplicate.** `create_*` runs in a transaction, and `try_insert_{entity}` answers a unique violation by looking the id up on that same transaction, so a derived id can probe on to `-2` and a client id becomes `{Entity}AlreadyExists`. A derived id has no other probe: each candidate is inserted, and a taken one fails its insert, so the transaction's first statement is a write (see "Checked and already portable"). A failed `INSERT` leaves an SQLite transaction usable; Postgres aborts the whole transaction on any failed statement, so the lookup (and every later statement, the next candidate's insert included) would fail. Replacement: run the insert inside a savepoint (`txn.begin()` on the `DatabaseTransaction` opens one) and roll back to it on the violation before the lookup.

## Generator, source only

5. **`src/persistence/seaorm/gen_entity.rs`, `field_db_type`: integer primitives in `i64` columns.** Phase 1a moved `u8`, `u16`, `u32`, `i8` and `i16` fields from `i32` to `i64` model fields (and `u64`, `usize`, `isize`, `u128`, `i128` gained `i64` columns). SQLite stores every integer in one `INTEGER` storage class, so an existing column reads as `i64` in place and only a wrapped `u32` needs the upgrade guide's `UPDATE ... + 4294967296`. Other engines need the column widened first (`ALTER COLUMN ... TYPE bigint` on Postgres, `MODIFY ... BIGINT` on MySQL); the upgrade guide gives that SQL but nothing generates or tests it. Replacement: ship per-engine migration SQL for the widening (or a generated sea-orm-migration step) and test it against each engine.

## In-tree consumer helpers

6. **`crates/parity/seaorm/src/store/mod.rs`, `Store::open_in_memory`.** The parity harness connects to `sqlite::memory:` with one connection, through `Store::open_url`, which takes any URL. Replacement: take the connection URL from the environment (for example a `PARITY_DATABASE_URL`) and run the same scenarios per engine in CI.
7. **`crates/parity/seaorm/src/store/mod.rs`, `Store::sync_junction`.** Inserts junction rows in list order and relies on SQLite's `rowid` following insertion order for `many_to_many` written order (ADR 0006 §4). Replacement: give junction tables a `position` column, write it from the list index, and drop the reliance on insertion order.
8. **`crates/parity/seaorm/src/store/mod.rs`, `Store::load_junction_ids`.** Reads junction targets `ORDER BY rowid`, a column only SQLite has (and only on tables not declared `WITHOUT ROWID`). Replacement: `ORDER BY position` over the column from item 7.
9. **`crates/parity/seaorm/src/store/mod.rs`, `sqlite()`.** Builds every raw junction statement with `DatabaseBackend::Sqlite` and `?` placeholders. Replacement: `self.db.get_database_backend()` with per-engine placeholders, or sea-query statements.
10. **`examples/iron-log/src-tauri/src/store/mod.rs`, `Store::sync_junction`.** Raw `DELETE`/`INSERT` built with `DatabaseBackend::Sqlite` and `?` placeholders, and list order kept only through `rowid` insertion order. Replacement: items 7 and 9.
11. **`examples/iron-log/src-tauri/src/store/mod.rs`, `Store::load_junction_ids`.** `ORDER BY rowid` under `DatabaseBackend::Sqlite`. Replacement: items 8 and 9.
12. **`crates/parity/check/tests/runtime_parity.rs`, `refuse_junction_inserts`.** The SeaORM-only rollback test makes junction inserts fail with an SQLite trigger (`RAISE(ABORT, ...)`). Replacement: an engine's own trigger syntax, or another way to fail the junction write after the row (a check constraint the test adds, for instance).
13. **`crates/parity/check/tests/runtime_parity.rs`, `Sqlite::delete_tag_leaving_links`.** To give an item a `many_to_many` id whose tag is gone, as a markdown delete leaves one, the harness switches SQLite's foreign keys off with `PRAGMA foreign_keys = OFF` and deletes the tag. Replacement: per engine, `SET session_replication_role = replica` on Postgres or `SET FOREIGN_KEY_CHECKS = 0` on MySQL, or create the junction table without its foreign key for that scenario.
14. **`crates/parity/check/tests/runtime_parity.rs`, `concurrent_derived_id_creates_all_succeed_on_a_pooled_database`.** The SeaORM-only concurrency test opens a file-backed SQLite database (`sqlite://<tempdir>/parity.db?mode=rwc`) with a pool of eight connections. Replacement: a fresh database per run on the engine under test, from the same environment URL as item 6.

## Docs

15. **`site/src/content/docs/guides/store-layer.mdx`, "Junction Sync": the `sync_junction` snippet.** The example consumers copy builds raw `DELETE`/`INSERT` statements with `DatabaseBackend::Sqlite` and `?` placeholders, and keeps list order through `rowid` insertion order. Replacement: as items 9 and 7 (or B-JSSQ, which generates the sync and removes the snippet).
16. **`site/src/content/docs/guides/store-layer.mdx`, "Junction Sync": the `load_junction_ids` snippet.** It uses `ORDER BY rowid`. Replacement: the snippet follows items 7 and 8 (and the `load_junction_ids` order note in the upgrade guide changes with it).

## Checked and already portable

These looked engine-specific but are not, and carry no marker:

- `try_insert_{entity}` detects a duplicate id with `DbErr::sql_err()` and `SqlErr::UniqueConstraintViolation`, which sea-orm maps for SQLite, Postgres and MySQL. What it does next is item 4.
- The transaction of `create_*` and `update_*` opens with `TransactionTrait::begin` and commits with `DatabaseTransaction::commit`, which every engine sea-orm supports provides.
- That transaction's first statement is a write, and every read the write needs runs before `begin()`. SQLite needs this: a deferred transaction that reads and then wants the write lock another connection holds fails at once with `SQLITE_BUSY`, busy timeout or not. On Postgres and MySQL, which lock rows, the order is harmless, so the generator keeps one order for every engine and carries no marker for it.
- The parity crates create tables with `Schema::new(db.get_database_backend()).create_table_from_entity(..)`.
- Enum fields are stored as their string value in a text column and `Vec` fields as JSON text. Both work on every engine; native enum or `jsonb` columns would be an optimisation, not a fix.
- The `i64::MAX` clamp of `limit` and `offset` in `list_*` (sea-query binds them as `i64` on every engine).
