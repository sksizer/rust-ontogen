---
type: backlog
schema_version: '2'
id: B-HMCY
tags:
- store
- relations
- has-many
- parity
last_reviewed: '2026-10-04'
---

# Refuse ancestor cycles in self-referential has_many writes

A self-referential `has_many` (`Task.subtasks -> Task.parent_id`) describes a tree. The store refuses the one-record cycle: a `has_many` list that names the record itself fails with `{Child}ParentCycle(id)` (`403 {child}_parent_cycle` over HTTP) before anything is written, on both backends (JSON:API wire contract §5.4, `src/store/linked_ids.rs`). Longer cycles are accepted. With `b` a child of `a`, an update that lists `a` in `b`'s subtasks, or sets `a.parent_id` to `b`, succeeds, and `a` and `b` are each other's ancestors. A walk up the parents from either never reaches a root.

## Why it is not refused today

Refusing a cycle needs an ancestor walk on every write that can close one: a `has_many` list (each listed child must not be an ancestor of the record) and a `belongs_to` write (the new parent must not be a descendant of the record). On SeaORM the walk and the write can share the transaction `create_*` and `update_*` already run in, though a concurrent write on another connection can still close a cycle unless the walk locks the rows it reads or runs at a serializable isolation level. The markdown store has no transactions: two concurrent updates (`a.parent_id = b`, `b.parent_id = a`) can each pass the walk and both land. The backends would then disagree under concurrency, which the parity contract does not allow, so the store leaves acyclicity to the consumer for now. A `before_create` or `before_update` hook can walk the ancestors and refuse.

## Design

- Reuse `{Child}ParentCycle(id)`, whose name was chosen for this. The payload stays the record's own id, so the variant and its `403` mapping do not change.
- Emit the walk in both backends' `create_*` and `update_*`, after the self-listing check and the missing-id check (§5.4's store check order), and before `{Child}ParentRequired`:
  - for each child a `has_many` write lists, refuse when the child is the record or one of its ancestors;
  - for a `belongs_to` to the same entity, refuse when the new parent is the record or one of its descendants. A required foreign key's root names itself and stays allowed.
- Bound the walk (by the record count, or a configurable depth) so a cycle already in the data, written before the check existed or by hand into a vault, ends the walk instead of looping.
- SeaORM: walk inside the write's transaction and take the rows it reads under a lock, or document the isolation level it needs per engine (B-SQLT).
- Markdown: run the walk and the writes under the vault's write lock (the one `create` probes ids under), so two writers in one process cannot interleave. Writers in separate processes stay best-effort, as every multi-record markdown write is (ADR 0001 contract item 2); the contract would say so.
- Add runtime parity scenarios in `crates/parity` for a two-record and a three-record cycle through `has_many` and through `belongs_to`, on create and update, each refused with nothing written.
