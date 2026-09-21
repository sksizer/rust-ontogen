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

## M1 — Code-generation core · *in progress*

Schema parsing, persistence (SeaORM), store with CRUD + lifecycle hooks,
API forwarding, server transports (HTTP / Tauri IPC / MCP), TypeScript
client generation, admin registry. The "one `cargo build` produces the full
stack" foundation that everything else builds on.

| Epic                                                            | Status      |
|-----------------------------------------------------------------|-------------|
| [TypeScript bindings pipeline](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/epics/ts-pipeline.md)   | in progress |

**Exit criteria:** a Tauri + frontend consumer can define entities in
`src/schema/`, write custom API endpoints in `src/api/v1/`, and get a
generated stack (persistence + store + API + HTTP/IPC/MCP transports +
TS client with full type bindings) that compiles clean with zero fallback
warnings, on `cargo build` alone. iron-log demonstrates this end-to-end;
Pumice validates it on a second consumer.

---

## M2 — Pipeline ergonomics · *shipped*

Cleaner API separation between servers and clients (the M1 entry points
grew organically). Architecting the pipeline to allow persistence backends
beyond SeaORM. Smoothing the rough edges that the M1 pass exposed once real
consumers (Pumice, iron-log, future adopters) hit them.

| Epic                                                            | Status  |
|-----------------------------------------------------------------|---------|
| [Markdown as a store backend](https://github.com/sksizer/rust-ontogen/blob/main/docs/architecture/0001-markdown-as-store-backend.md) | shipped |

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
| [Consumer-controlled HTTP error responses](planning/epics/http-error-mapping.md) | proposed |

**Exit criteria:** a consumer can register hooks at any CRUD entry point
without subclassing or wrapping the store; the wire error shape is
consumer-controlled rather than ontogen-imposed.

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

## Planning artefacts

- [`planning/README.md`](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/README.md) — structural index: where
  epics and tasks live, how they link
- [`planning/epics/`](https://github.com/sksizer/rust-ontogen/tree/main/docs/planning/epics/) — capability slices, one file per
  epic
- [`planning/tasks/`](https://github.com/sksizer/rust-ontogen/tree/main/docs/planning/tasks/) — PR-sized work units; the open /
  closed backlog tables live in [`planning/tasks/README.md`](https://github.com/sksizer/rust-ontogen/blob/main/docs/planning/tasks/README.md)
- [`architecture/`](https://github.com/sksizer/rust-ontogen/tree/main/docs/architecture/) — ADRs
