---
type: task
schema_version: '3'
status: in-progress
created: '2026-09-07'
last_reviewed: '2026-09-20'
impact: medium
complexity: medium
tags:
- servers
- store
- pagination
related:
- 2026-09-07-pagination-declared-per-module.md
- 2026-09-07-api-surfaces-with-own-store-accessor.md
relevance_note: In review as #159, with #166 extending it to state-scoped modules and the MCP generator. Spec ported from the dev monorepo, where it was authored as T-P0CH.
---
# Generated list handlers push limit and offset into the store

## Goal

When a list is paginated, the generated handler loads every row, takes `len()`
as the total, and slices the page in memory. For the driving consumer that
means whole tables per page request: `activity_streams` rows are about 50 KB
each, and Apple Health `observations` run to roughly 110k rows a year.

The generated forwarder should pass `limit` and `offset` down, and a generated
count should supply the total, so a page costs one bounded query plus one
`COUNT(*)`.

## Today

- `src/api/gen_crud.rs` — emits
  `pub async fn list(store: &Store) -> Result<Vec<T>, AppError> { store.list_<plural>(None, None).await }`
- `src/store/backends/seaorm/gen_crud.rs` — `generate_list` emits the store
  method with `limit`/`offset`; there is no count method.
- `src/store/backends/markdown/gen_crud.rs` — the markdown backend's
  `list_<plural>(limit, offset)`, kept at parity with the SeaORM one by
  `tests/backend_parity.rs`.
- `src/servers/generators/http.rs` — the paginated handler shape:
  `let all_items = ...; let total = all_items.len() as u64;` then `skip/take`.
- `src/servers/generators/ipc.rs` — `generate_paginated_ipc_handler` does the
  same.
- `src/servers/classify.rs` — `OpKind::List` classification from the fn name and
  params.

## Proposed

For a paginated module:

- `src/api/gen_crud.rs` emits `list(store, limit: Option<u64>, offset: Option<u64>)`
  and a new `count(store) -> Result<u64, AppError>`.
- Both store backends gain `count_<plural>(&self) -> Result<u64, AppError>`
  (`SELECT COUNT(*)` for SeaORM; directory entry count for markdown).
- The paginated HTTP and IPC handlers call `list(&store, Some(limit), Some(offset))`
  and `count(&store)` and build `PaginatedResult { items, total, limit, offset }`
  without materialising the table.
- Unpaginated modules keep `list(store)`; a hand-written custom `list` in a
  paginated module must take the two extra params or the build fails with a
  message naming the fn.
- The generated `count` is not a route: the classifier treats `count` as
  store-internal and emits no handler for it.

## Approach

1. In `src/store/backends/seaorm/gen_crud.rs`, add `generate_count` emitting
   `count_<plural>` with `Entity::find().count(self.db())`. Mirror it in
   `src/store/backends/markdown/gen_crud.rs` and extend
   `tests/backend_parity.rs`.
2. In `src/api/gen_crud.rs`, take the paginated flag through `ApiConfig` (a
   `paginated_entities: Vec<String>` derived from the surface's
   `paginated_modules`) and emit the two-param `list` plus `count` for flagged
   entities.
3. In `src/servers/classify.rs`, exclude `count` from route emission — it is a
   helper the paginated handler calls, not an operation.
4. In `src/servers/generators/http.rs` and `ipc.rs`, replace the materialising
   body with the two calls. Keep the `max_limit` clamp and the `default_limit`
   fallback.
5. Parser check in `src/servers/parse.rs`: a paginated module's `list` must
   declare `limit`/`offset`; otherwise `CodegenError` naming the module and fn.
6. Tests in `src/servers/tests.rs`: the generated paginated handler body
   contains `count(` and does not contain `.len()`; the unpaginated one is
   unchanged. Snapshot in `src/snapshots.rs` for the new `count_<plural>` store
   method.

## Files to touch

`src/api/gen_crud.rs`, `src/api/mod.rs`, `src/store/backends/seaorm/gen_crud.rs`,
`src/store/backends/markdown/gen_crud.rs`, `src/servers/classify.rs`,
`src/servers/parse.rs`, `src/servers/generators/http.rs`,
`src/servers/generators/ipc.rs`, `src/servers/tests.rs`, `src/snapshots.rs`,
`tests/backend_parity.rs`, `CHANGELOG.md`.

## Acceptance criteria

- [ ] AC-1: The generated SeaORM store for a paginated entity contains
      `pub async fn count_<plural>(&self) -> Result<u64, AppError>` and the
      markdown backend matches it in `tests/backend_parity.rs`.
- [ ] AC-2: The generated paginated HTTP handler body for that entity contains
      one `list(&store, Some(` call and one `count(&store)` call and no
      `.len()`.
- [ ] AC-3: A paginated module whose hand-written `list` lacks `limit`/`offset`
      fails the build with a message naming the module and `list`.

## Out of scope

- Cursor pagination; this is `limit`/`offset` only.
- Search (`q`) on paginated lists — a custom `search` handler per module.

## Dependencies

- [Pagination declared per module](./2026-09-07-pagination-declared-per-module.md)
  — the paginated flag per module is what selects the two-param forwarder.
