---
type: epic
schema_version: "1"
id: E0002
status: in-review
title: Markdown as a first-class store backend (ADR 0001)
created: 2026-06-06
last_reviewed: 2026-06-06
tags: [markdown-backend, store, adr-0001]
---
# Epic — Markdown as a first-class store backend

**Milestone:** SDLC integration program, item 0 (dev D-DX1Q / T-YO00)
**Status:** in-review — all 15 PRs open as a stack, awaiting human review/merge
**Design sources:** [ADR 0001](../../architecture/0001-markdown-as-store-backend.md) ·
[campaign design record](../../markdown-backend-campaign.md) ·
[id-string design note](../../id-string-constraint.md)

## Shipping log

Stacked PRs, each based on the previous (`md-backend/NN-…` branches); two
front-loaded showcase PRs were the user's edit surface, propagated downstream
by an executable conformance harness. Three adversarial design gates
(G1/G2/G3) ran between stages; every blocking finding was fixed in-stack
(summaries on #98, #101, #105).

| # | PR | Scope | Gate evidence |
|---|---|---|---|
| 00 | #95 | preflight: iron-log repair (didn't compile on main), id-string design note (pre-implementation, per the ADR's Notes), CI concurrency | iron-log builds; full-check |
| 01 | #97 | `markdown-store` runtime crate (showcase A): lossless `Document` round-trip, **byte-stable verbatim render** + no-op writes skip entirely (T-Z0PE substrate), wikilinks, traversal-proof paths, atomic fsops, `VaultHandle` w/ atomic derived-id create | 60+ unit/integration/doc tests; 4 runnable examples; cargo-audit clean after the serde_norway swap |
| 02 | #98 | consumer golden spec (showcase B) + campaign design record | golden tree guard; vault goldens round-trip verbatim through the real crate |
| 03 | #99 | SeaORM lift behind the `pub(crate) StoreBackend` seam | zero snapshot drift; gen_crud moved verbatim |
| 04 | #100 | `Backend` enum + markdown IR in ontogen-core (additive) | semver additive; snapshots unchanged |
| 05 | #101 | **the one breaking cutover**: `StoreConfig.backend`, 2-arg `gen_store`, `gen_markdown_io → MarkdownIoOutput`, Pipeline inference | snapshots byte-identical; iron-log builds unchanged; G2 PASS |
| 06 | #102 | wikilink policy: SeaORM passthrough, consumer-stub wart deleted, `serde(default)` on Create ids | the two declared snapshot diffs only, inspected hunk-by-hunk |
| 07 | #103 | legacy markdown layer (−1,422 lines) → `{Entity}Frontmatter` typed boundary; `enum_to_string` SeaORM coupling severed | new emission snapshot; seaorm snapshots untouched |
| 08 | #104 | **markdown CRUD emitter** + CI-executed `markdown-pilot` | matched the golden spec byte-for-byte on first conformance run; pilot smoke tests execute generated CRUD in CI |
| 09 | #105 | backend parity + negative control; full-set conformance; typed-write vault exemplar | `gen_api` byte-identical across backends; the parity check provably falsifiable; G3 satisfied in-suite |
| 10 | #106 | `examples/iron-log-md` twin | live HTTP CRUD + hand-edit survival; `diff -r` of generated api trees: empty |
| 11 | #107 | `examples/tasks-tracker` (HTTP + **MCP tool registry**) | MCP list/create over the vault verified live; slug derivation over the wire |
| 12 | #108 | `examples/notes-kb` (wikilink graph + markdown-vault composition) | two-crate boundary demo live (`--features vault-tags`) |
| 13 | #109 | site docs: markdown-backend guide; stale guides retargeted; **publish pause-point presented** | site builds (34 pages) |
| 14 | #110 | ADR amendments + accept; this shipping log | every promise rechecked line-by-line |

## ADR promise rollup

- Generation-time `Backend` choice — **shipped** (owned enum; amendment 1).
- Byte-identical `gen_api`/`gen_servers`/`gen_clients` — **shipped &
  CI-enforced** with a negative control (`tests/backend_parity.rs`).
- Hook lifecycle parity — **shipped**, including hook *value* parity
  (relations populated before `before_update`, a G1 catch).
- `id: String` contract — **pre-audited clean** (`docs/id-string-constraint.md`),
  enforced at the runtime's traversal-proof path boundary.
- Single-record atomicity — **shipped** (same-dir tempfile+fsync+rename);
  multi-record correctly absent.
- Wikilink citation format — **shipped** at the `{Entity}Frontmatter`
  boundary; Obsidian-compatible quoting verified.
- Stable list order + explicit cap — **shipped** (lexicographic-by-id;
  `OrderBy` deliberately absent — amendment 4).
- Follow-on work the ADR listed: lift PR, markdown codegen, pilot consumer,
  docs pass — **all shipped** (#99–#105, #104, #109).

## Follow-ups filed

- HTTP generator emits axum-0.7 route syntax (`:param`); axum 0.8 panics at
  router build — iron-log's own server is boot-broken on main. Generator
  fix to `{param}` + iron-log regen.
- Surgical per-key frontmatter rewriting (narrow the mutation-path
  normalization whitelist) — lands with the T-Z0PE fidelity harness.
- Derived (non-authoritative) m2m views + delete-time cascade link-cleanup
  (ADR amendment 5).
- `OrderBy` as a both-backends ADR if a consumer earns it (amendment 4).
- Batch operations + the reserved `AtomicityError`/`BatchPartialFailure`
  naming (amendment 3).
- A real Nuxt frontend over notes-kb's generated TS client (declared scope
  decision in #108).
- markdown-store → rust-markdown migration; markdown-vault's serde_yml →
  serde_norway move (RUSTSEC-2025-0067/-0068).
- ontogen-ts is release-plz-exposed by default (no exclusion) — decide
  alongside the markdown-store publish question (#109).
- JSON Canvas bridge (entity graph → `.canvas`) — deferred from the
  crate-extraction survey.
