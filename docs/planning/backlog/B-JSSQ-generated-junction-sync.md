---
type: backlog
schema_version: '2'
id: B-JSSQ
tags:
- seaorm
- store
- relations
- multi-engine
last_reviewed: '2026-10-05'
---

# Generate the SeaORM junction sync and drop the consumer's `sync_junction`

A SeaORM `many_to_many` write calls the consumer's `sync_junction(conn, table, source_col, target_col, source_id, target_ids)`, which every SeaORM consumer writes by hand (`crates/parity/seaorm/src/store/mod.rs`, `examples/iron-log/src-tauri/src/store/mod.rs`). Since the pre-0.9.0 fixes it takes the transaction the generated `create_*`/`update_*` run in, so the hook is now a pure function of generated data: the junction table and columns are generated (`src/persistence/seaorm/gen_entity.rs`), and `set_{entity}_parent` is already emitted with sea_query on the same connection. The consumer's copy is raw SQL marked `sqlite-only` (B-SQLT), and a hook that runs a statement on `self.db()` instead of the connection it is given escapes the transaction (and deadlocks a one-connection pool).

## Proposal

- Emit the sync into the generated store: `{junction}::Entity::delete_many().filter(source = id).exec(conn)`, then one insert per target in list order (`insert_many` keeps statement order on SQLite; see below), on the transaction.
- Keep `load_junction_ids` as the read side, or generate it too once order is portable.
- Order: many_to_many lists keep their written order (ADR 0006 §4), which today relies on SQLite's `rowid` (B-SQLT items for `sync_junction` and `load_junction_ids`). Generating the sync is the natural point to add a `position` column to junction tables and order by it, which also removes those two B-SQLT locations.
- Remove `sync_junction` from the consumer contract: a breaking change (the hook becomes dead code a consumer deletes, and junction tables gain a column if `position` is added). Upgrading guide entry and migration SQL for existing junction tables.

## What done looks like

- No consumer-written junction SQL; the parity harness and iron-log drop their `sync_junction`.
- Runtime parity unchanged, including many_to_many written order and the rollback test.
