---
type: task
schema_version: '3'
status: in-progress
created: '2026-09-07'
last_reviewed: '2026-09-20'
impact: medium
complexity: small
tags:
- servers
- clients
- pagination
related:
- 2026-09-07-api-surfaces-with-own-store-accessor.md
- 2026-09-07-pagination-pushdown-into-the-store.md
relevance_note: Folded into #156 as ApiSurface::paginated_modules. Spec ported from the dev monorepo, where it was authored as T-7UGF.
---
# Pagination declared per module, so the admin registry paginates only what needs it

## Goal

Pagination is one switch per run: `pagination: Some(...)` paginates every list
in the surface and the admin registry marks every entity `paginated: true`.

That is wrong for any realistic schema. The driving consumer's fitness surface
has 22 entities; four of them (`activity_streams`, `observations`,
`intake_entries`, `workouts`) can hold tens of thousands of rows, and the
catalogs hold under a hundred. A surface should be able to name the modules
that paginate.

## Today

- `src/servers/config.rs` — `PaginationConfig { default_limit, max_limit }`, one
  per config.
- `src/servers/generators/http.rs` — when `config.pagination` is set, every
  `list` op emits the `PaginatedResult` handler shape.
- `src/servers/generators/ipc.rs` — `generate_paginated_ipc_handler` runs for
  every `Vec<...>`-returning list when `config.pagination.is_some()`.
- `src/clients/generators/admin.rs` — emits `paginated: true, defaultLimit,
  maxLimit` for every entity when `config.pagination` is set, else
  `paginated: false`.
- `src/clients/generators/transport.rs`, `ts_client.rs` — TS list signatures
  take `(query, limit, offset)` for every module when pagination is on.
- `src/servers/parse.rs` — already parses a `//! ontogen:singleton` marker from
  a file's leading comment block into `ApiModule.is_singleton`; the same
  mechanism fits a `//! ontogen:paginated` marker.
- `packages/nuxt_admin_layer/app/composables/useAdminEntity.ts` — reads
  `config.paginated` per entity and unwraps `{ items, total }` only for those.

## Proposed

Pagination is declared per module, two ways that resolve to one flag:

- `paginated_modules: Vec<String>` on `ApiSurface` and on the primary config.
  The list names modules by name (`"workout"`, `"observation"`).
- A `//! ontogen:paginated` marker in the module file's leading comment block,
  parsed the way `ontogen:singleton` is, for consumers that prefer a
  source-side declaration.

`ApiModule` gains `is_paginated: bool`. Every generator branches on the module
flag instead of `config.pagination.is_some()`. `pagination` (the limits) stays
on the surface; a paginated module with no limits configured is a build error.
The admin registry emits `paginated: true` only for flagged modules. Unflagged
modules keep the plain `Vec<T>` shape end to end.

## Approach

1. In `src/servers/parse.rs`, parse `//! ontogen:paginated` /
   `// ontogen:paginated` into `ApiModule.is_paginated`, mirroring the singleton
   marker. Add `apply_paginated_overlay(modules, &[String])` beside
   `apply_singleton_overlay` for the config list.
2. Add `paginated_modules: Vec<String>` to `ApiSurface`, `ServersConfig`,
   `ClientsConfig` and the internal `Config`. Apply the overlay after parsing
   each surface.
3. In `src/servers/generators/http.rs` and `ipc.rs`, replace every
   `config.pagination.is_some()` branch with `module.is_paginated`; read the
   limits from the surface's `pagination`, and return a `CodegenError` naming
   the module when it is flagged but the surface has no limits.
4. In `src/clients/generators/admin.rs`, `transport.rs` and `ts_client.rs`, do
   the same for the TS signatures and the registry entry.
5. Tests in `src/servers/tests.rs` and `src/clients/tests.rs`: two modules in
   one surface, one flagged; assert the flagged one gets the `PaginatedResult`
   handler and `paginated: true`, the other keeps `Vec<T>` and
   `paginated: false`; assert the marker form and the config form agree; assert
   the flagged-without-limits error.

## Files to touch

`src/lib.rs`, `src/servers/config.rs`, `src/servers/parse.rs`,
`src/servers/mod.rs`, `src/servers/generators/http.rs`,
`src/servers/generators/ipc.rs`, `src/clients/generators/admin.rs`,
`src/clients/generators/transport.rs`, `src/clients/generators/ts_client.rs`,
`src/servers/tests.rs`, `src/clients/tests.rs`, `CHANGELOG.md`.

## Acceptance criteria

- [ ] AC-1: A synthetic surface with modules `workout` (flagged) and `muscle`
      (not flagged) generates an HTTP `workout_list` handler returning
      `PaginatedResult<Workout>` and a `muscle_list` handler returning
      `Vec<Muscle>`.
- [ ] AC-2: The generated `admin-registry.ts` for that surface contains
      `paginated: true` under the `workout` entry and `paginated: false` under
      `muscle`.
- [ ] AC-3: A module file carrying `//! ontogen:paginated` produces the same
      output as naming it in `paginated_modules`.
- [ ] AC-4: A flagged module on a surface with `pagination: None` fails
      `gen_servers` with an error naming the module.

## Out of scope

- Pushing `limit`/`offset` into the store query — see
  [pagination pushdown](./2026-09-07-pagination-pushdown-into-the-store.md).
- The admin layer's paging UI beyond what `useAdminEntity.ts` already does.
