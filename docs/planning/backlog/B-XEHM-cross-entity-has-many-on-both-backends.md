---
type: backlog
schema_version: '2'
id: B-XEHM
tags:
- store
- relations
- has-many
- parity
last_reviewed: '2026-10-03'
---

# Support cross-entity has_many on both store backends

A `has_many` whose `target` is another entity (`Workout.sets -> WorkoutSet.workout_id`) fails the build with a `CodegenError` (`has_many::validate_targets` in `src/store/has_many.rs`). Only the self-referential shape (`Node.contains -> Node.parent_id`) is generated. The workaround is to declare the `belongs_to` on the child and list the children in a hand-written API function.

Before the restriction both backends emitted the cross-entity shape against the declaring entity's own records:

- SeaORM: `set_{entity}_parent` ran `UPDATE {declaring table} SET {fk} = ?`, so a create or update rewrote the wrong table; `{entity}_exists` checked the wrong table for the listed children; the relation load filtered the declaring entity's column.
- Markdown: `set_{entity}_parent` read and rewrote the declaring entity's own records and the frontmatter field set had no `{fk}`, so the generated store did not compile.

To support it:

- Emit the child-side helpers against the target: `set_{child}_parent`, `{child}_exists`, and the relation load over the target's table or vault directory and its `{fk}` column or frontmatter field, on both backends.
- Read the foreign key's type (`String` or `Option<String>`, so `{Child}ParentRequired` or clear) from the target entity, not the declaring one. That needs the whole entity set in the store emission, which today generates one entity at a time.
- Validate at build time that the target exists and has a `belongs_to` field named `foreign_key` pointing back at the declaring entity.
- Add a runtime parity pair in `crates/parity` (a parent and a child entity, optional and required foreign keys) covering create, update with dropped children, `{Child}NotFound`, `{Child}ParentRequired` and child order, and run it on both backends.

This has to land, parity pair included, before E0004 phase 3a, which serves `has_many` relationship endpoints from the generated store (`docs/planning/epics/jsonapi-http-transport.md`). Until then phase 3a serves only the self-referential shape.
