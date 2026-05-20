---
type: task
schema_version: '2'
status: in-progress
created: '2026-05-19'
last_reviewed: '2026-05-20'
readiness_verified_at: '2026-05-20T04:25:53Z'
impact: medium
complexity: medium
autonomy: supervised
tags:
- architecture
- refactor
- clients
- servers
- breaking-change
- ts-pipeline
related:
- OF-015
- OF-022
relevance_note: |
  Relevance check 2026-05-20: all cited files exist and structural
  claims hold. Minor line drifts since 2026-05-19 (file growth in
  servers/mod.rs, config.rs, lib.rs — ranges in the spec are now
  approximate). One concrete staleness: the spec referenced
  `client-generation.mdx:204` (the `disable_codegen` knob mention in
  the Integration gotchas section) — that section was deleted in
  OF-015 PR 8 (#68); the reference is dead. The new
  `site/src/content/docs/guides/typescript-bindings.mdx` (also added
  in PR 8) likely needs no edits — it describes ontogen-ts in
  isolation. Spot-check during implementation.
---
# Split client SDK generation out of the `servers` module

## Goal

The `servers` module owns two unrelated concerns: server transport codegen (Rust output — Axum / Tauri IPC / MCP) and client SDK codegen (TypeScript output — bindings, HTTP client, split transport, admin registry). Promote the client concern to a sibling `clients` module with its own `gen_clients` / `ClientsConfig` entry points and a `Pipeline.clients(...)` stage. Pure separation-of-concerns cleanup with no behavioural change in generated output; public API break for every downstream `build.rs`.

Filed 2026-05-19 to act on the long-standing acknowledgement in `src/servers/mod.rs:1-4` (*"Also includes client generators... which will move to the `clients` module in a later phase"*). The "later phase" is now — OF-015 productionized the TS pipeline and there are no remaining reasons to keep the two entangled. Naturally follows [OF-015 / `ts-pipeline`](../epics/ts-pipeline.md); touches the same surface as OF-022 (richer external-type renderings) but the two are independent.

## Today

The `servers` module owns two distinct concerns:

1. **Server transport codegen** — Rust output: Axum HTTP routes (`HttpAxum`), Tauri IPC commands (`TauriIpc`), MCP tool registry (`Mcp`).
2. **Client SDK codegen** — TypeScript output: schema-known + long-tail bindings (`ts_bindings.rs` + the `ontogen-ts` AST walker), HTTP-only TS client (`HttpTs`), HTTP+IPC unified transport (`HttpTauriIpcSplit`), admin registry (`AdminRegistry`).

These share an upstream input (`ApiOutput` / the parsed `ApiModule` list) but produce outputs in different languages, for different runtime targets, with different review profiles. The current code conflates them at every level:

- **Module layout (`src/servers/generators/`):** `http.rs` / `ipc.rs` / `mcp.rs` (server) sit alongside `ts_bindings.rs` / `ts_client.rs` / `transport.rs` / `admin.rs` (client).
- **Config (`src/servers/config.rs`):** `GeneratorConfig::Server(ServerGenerator)` and `GeneratorConfig::Client(ClientGenerator)` are funneled through a single dispatch loop (`servers/mod.rs` around lines 320-360).
- **Public API (`src/lib.rs`):** `gen_servers` generates both; `ServersConfig` carries client-only fields (`client_generators`, `ts_skip_commands`, `schema_entities`) that have nothing to do with servers.
- **Pipeline (`src/pipeline.rs`):** only a `.servers(...)` stage exists; clients are reached transitively through `ServersConfig.client_generators`.
- **Long-tail TS wiring (`servers/mod.rs` around lines 255-320):** the `ontogen-ts` AST walker step — conceptually 100% client-side — lives inside `generate_transport`.
- **Docs (`docs/proposal.md`, `docs/walkthrough.md`, `site/src/content/docs/**`, `concepts/architecture.mdx`):** every reference to client generation has to re-explain that it lives "inside `gen_servers`".

The smell is acknowledged in the source itself:

```rust
// src/servers/mod.rs:3-4
//! Also includes client generators (TypeScript, admin registry) which will
//! move to the `clients` module in a later phase.
```

It is also surfaced obliquely in the OF-015 design docs and several site pages that warn "client generation runs inside `gen_servers` — there is no separate `gen_clients` function" (e.g., `guides/client-generation.mdx:33`). That note exists *because* the conflation surprises users.

## Proposed

Promote client SDK generation to a sibling stage of server transport generation. The split is structural, not functional — generated output is byte-identical before and after.

New shape:

```text
src/
├── servers/                ← Rust server transports only
│   ├── config.rs           (ServerGenerator only; no ClientGenerator)
│   ├── generators/
│   │   ├── http.rs
│   │   ├── ipc.rs
│   │   └── mcp.rs
│   └── mod.rs              (gen_servers entry point)
└── clients/                ← TS client SDKs + admin registry
    ├── config.rs           (ClientGenerator + ClientsConfig)
    ├── generators/
    │   ├── ts_bindings.rs
    │   ├── ts_client.rs    (HttpTs)
    │   ├── transport.rs    (HttpTauriIpcSplit)
    │   └── admin.rs        (AdminRegistry)
    └── mod.rs              (gen_clients entry point; owns the ontogen-ts long-tail wiring)
```

`gen_servers` returns to its name's meaning: Rust transports only.
`gen_clients` takes the parsed `ApiOutput` (or scans `api_dir` itself, mirroring `gen_servers`'s current fallback) plus a `ClientsConfig` and emits every TypeScript artefact.

Per-user decision (2026-05-19): **breaking split, no compat shims.** Downstream `build.rs` files update in lockstep — iron-log here, Pumice as a coordinated PR.

## Approach

1. **Move** the four client generator files (`ts_bindings.rs`, `ts_client.rs`, `transport.rs`, `admin.rs`) from `src/servers/generators/` to `src/clients/generators/` via `git mv`.
2. **Introduce** `src/clients/mod.rs` + `src/clients/config.rs` with `gen_clients`, `ClientsConfig`, and `ClientGenerator`.
3. **Move** the schema-known bindings emission and the long-tail `ontogen-ts` AST-walker integration (currently around `servers/mod.rs:238-318`) into `clients::generate`.
4. **Shrink** `ServersConfig` to server-only fields; drop `client_generators`, `ts_skip_commands`, `schema_entities` from it.
5. **Drop** `GeneratorConfig::Client` and the flat-list muxing in `servers/config.rs`; `ServerGenerator` becomes the only variant.
6. **Add** a `Pipeline::clients(ClientsConfig)` builder method + `clients_scan_dirs`. Pipeline runs `gen_servers` then `gen_clients` (order doesn't matter for correctness — neither produces input for the other — but stable ordering keeps the rerun-if-changed traces deterministic).
7. **Auto-forward** `schema.entities` from the schema stage into `ClientsConfig.schema_entities` (mirroring the current `ServersConfig.schema_entities` auto-forward at `pipeline.rs:371-373`).
8. **Update** `examples/iron-log/src-tauri/build.rs` to the new API.
9. **Update** all `docs/` and `site/` pages that document `ServersConfig.client_generators` / `gen_servers`-emits-clients to point at the new surface.
10. **Verify** with `just full-check` + a clean build of iron-log + a snapshot-output diff to confirm zero behaviour change.

## Files to touch

Files that move (use `git mv` to preserve history):

- `src/servers/generators/ts_bindings.rs` → `src/clients/generators/ts_bindings.rs`
- `src/servers/generators/ts_client.rs` → `src/clients/generators/ts_client.rs`
- `src/servers/generators/transport.rs` → `src/clients/generators/transport.rs`
- `src/servers/generators/admin.rs` → `src/clients/generators/admin.rs`

Files that change:

- `src/lib.rs` — add `pub mod clients;`; add `gen_clients(api, scan_dirs, &ClientsConfig) -> Result<(), CodegenError>`; remove client-only fields from `ServersConfig` (`client_generators`, `ts_skip_commands`, `schema_entities`); update the pipeline diagram in the module rustdoc; update the `gen_servers` rustdoc to drop the "also TS clients" line.
- `src/servers/config.rs` — drop the `GeneratorConfig::Client` variant; rename to flat `ServerGenerator` list on `Config`; keep `ServerGenerator`. Delete `ClientGenerator` from this file (moves to `clients/config.rs`).
- `src/servers/mod.rs` — remove the long-tail TS wiring (`written_bindings`, the `ontogen-ts` pool walk, `append_long_tail_to_bindings`, `rerun_if_changed_under`); narrow the dispatch loop to the three `ServerGenerator` variants; drop the `// Also includes client generators` header note.
- `src/clients/mod.rs` (new) — own `gen_clients`, the dispatch loop for `HttpTs` / `HttpTauriIpcSplit` / `AdminRegistry`, the schema-known bindings emission, and the long-tail `ontogen-ts` integration.
- `src/clients/config.rs` (new) — `ClientsConfig` (carries `api_dir`, `service_import_path`, `types_import_path`, `naming`, `client_generators`, `ts_skip_commands`, `schema_entities`, `rustfmt_edition`, `route_prefix`, `store_type`, `store_import`, `pagination`); `ClientGenerator` enum.
- `src/pipeline.rs` — add `.clients(ClientsConfig)` builder method and a `ClientsStage` struct; new stage runs after `servers` in `build()`; add `clients_scan_dirs(...)` setter mirroring `servers_scan_dirs`; the `Pipeline::servers(...)` doc loses the "client generators" mention.
- `src/snapshots.rs` / `src/servers/tests.rs` — relocate the client-side test cases to `src/clients/tests.rs` (new file). Snapshot files under `src/snapshots/` retain their names — they exercise the generated output, not the module path.

Cross-references that need rewriting (mechanical):

- `examples/iron-log/src-tauri/build.rs` — split the single `ServersConfig` into a `ServersConfig` + `ClientsConfig`; update imports (`use ontogen::clients::{ClientGenerator, ...}`); chain `Pipeline::...servers(servers_config).clients(clients_config)`.
- `examples/iron-log/README.md:65-66` — drop the "performed inline by `servers::generate_transport()`" wording.
- `site/src/content/docs/getting-started/your-first-entity.mdx`, `site/src/content/docs/guides/client-generation.mdx`, `site/src/content/docs/guides/server-transports.mdx`, `site/src/content/docs/cookbook/tauri-integration.mdx`, `site/src/content/docs/cookbook/mcp-integration.mdx`, `site/src/content/docs/concepts/architecture.mdx`, `site/src/content/docs/concepts/pipeline.mdx`, `site/src/content/docs/guides/api-layer.mdx` — every page that says "client generators are configured through `ServersConfig.client_generators`" or "client generation runs inside `gen_servers`" needs to point at `ClientsConfig` / `gen_clients` / `Pipeline.clients(...)` instead. The "no separate `gen_clients` function" disclaimer in `guides/client-generation.mdx:33` (line drift since 2026-05-19) and similar copy elsewhere all go away.
- `site/src/content/docs/guides/typescript-bindings.mdx` (added in OF-015 PR 8 after this task was drafted) — likely needs no edits; it describes ontogen-ts in isolation. Spot-check.
- `docs/proposal.md`, `docs/walkthrough.md`, `docs/architecture/*`, `docs/crate-extraction.md` (one-line aside about `ServersConfig` referencing `servers::NamingConfig` and `RoutePrefix` — `NamingConfig` likely belongs in a shared spot once both configs need it; see Open questions).
- `README.md` — pipeline diagram in the top-level README.

Pumice has the same `ServersConfig { client_generators: ... }` shape and will need a coordinated PR. Out of scope for this ticket (see Out of scope below) but mentioned in Open questions.

## Acceptance criteria

- [x] **AC-1**: `src/clients/` exists as a sibling module to `src/servers/`. The four files `ts_bindings.rs`, `ts_client.rs`, `transport.rs`, `admin.rs` are relocated via `git mv` so `git log --follow` from each new path reaches pre-move history.
- [x] **AC-2**: `gen_clients(api: Option<&ApiOutput>, scan_dirs: &[PathBuf], config: &ClientsConfig) -> Result<(), CodegenError>` is the public entry point for TypeScript and admin-registry generation. Signature mirrors `gen_servers`'s shape.
- [x] **AC-3**: `ServersConfig` no longer carries `client_generators`, `ts_skip_commands`, or `schema_entities`. Those move to `ClientsConfig`. `gen_servers` only ever emits Rust server transport handlers.
- [x] **AC-4**: `GeneratorConfig::Client` and the wrapping `GeneratorConfig` enum are deleted; `Config::generators` (in `servers/config.rs`) becomes `Vec<ServerGenerator>`.
- [x] **AC-5**: The `ontogen-ts` long-tail wiring (schema-known emission, `ontogen_ts::scan_src_dir`, `emit`, `append_long_tail_to_bindings`, `rerun_if_changed_under`) lives in `src/clients/mod.rs`, not `src/servers/mod.rs`.
- [x] **AC-6**: `Pipeline::clients(ClientsConfig)` exists and is invoked from `build()` after the servers stage. `Pipeline` auto-forwards `schema.entities` into `ClientsConfig.schema_entities` when empty, mirroring the current `ServersConfig.schema_entities` auto-forward.
- [x] **AC-7**: `examples/iron-log/src-tauri/build.rs` compiles and produces byte-identical output to the pre-split version. Snapshot files under `src/snapshots/` are unchanged. (`git diff` after the split run must show no edits to any generated file.)
- [x] **AC-8**: A repo-wide grep for the legacy surface (`grep -rIn --exclude-dir=target --exclude-dir=node_modules -e 'ServersConfig.*client_generators' -e 'gen_servers.*also.*TS' -e 'client_generators:' .`) returns hits only in: (a) `CHANGELOG.md`, (b) `docs/planning/tasks/2026-05-19-split-clients-from-servers.md` (this file), and (c) historical task entries in `docs/planning/tasks/OF-*` (legacy artefacts).
- [x] **AC-9**: Every site page that mentions "client generators run inside `gen_servers`" or "`ServersConfig.client_generators`" is rewritten to reference `gen_clients` / `ClientsConfig` / `Pipeline.clients(...)`. Specifically: `guides/client-generation.mdx`, `guides/server-transports.mdx`, `guides/api-layer.mdx`, `getting-started/your-first-entity.mdx`, `cookbook/tauri-integration.mdx`, `cookbook/mcp-integration.mdx`, `concepts/architecture.mdx`, `concepts/pipeline.mdx`. The site builds cleanly.
- [x] **AC-10**: The module-rustdoc pipeline diagram in `src/lib.rs:7-15` is updated. The new shape: `gen_api → ApiOutput → { gen_servers → ServersOutput, gen_clients → () }` (or equivalent — clients no longer hide behind servers).
- [x] **AC-11**: `just full-check` passes (fmt + clippy with `--deny warnings` + `cargo test`).
- [x] **AC-12**: `cargo build` succeeds in `examples/iron-log/src-tauri/` against the new API. Iron-log's end-to-end build (Rust + Nuxt) completes; the generated TS files (`generated/types.ts`, `generated/transport.ts`, `admin-registry.ts`) are byte-identical to the pre-split outputs.

## Out of scope

- **Renaming `NamingConfig`, `RoutePrefix`, `PaginationConfig`, `PrefixParam`** or moving them to a shared module. They are used by both server and client generators; the cleanest landing is probably `ontogen-core::servers_shared` (or a new shared module), but that's a separate cleanup. For this ticket, the new `ClientsConfig` imports them from `servers::` — slightly upside-down, but tolerable. See Open questions.
- **Pumice's `build.rs` migration.** Pumice consumes ontogen as a git-dep at a pinned rev; the maintainer migrates on their next rev bump. Heads-up via PR description and the feedback log.
- **A `disable_codegen` knob on `ClientsConfig`.** Mentioned as future work in earlier docs; still future work after this split. Filed separately when motivated.
- **Splitting `servers::parse` / `servers::classify`.** Both are AST-shape utilities consumed by client generators today (e.g., `ipc::command_name` is reused by `ts_client`). Either move them to `ontogen-core` (the right home — see `docs/architecture/ARCHITECTURE-FOLLOWUPS-2026-05-03.md` item #9) or expose them as `pub(crate)` from `servers` for the new `clients` module. Picking the latter for this ticket; the relocation is a separate cleanup.
- **Renaming `ServersOutput` to `TransportsOutput`** or splitting it. `ServersOutput` describes server-side routes/commands/tools, so its name is now correct after the split. Leave it.

## Dependencies

- None hard. Soft dependency: Pumice consumes ontogen as a git-pinned rev; once this PR merges, Pumice's next rev bump must include the `ClientsConfig` migration in its `build.rs`. Coordinate via the Pumice maintainer's normal cadence. Not a blocker for merging.

## Discovery context

**Effort.** Medium. Most of the lift is mechanical re-homing + doc rewrites. Roughly half a day:

- ~1h: the actual file moves + `Cargo.toml` adjustments (none needed — same crate) + the config-type split.
- ~1h: introducing `ClientsStage` in `Pipeline` and threading `schema.entities` through.
- ~1h: rewriting docs and the cookbook recipes. Eight pages on the site, plus `docs/proposal.md` and `docs/walkthrough.md`.
- ~1h: iron-log `build.rs` update, `just full-check`, end-to-end build of iron-log, snapshot diff verification.
- Buffer for the docs scope creeping (likely — the conflation is mentioned in many places).

Risk is incidental: easy to miss a doc reference, easy to forget that `ServersConfig.schema_entities` auto-forwards from the pipeline (must mirror in the new code path). A repo-wide `grep -rIn 'client_generators\|ts_skip_commands\|gen_servers' docs site README.md examples` after the move catches most of these.

**Why this is worth doing.**

- User-facing payoff: small but real. Future readers of `src/servers/` see only what the name promises; future readers of `src/clients/` see only TS+admin output. The mental model collapses from "servers does both" into two cohesive modules.
- Internal payoff: bigger. Removing `GeneratorConfig::Client` and the dispatch muxer simplifies the `generate_transport` body from ~140 lines to ~60. The schema-known + long-tail TS wiring (currently a long inline block in `servers/mod.rs`) becomes the natural body of `clients::generate` and stops looking misplaced.
- The choice not to ship a compat shim is deliberate: ontogen has a single visible downstream (iron-log) and one external consumer (Pumice, git-pinned by rev). The cost of carrying a deprecation surface for one release cycle is higher than the cost of a coordinated PR with Pumice's maintainer.
- The OF-015 epic admits this conflation as future work but never schedules the lift. This ticket schedules it.

## Open questions

- **Where does `NamingConfig` live?** Both `ServersConfig` and `ClientsConfig` need it (URL pluralization is shared between Axum route generation and TS client method names). For this ticket: it stays at `src/servers/types.rs` and `clients` re-imports it. The right long-term home is `ontogen-core` (or a new `ontogen-core::naming::config` module) — file as a follow-up if reviewers feel strongly. Same question for `RoutePrefix`, `PrefixParam`, `PaginationConfig`.
- **Where do `servers::parse` / `servers::classify` live?** Both are consumed by client generators today (`ts_client::generate` calls `parse::scan_api_dir` and `classify::classify_op`). Two options: (1) leave them in `servers/` and have `clients/` `pub(crate)`-import them — directionally awkward; (2) lift them to `ontogen-core` per `docs/architecture/ARCHITECTURE-FOLLOWUPS-2026-05-03.md` item #9. Defaulting to (1) for the cleanup-of-bounded-scope reason; (2) belongs in its own ticket.
- **Pumice timing?** Coordinate with the Pumice maintainer so the rev bump that picks up this split also picks up the `ClientsConfig` migration in their `build.rs`. Not a blocker for merging this ticket; Pumice pins by git rev.
- **Single PR or two?** The split is mechanical enough that a single PR is reviewable — the diff is dominated by file moves (which `git diff` represents as renames) and one large `ServersConfig` shrinkage. Per user direction (2026-05-19) this is "one big task" — single PR.
- **Update `docs/proposal.md` and `docs/walkthrough.md`?** Both are historical-narrative docs. The `walkthrough.md` example code (the `ServersConfig` literal around line 540 — line numbers drifted since 2026-05-19) will compile-break against the new API. Either update them in this PR (preferred — they're examples) or stamp them as snapshot-of-an-earlier-API at the top. Default: update.
