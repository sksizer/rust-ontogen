---
type: backlog
schema_version: '2'
id: B-DLFK
tags:
- store
- seaorm
- relations
- delete
- parity
last_reviewed: '2026-10-04'
---

# A SeaORM delete clears its junction rows and refuses a still-referenced row with a typed error

The generated SeaORM `delete_{entity}` (`generate_delete` in `src/store/backends/seaorm/gen_crud.rs`; for example `delete_workout` in `examples/iron-log/src-tauri/src/store/generated/workout.rs`) deletes the row and nothing else. It does not remove the entity's `many_to_many` junction rows, and it does not handle rows that reference it: the children of a `has_many` and the junction rows of a `many_to_many` it is the target of. The generated SeaORM entities (`src/persistence/seaorm/gen_entity.rs`) declare `belongs_to` relations with no `on_delete`.

SQLite enforces foreign keys under sqlx's default connection options, so on iron-log running headless each of these answers `500 db_error` with `FOREIGN KEY constraint failed`:

- deleting a workout that has tags (its own `workout_tags` rows);
- deleting a workout that still has sets (`workout_set.workout_id`);
- deleting a tag still attached to a workout (`workout_tags.tag_id`).

The first is a SeaORM defect with no markdown counterpart: on markdown a record's `many_to_many` links live in its own frontmatter, so deleting the file removes them. The other two are a `500` where E0004's acceptance criterion asks for "a non-`500` status wherever E0003 phase 1 can derive one" ([epic](../epics/jsonapi-http-transport.md#acceptance-criteria)): the store knows why it refused, but reports it as `DbError`, which the `AppError` scan maps to `500` by design (wire contract §13.4).

## Scope: SeaORM only

This item is narrowed to the SeaORM store. The markdown backend deletes a record file whatever links to it and leaves those links dangling, deliberately: [ADR 0001](../../architecture/0001-markdown-as-store-backend.md) amendment 5 defers delete-time cascade link cleanup, and the wire contract (§7.5, §9.3) tolerates dangling linkage. A rule that made both backends handle a still-referenced record the same way (refuse it, or clear the references) would change markdown delete too, and so amend ADR 0001 amendment 5. That needs its own decision; this item does not take it. A SQLite row cannot hold a dangling foreign key, so on SeaORM the honest outcome is a typed refusal.

## What done looks like

- A SeaORM delete removes the deleted entity's own `many_to_many` junction rows in the same transaction as the row, so deleting a tagged record succeeds, as it does on markdown.
- A SeaORM delete of a row that is still referenced (a row whose `belongs_to` foreign key names it, `has_many` children included, or a junction row that targets it) is refused before anything is written with a new typed store error, for example `{Entity}InUse(id)`. The `AppError` scan maps it by suffix like the other typed errors (for example `409 {entity}_in_use`), and wire contract §13.4 lists it. Every example's `AppError` declares the variant the store generator constructs.
- A runtime parity case in `crates/parity` deletes a record that has junction rows and asserts the same outcome on both backends. A SeaORM store test covers the refusal; the backends differ there by design, as above.
- The wire contract (§8.4, §13.4) and the SeaORM guide describe the behaviour.

Found while running iron-log's HTTP server on SeaORM/SQLite during E0004 phase 4.
