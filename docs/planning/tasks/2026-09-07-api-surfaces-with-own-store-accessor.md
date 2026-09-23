---
type: task
schema_version: '3'
status: in-progress
created: '2026-09-07'
last_reviewed: '2026-09-20'
impact: high
complexity: large
tags:
- servers
- clients
- surfaces
related:
- 2026-09-07-pagination-declared-per-module.md
- 2026-09-07-pagination-pushdown-into-the-store.md
relevance_note: In review as #156. Spec ported from the dev monorepo, where it was authored as T-6071.
---
# A second API surface with its own store accessor, merged into one transport

## Goal

ontogen reads one `api_dir` per run and every generated handler calls the
literal `state.store().await`. A consumer that owns two crates — a platform
crate and a domain crate with its own generated CRUD forwarders — cannot
reach the second crate's store from a generated handler, and cannot merge
same-named modules into the one Axum router, the one `ipc_handler()` and
the one TS `Transport`.

The driving consumer is the dev monorepo's `determined-platform`, which needs
to read `determined-fitness`'s generated CRUD through a second surface with a
`fitness_store` accessor. Until it can, every fitness entity is served by
hand-written passthrough handlers.

## Today

- `src/lib.rs` — `ServersConfig` and `ClientsConfig` each carry one `api_dir`,
  one `service_import_path`, one `types_import_path`, one `store_type`, one
  `pagination`.
- `src/servers/config.rs` — the internal `Config` the generators read;
  `PaginationConfig { default_limit, max_limit }`.
- `src/servers/parse.rs` — `ApiFn` has `first_param_is_store` and no surface;
  `ApiModule` has name, functions, events, `is_singleton`.
- `src/servers/generators/http.rs` — emits `let store = state.store().await...`
  in two places (the unscoped store preamble and the scoped-handler preamble).
- `src/servers/generators/ipc.rs`, `src/servers/generators/mcp.rs` — the same
  literal.
- `src/clients/generators/transport.rs`, `ts_client.rs`, `ts_bindings.rs` — TS
  output keyed by module name.
- `src/clients/generators/admin.rs` — emits a registry entry for a module only
  when it has all five CRUD function names.
- `src/pipeline.rs` — `ServersStage` and `ClientsStage` hold one config plus
  `scan_dirs`; `build()` calls `gen_servers` and `gen_clients` once.

## Proposed

A surface config:

```text
ApiSurface { api_dir, service_import_path, types_import_path, store_accessor,
             store_type, pagination, paginated_modules }
```

The existing top-level fields on `ServersConfig` and `ClientsConfig` become the
primary surface; `extra_surfaces: Vec<ApiSurface>` adds more. `store_accessor`
defaults to `"store"`, so every existing consumer stays byte-identical.

- `ApiModule` and `ApiFn` carry the surface index and the accessor. `http.rs`,
  `ipc.rs` and `mcp.rs` emit `state.{accessor}().await` from the fn, not a
  literal.
- Imports are per surface. Two surfaces with a module of the same name import
  it under an alias and the handler calls through the alias.
- Same-named modules across surfaces merge under one route prefix and one TS
  method prefix. The five CRUD names (`list`, `get_by_id`, `create`, `update`,
  `delete`) may come from one surface only. A function name present in both
  surfaces is a build error naming the module, the function and both files.
- `Pipeline::servers_surface(ApiSurface)` and `Pipeline::clients_surface(ApiSurface)`
  push onto the respective config.

If review stalls, the change splits into two PRs: "surfaces" (accessor plus
per-surface imports, no merge) and "merge" (same-name modules).

## Approach

1. Add `ApiSurface` to `src/lib.rs` beside `ServersConfig`, with
   `extra_surfaces: Vec<ApiSurface>` on both public configs and on the internal
   `src/servers/config.rs` `Config`. Default `store_accessor` to `"store"`.
2. In `src/servers/parse.rs`, add `surface: usize` and `store_accessor: String`
   to `ApiFn` (and `surface` to `ApiModule`). Parse every surface's `api_dir`;
   stamp the index and accessor on each parsed fn.
3. Add `merge_surfaces(Vec<Vec<ApiModule>>) -> Result<Vec<ApiModule>, CodegenError>`
   in `src/servers/mod.rs`: group by module name, reject a CRUD five split
   across surfaces, reject a duplicate fn name, keep `is_singleton` from the
   primary surface.
4. In `src/servers/generators/http.rs`, `ipc.rs` and `mcp.rs`, replace the
   literal `state.store().await` with `state.{accessor}().await` read from the
   fn. Emit one `use` block per surface; alias the module when a name repeats.
5. In `src/clients/generators/transport.rs`, `ts_client.rs`, `ts_bindings.rs`
   and `admin.rs`, read the merged module list. Type imports come from the
   surface's `types_import_path`; the TS pool already handles cross-crate types
   through `pool_extra_roots`.
6. Add `Pipeline::servers_surface` and `Pipeline::clients_surface` in
   `src/pipeline.rs`.
7. Add the snapshot tests to `src/servers/tests.rs` and `src/clients/tests.rs`
   using `write_synthetic_api`. Run `just full-check`.

## Files to touch

`src/lib.rs`, `src/servers/config.rs`, `src/servers/parse.rs`,
`src/servers/mod.rs`, `src/servers/generators/http.rs`,
`src/servers/generators/ipc.rs`, `src/servers/generators/mcp.rs`,
`src/clients/generators/transport.rs`, `src/clients/generators/ts_client.rs`,
`src/clients/generators/ts_bindings.rs`, `src/clients/generators/admin.rs`,
`src/pipeline.rs`, `src/servers/tests.rs`, `src/clients/tests.rs`,
`CHANGELOG.md`.

## Acceptance criteria

- [ ] AC-1: `just full-check` passes, and a synthetic two-surface test in
      `src/servers/tests.rs` produces an HTTP handler containing
      `state.fitness_store().await` and a second handler containing
      `state.store().await` in the same generated file.
- [ ] AC-2: A synthetic test with the module `workout` in both surfaces (custom
      fns in one, the CRUD five in the other) emits routes
      `GET|POST /api/workouts` and `GET|PUT|DELETE /api/workouts/{id}` plus the
      custom routes, all under one `workout_*` command prefix.
- [ ] AC-3: A synthetic test with the fn `start` in both surfaces' `workout`
      module fails `gen_servers` with an error message naming `workout`,
      `start` and both surfaces.
- [ ] AC-4: With `extra_surfaces` empty, the insta snapshots in
      `src/snapshots.rs` and the golden tree under `tests/golden/` are
      unchanged.

## Out of scope

- Pushing `limit`/`offset` into the store query — see
  [pagination pushdown](./2026-09-07-pagination-pushdown-into-the-store.md).
- Any change to the TS long-tail type pool beyond what a second surface's
  `types_import_path` needs.
