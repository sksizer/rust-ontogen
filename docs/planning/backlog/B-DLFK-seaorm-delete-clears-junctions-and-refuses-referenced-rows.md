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

# A SeaORM delete clears junction rows and refuses a referenced row with a non-500 status

The generated SeaORM `delete_{entity}` (`generate_delete` in `src/store/backends/seaorm/gen_crud.rs`; for example `delete_workout` in `examples/iron-log/src-tauri/src/store/generated/workout.rs`) deletes the row and nothing else. It does not remove the entity's `many_to_many` junction rows, and it does not handle rows that reference it: the children of a `has_many` and the junction rows of a `many_to_many` it is the target of. The generated SeaORM entities (`src/persistence/seaorm/gen_entity.rs`) declare `belongs_to` relations with no `on_delete`.

SQLite enforces foreign keys under sqlx's default connection options, so on iron-log running headless each of these answers `500 db_error` with `FOREIGN KEY constraint failed`:

- deleting a workout that has tags (its own `workout_tags` rows);
- deleting a workout that still has sets (`workout_set.workout_id`);
- deleting a tag still attached to a workout (`workout_tags.tag_id`).

The markdown backend deletes the record file whatever links to it, and the links dangle, which it tolerates by design (ADR 0001 amendment 5). The backends therefore diverge on delete, and over HTTP the SeaORM failure is a `500` where the contract (§13) wants a mapped status wherever one can be derived.

## What done looks like

- A delete removes the deleted entity's own `many_to_many` junction rows in the same transaction as the row, so deleting a tagged record succeeds on SeaORM as it does on markdown.
- A row that is still referenced (a row whose `belongs_to` foreign key names it, `has_many` children included, or a junction row that targets it) is handled one consistent way on both backends: either the delete is refused with a mapped non-500 status (for example `409 {entity}_in_use`, a new typed store error that the `AppError` scan maps and the wire contract §13.4 lists), or the references are cleared according to the foreign key's optionality (an `Option` foreign key set to null, a required one refused). The choice between the two is open.
- A runtime parity case in `crates/parity` deletes a record with junction rows, and a still-referenced record, on both backends and asserts the same outcome.
- The wire contract (§8.4, §13.4) and the SeaORM guide describe the behaviour.

Found while running iron-log's HTTP server on SeaORM/SQLite during E0004 phase 4.
