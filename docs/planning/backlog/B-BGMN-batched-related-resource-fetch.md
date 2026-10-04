---
type: backlog
schema_version: '2'
id: B-BGMN
tags:
- store
- jsonapi
- performance
- include
last_reviewed: '2026-10-04'
---

# Fetch related resources in one store call, not one `get_by_id` per id

The relationship handlers resolve related ids one at a time. In `src/servers/generators/http/relationship.rs` the generated `ontogen_{module}_fetch` helper (related-resource links) calls the target's `get_by_id` once per related id, and `ontogen_{module}_check_ids` (the linked-id check on relationship writes) does the same. A to-many relationship with N ids costs N store reads: N file reads on the markdown backend, N queries on SeaORM. The phase 3b `include` helper adds a third call site that repeats it per distinct id of a page.

## Proposal

Add a store-level `get_many_{plural}(ids: &[String]) -> Result<Vec<{Entity}>, AppError>` on both backends:

- Returns the entities in the order of `ids`.
- Skips ids that do not exist instead of failing, so callers can tell which were missing by comparing lengths or ids.
- SeaORM: one `WHERE id IN (...)` query, reordered to match `ids`.
- Markdown: read each file once (there is no cheaper primitive), but under one store call so locking and path resolution happen once.

Both implementations must behave the same, so add a runtime parity case in `crates/parity` (order, duplicates, a missing id, empty input).

## Where the generator uses it

- `fetch`: replace the per-id `get_by_id` loop with one `get_many`. The module-level API gets a matching `get_many` forwarder only if hand-written modules should be able to override it; otherwise the generated helper calls the store directly, which bypasses a hand-written `get_by_id` and so needs a decision against the "served through the module's `get_by_id`" rule in the wire contract (§9.1).
- `check_ids`: one `get_many`, then report the first requested id absent from the result as the target's `{Entity}NotFound`.
- Phase 3b `include`: shipped with per-id reads. The generated `ontogen_{module}_included` helper calls the target's `ontogen_{target}_fetch` once per relationship with the distinct unseen ids, which is one `get_by_id` per id. A `get_many` would replace that inner loop with one call per relationship.

## Why phase 3b kept per-id reads

- The contract (§7.5) and ADR 0004 already accept one read per distinct id, and de-duplication bounds the cost to the distinct ids of one page (at most `max_limit` times the relationships' fan-out).
- The store has no batch primitive on either backend. Adding one is store-generator work on both backends plus a parity case.
- The markdown backend gains nothing: it reads one file per id whichever way it is called.
- A store-level `get_many` bypasses a hand-written `get_by_id`, which needs its own decision against the wire contract (§9.1, §9.3).
