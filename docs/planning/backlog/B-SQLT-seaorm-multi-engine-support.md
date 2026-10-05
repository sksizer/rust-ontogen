---
type: backlog
schema_version: '2'
id: B-SQLT
tags:
- seaorm
- sqlite
- multi-engine
- store
last_reviewed: '2026-10-03'
---

# Support Postgres and MySQL on the SeaORM backend

The SeaORM backend is SQLite-only (maintainer decision D12, 2026-10-03; ADR 0006 §4, and "Database support" in the SeaORM guide). Each SQLite-specific location carries a `sqlite-only` comment that says why. This item lists every one, with what a multi-engine store would do instead. Supporting another engine means replacing all of them and running the runtime parity suite (`crates/parity`) against that engine as well as SQLite.

Regenerate the list with:

```sh
rg -n 'sqlite-only:' -g '!docs/**' -g '!**/generated/**' -g '!*.snap' .
```

That prints the 11 locations below. The generator emits three of its markers into the code it writes, so every generated SeaORM store module (`**/store/generated/*.rs`) and the store snapshots in `src/snapshots/` carry copies of them; dropping the globs shows those too.

## Generator, emitted into the generated store

1. **`src/store/backends/seaorm/gen_crud.rs`, `generate_order_query`: string sort keys.** `order_{plural}_query`, which `list_*` and hand-written SQL lists order through, sorts every string key (the id tie-break and any sortable `String` field) and promises byte order, the markdown backend's order (ADR 0006 §3). Generated columns declare no `COLLATE`, so this holds only under SQLite's default `BINARY` collation; a Postgres locale collation or MySQL's case-insensitive default orders `alpha` before `Zeta`. Its explicit `NULLS FIRST`/`NULLS LAST` is portable: sea-query emulates it on MySQL. Replacement: declare a bytewise collation on id and sortable string columns per engine (`COLLATE "C"` on Postgres, `utf8mb4_bin` on MySQL), or order by an explicit `COLLATE` expression in the query.
2. **`src/store/backends/seaorm/gen_crud.rs`, `generate_list`: `LIMIT` for an offset alone.** SQLite rejects `OFFSET` without `LIMIT`, so an offset alone sends `LIMIT i64::MAX`. Replacement: per engine, omit the limit (Postgres accepts `OFFSET` alone) or use the engine's "all rows" limit (MySQL documents `18446744073709551615`; `i64::MAX` also works there). The `i64::MAX` clamp on both values above it is about sea-query binding, not SQLite, and stays.
3. **`src/store/backends/seaorm/gen_crud.rs`, `generate_populate_relations`: `has_many` child order.** The child-id load orders by id and promises byte order, the same collation assumption as item 1. Replacement: as item 1.

## Generator, source only

4. **`src/persistence/seaorm/gen_entity.rs`, `field_db_type`: integer primitives in `i64` columns.** Phase 1a moved `u8`, `u16`, `u32`, `i8` and `i16` fields from `i32` to `i64` model fields (and `u64`, `usize`, `isize`, `u128`, `i128` gained `i64` columns). SQLite stores every integer in one `INTEGER` storage class, so an existing column reads as `i64` in place and only a wrapped `u32` needs the upgrade guide's `UPDATE ... + 4294967296`. Other engines need the column widened first (`ALTER COLUMN ... TYPE bigint` on Postgres, `MODIFY ... BIGINT` on MySQL); the upgrade guide gives that SQL but nothing generates or tests it. Replacement: ship per-engine migration SQL for the widening (or a generated sea-orm-migration step) and test it against each engine.

## In-tree consumer helpers

5. **`crates/parity/seaorm/src/store/mod.rs`, `Store::open_in_memory`.** The parity harness connects to `sqlite::memory:` with one connection. Replacement: take the connection URL from the environment (for example a `PARITY_DATABASE_URL`) and run the same scenarios per engine in CI.
6. **`crates/parity/seaorm/src/store/mod.rs`, `Store::sync_junction`.** Inserts junction rows in list order and relies on SQLite's `rowid` following insertion order for `many_to_many` written order (ADR 0006 §4). Replacement: give junction tables a `position` column, write it from the list index, and drop the reliance on insertion order.
7. **`crates/parity/seaorm/src/store/mod.rs`, `Store::load_junction_ids`.** Reads junction targets `ORDER BY rowid`, a column only SQLite has (and only on tables not declared `WITHOUT ROWID`). Replacement: `ORDER BY position` over the column from item 6.
8. **`crates/parity/seaorm/src/store/mod.rs`, `sqlite()`.** Builds every raw junction statement with `DatabaseBackend::Sqlite` and `?` placeholders. Replacement: `self.db.get_database_backend()` with per-engine placeholders, or sea-query statements.
9. **`examples/iron-log/src-tauri/src/store/mod.rs`, `Store::sync_junction`.** Raw `DELETE`/`INSERT` built with `DatabaseBackend::Sqlite` and `?` placeholders, and list order kept only through `rowid` insertion order. Replacement: items 6 and 8.
10. **`examples/iron-log/src-tauri/src/store/mod.rs`, `Store::load_junction_ids`.** `ORDER BY rowid` under `DatabaseBackend::Sqlite`. Replacement: items 7 and 8.

## Docs

11. **`site/src/content/docs/guides/store-layer.mdx`, "Junction Sync".** The `load_junction_ids` snippet the guide tells consumers to copy uses `ORDER BY rowid`. Replacement: the snippet follows items 6 and 7 (and the `load_junction_ids` order note in the upgrade guide changes with it).

## Checked and already portable

These looked engine-specific but are not, and carry no marker:

- `try_insert_{entity}` detects a duplicate id with `DbErr::sql_err()` and `SqlErr::UniqueConstraintViolation`, which sea-orm maps for SQLite, Postgres and MySQL.
- The parity crates create tables with `Schema::new(db.get_database_backend()).create_table_from_entity(..)`.
- Enum fields are stored as their string value in a text column and `Vec` fields as JSON text. Both work on every engine; native enum or `jsonb` columns would be an optimisation, not a fix.
- The `i64::MAX` clamp of `limit` and `offset` in `list_*` (sea-query binds them as `i64` on every engine).
