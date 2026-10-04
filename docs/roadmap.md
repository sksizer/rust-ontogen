---
title: Roadmap
description: Capability tiers and exit criteria for Ontogen.
---

# Ontogen Roadmap

Ontogen is a build-script-time code generator: schema files → persistence,
store, API forwarding, server transports (HTTP / Tauri IPC / MCP), and
TypeScript clients. The library runs as a `[build-dependencies]` of the
consuming crate; one `cargo build` produces the full stack.

The roadmap is organized in capability tiers. Each tier names its **exit
criteria** and the [epics](https://github.com/sksizer/rust-ontogen/tree/main/docs/planning/epics/) that compose it. Earlier tiers
are foundation; later tiers build on what came before without breaking it.

Status legend: `planned` · `in progress` · `shipped`

---

## M1 — Code-generation core · *shipped*

Schema parsing, persistence (SeaORM), store with CRUD + lifecycle hooks,
API forwarding, server transports (HTTP / Tauri IPC / MCP), TypeScript
client generation, admin registry. The "one `cargo build` produces the full
stack" foundation that everything else builds on.

| Epic                                                            | Status      |
|-----------------------------------------------------------------|-------------|
| [TypeScript bindings pipeline](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/epics/ts-pipeline.md)   | shipped     |

**Exit criteria:** a Tauri + frontend consumer can define entities in
`src/schema/`, write custom API endpoints in `src/api/v1/`, and get a
generated stack (persistence + store + API + HTTP/IPC/MCP transports +
TS client with full type bindings) that compiles clean with zero fallback
warnings, on `cargo build` alone. iron-log demonstrates this end-to-end;
Pumice validates it on a second consumer. **Met** — the TS pipeline epic
closed 2026-05-20 with Pumice validation in #67.

---

## M2 — Pipeline ergonomics · *shipped*

Cleaner API separation between servers and clients (the M1 entry points
grew organically). Architecting the pipeline to allow persistence backends
beyond SeaORM. Smoothing the rough edges that the M1 pass exposed once real
consumers (Pumice, iron-log, future adopters) hit them.

| Epic                                                            | Status  |
|-----------------------------------------------------------------|---------|
| [Markdown as a store backend](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/epics/markdown-backend.md) ([ADR 0001](https://github.com/sksizer/rust-ontogen/blob/main/docs/architecture/0001-markdown-as-store-backend.md)) | shipped |

**Exit criteria:** a consumer can swap in an alternative persistence layer
without touching the rest of the pipeline; the server/client split is
documented and stable. **Both met.**

- The `gen_servers` / `gen_clients` split landed with dedicated
  `ServersConfig` / `ClientsConfig` types.
- `StoreConfig::backend` selects the persistence backend at generation
  time. The markdown vault backend ships behind it, and everything above
  the store emits byte-identical output on either — enforced by
  `tests/backend_parity.rs`.

The backend seam is a closed enum by upstream design (ADR 0001,
alternative C): a third backend — diesel, sqlx-native — is a PR against
`Backend`, not an out-of-tree trait impl.

---

## M3 — Observability & extensibility · *planned*

Hooks for all entity operations (aspect-oriented patterns like logging,
audit trails, metrics). First-class error-type specification, threaded
through the full generation stack so downstream consumers can pin domain
errors at the wire boundary.

| Epic                                                                            | Status   |
|---------------------------------------------------------------------------------|----------|
| [Consumer-controlled HTTP error responses](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/epics/http-error-mapping.md) | proposed (envelope superseded by M4 JSON:API; mapping mechanism stands) |

**Exit criteria:** a consumer can register hooks at any CRUD entry point
without subclassing or wrapping the store; the wire error shape is
consumer-controlled rather than ontogen-imposed.

Shipped under this tier without an epic (0.7.0–0.8.0, 2026-09): API
surfaces with their own store accessor (#156), per-module pagination and
pushdown into the store (#159, #166, #172), schema enums and labels in the
admin registry (#160), the docs stage emitting the data-model reference and
JSON Schema (#161), the admin layer's pickers and packaging (#157, #158),
and resumable, parameterized event ops on server and client (#184, #185).

---

## M4 — Standard formats · *shipped*

Replace the two home-grown formats ontogen exposes to the outside world
with the open specifications that already cover them. On the wire, the
generated HTTP transport is a JSON:API 1.1 server and the TS transport its
client; on disk, the markdown backend writes an OKF 0.2 bundle by
default. Both are licensed by [ADR 0003](https://github.com/sksizer/rust-ontogen/blob/main/docs/architecture/0003-api-design-over-backwards-compatibility.md):
the break is taken once, without compatibility modes.

| Epic                                                                                       | Status   |
|--------------------------------------------------------------------------------------------|----------|
| [JSON:API as the generated HTTP wire format](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/epics/jsonapi-http-transport.md) ([ADR 0004](https://github.com/sksizer/rust-ontogen/blob/main/docs/architecture/0004-jsonapi-http-wire-format.md)) | shipped |
| [OKF-conformant markdown vaults by default](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/epics/okf-markdown-vault.md) ([ADR 0005](https://github.com/sksizer/rust-ontogen/blob/main/docs/architecture/0005-okf-markdown-vaults.md)) | shipped |

**Exit criteria:** a third-party JSON:API client performs CRUD and a
relationship fetch against an example server with no custom code; every
example vault passes an OKF conformance check in CI; IPC, MCP and the admin
layer are unchanged by either. The JSON:API `errors[]` document replaces the
`{"error"}` body that the M3 error-mapping epic assumed, so that epic's
envelope section is superseded while its status-mapping mechanism stands.
**Met** — the OKF epic closed 2026-10-03 ([#194](https://github.com/sksizer/rust-ontogen/pull/194),
[#196](https://github.com/sksizer/rust-ontogen/pull/196)) and the JSON:API epic 2026-10-04 ([#195](https://github.com/sksizer/rust-ontogen/pull/195),
[#197](https://github.com/sksizer/rust-ontogen/pull/197),
[#198](https://github.com/sksizer/rust-ontogen/pull/198),
[#199](https://github.com/sksizer/rust-ontogen/pull/199),
[#200](https://github.com/sksizer/rust-ontogen/pull/200),
[#201](https://github.com/sksizer/rust-ontogen/pull/201),
[#202](https://github.com/sksizer/rust-ontogen/pull/202),
[#203](https://github.com/sksizer/rust-ontogen/pull/203),
[#205](https://github.com/sksizer/rust-ontogen/pull/205),
[#206](https://github.com/sksizer/rust-ontogen/pull/206) and
[#207](https://github.com/sksizer/rust-ontogen/pull/207)).

- A third-party client, kitsu, drives `examples/tasks-tracker` through list,
  get, create, patch, delete and relationship fetches with only its
  documented options. The examples CI job runs it, and
  `just conformance-tasks-tracker` runs it locally.
- `tests/okf_conformance.rs` checks every example's seed vault on each CI
  run.
- IPC and MCP payloads stay flat, and the admin layer needed no source
  change. IPC and MCP list calls take an optional `sort`, and both get the
  store fixes the [wire contract](https://github.com/sksizer/rust-ontogen/blob/main/docs/jsonapi-wire-contract.md) lists in §15.

---

## Out of scope (for now)

- **A separate ORM.** Ontogen leans on SeaORM (and, in M2, optionally
  others). It's not in the business of inventing a new query language or
  schema definition syntax beyond the ontology annotations it already
  exposes.
- **Runtime code generation.** Ontogen is build-script-time only. No
  hot-reload, no codegen-at-server-startup, no dynamic schema changes.
- **A UI / admin app.** Ontogen emits the *admin registry* metadata; UIs
  that render it (Nuxt, React, etc.) are downstream.

---

## Architecture principles

Captured under [`architecture/`](https://github.com/sksizer/rust-ontogen/tree/main/docs/architecture/) as ADRs once they earn the
formal treatment. Principles that haven't yet warranted one are lived
through consistent practice and through individual task docs.

- [ADR 0001 — Markdown as a first-class store backend](https://github.com/sksizer/rust-ontogen/blob/main/docs/architecture/0001-markdown-as-store-backend.md)
  · *accepted* — establishes the backend seam, the id-as-filename
  constraint, and the "everything above the store is byte-identical"
  invariant that M2 exits on.
- [ADR 0003 — API design over backwards compatibility, until 1.0](https://github.com/sksizer/rust-ontogen/blob/main/docs/architecture/0003-api-design-over-backwards-compatibility.md)
  · *proposed* — pre-1.0, a cleaner surface takes the break: no shims,
  no compatibility flags. The licence M4 runs on.

## Planning artefacts

- [`planning/README.md`](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/README.md) — structural index: where
  epics and tasks live, how they link
- [`planning/epics/`](https://github.com/sksizer/rust-ontogen/tree/main/docs/planning/epics/) — capability slices, one file per
  epic
- [`planning/tasks/`](https://github.com/sksizer/rust-ontogen/tree/main/docs/planning/tasks/) — PR-sized work units; the open /
  closed backlog tables live in [`planning/tasks/README.md`](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/tasks/README.md)
- [`architecture/`](https://github.com/sksizer/rust-ontogen/tree/main/docs/architecture/) — ADRs
