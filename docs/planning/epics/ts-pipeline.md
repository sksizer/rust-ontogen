---
type: epic
schema_version: "1"
id: E0001
status: open/active
title: TypeScript bindings pipeline
created: 2026-05-14
last_reviewed: 2026-05-19
tags: [ontogen-ts]
---
# Epic — TypeScript bindings pipeline

**Milestone:** M1 — code generation core
**Status:** open/active (PR 1 shipped; 7 PRs remaining)
**Design source:** [OF-015](../tasks/OF-015-productionize-typescript-generation.md)
— full design pass, supported subset, decisions, alternatives, AC catalog.

## Goal

Replace the OF-014 specta side-car with a build-time AST → TypeScript emitter
(`crates/ontogen-ts/`). Long-tail user types are emitted from `syn` AST
inspection inside `build.rs`, eliminating the recursive cargo invocation,
the source-tree pollution, the watcher loops, and the CI disk pressure that
the spike imposes on every adopter.

The design pass behind this epic captured 14 decisions on 2026-05-14 (see
[OF-015 "Decisions captured during the design pass"](../tasks/OF-015-productionize-typescript-generation.md#decisions-captured-during-the-design-pass-2026-05-14)).
This epic doc is the active navigational hub — what ships in which PR, what's
landed so far, what's queued. It does not re-state the design decisions; OF-015
is the canonical record.

## Today

TypeScript bindings for ontogen consumers are produced by the OF-014 specta
side-car: a separate compilation that walks consumer source, emits a `.ts`
file via `specta`, and writes it back into the consumer tree. The side-car
imposes a recursive cargo invocation on every adopter build, pollutes the
source tree with generated artefacts, drives watcher loops in Tauri/iron-log
setups, and burns CI disk. `FallbackRecord` exists as a partial mitigation
for unknown types but mis-tokenizes generics (OF-018).

## Proposed

A new workspace member `crates/ontogen-ts/` exposes a pool-in API
(`emit(roots, type_pool, config) -> Result<String, Vec<EmitError>>`) backed
by `syn` AST inspection in `build.rs`. `gen_servers` calls into it; the
specta side-car, the `FallbackRecord` emitter, and the iron-log workarounds
are deleted. Phase-1 subset: named structs, C-style + externally-tagged
enums, primitives, hardcoded containers (`Option`/`Vec`/`HashMap`/`BTreeMap`),
smart-pointer transparency, references, external-types table; full serde
rename family (8 case modes, property-tested against `serde_json::to_string`);
`#[ontogen::ts_opaque]` and `#[ontogen::ts_name]` proc-macro attrs. Pumice
is the integration validator. User-facing docs land in PR 8.

## Tasks

Member tasks are tracked under `../tasks/OF-015-pr-*.md`. The wikilink form
the schema expects (`[[YYYY-MM-DD-slug]]`) does not match this project's
`OF-NNN` task-id convention; the canonical list is the PR table below until
the project renaming convention is reconciled with the SDLC schema.

8 PRs, sequential. Each PR is reviewed and merged before the next is filed
(per orchestrator preference — keeps review surface focused). PR 5 is the
functional cutover (ontogen-ts replaces side-car in `gen_servers`); PR 6 is
the deletion pass.

| PR | Scope | Phases | Status | Satisfies ACs |
|----|---|---|---|---|
| 1 | `crates/ontogen-ts/` scaffold + per-type emission | 1 + 2 | **shipped via #55** (2026-05-15) | AC-1, AC-2, AC-3 |
| 2 | Serde rename engine (8 modes, our own transforms, property tests) | 3 | queued | AC-4 |
| 3 | Type collection, topological ordering, use-resolution, external-types table | 4 + 5 | queued | AC-5, AC-6, AC-7 |
| 4 | Top-level `emit` entry point + `#[ontogen::ts_opaque]` / `#[ontogen::ts_name]` proc-macro attrs | 6 + 7 | queued | AC-8, AC-9, AC-10 |
| 5 | Ontogen wiring — `gen_servers` calls `ontogen_ts::emit` instead of `ts_sidecar::generate`; side-car code still present but unused | 8 | queued | AC-11 |
| 6 | Side-car deletion + iron-log workaround cleanup + `FallbackRecord` removal | 9 | queued | AC-12, AC-13, AC-14 |
| 7 | Pumice integration validation + any subset-gap backports into earlier PRs | 10 | queued | AC-15 |
| 8 | User-facing docs (new TS-bindings guide, `client-generation.mdx` rewrite, OF-019 doc rollback) | 11 | queued | AC-16 |

## Acceptance criteria

Full AC catalog lives in
[OF-015's "Acceptance criteria" section](../tasks/OF-015-productionize-typescript-generation.md#acceptance-criteria).
The rollup status here mirrors that catalog; check the PR-1 ticks against
that doc for the per-AC verification record.

- [x] **AC-1**: `crates/ontogen-ts/` is a workspace member; `cargo build` succeeds  → PR 1 (#55)
- [x] **AC-2**: Public API surface (`TypePath`, `EmitConfig`, `EmitError`, `emit` signature) settled  → PR 1 (#55)
- [x] **AC-3**: Per-type emission for the phase-1 supported subset  → PR 1 (#55)
- [ ] **AC-4**: Serde rename family with property tests  → PR 2
- [ ] **AC-5**: External-types table with shipped defaults  → PR 3
- [ ] **AC-6**: Type collection + topological ordering  → PR 3
- [ ] **AC-7**: Use-resolution + canonical paths + glob rejection  → PR 3
- [ ] **AC-8**: Top-level `emit()` composition + error aggregation  → PR 4
- [ ] **AC-9**: Proc-macro attrs ship  → PR 4
- [ ] **AC-10**: Name-collision detection  → PR 4
- [ ] **AC-11**: `gen_servers` wiring (functional cutover)  → PR 5
- [ ] **AC-12**: Side-car deletion  → PR 6
- [ ] **AC-13**: Iron-log workaround cleanup  → PR 6
- [ ] **AC-14**: Iron-log end-to-end clean build  → PR 6
- [ ] **AC-15**: Pumice integration validates phase-1 subset  → PR 7
- [ ] **AC-16**: User-facing docs land  → PR 8
- [ ] **AC-17**: `just full-check` + CI green after all PRs land  → spanning

## Out of scope

Spawned as follow-up tickets, deferred past this epic:

- User-defined generics → [OF-021](../tasks/OF-021-user-defined-generics-in-ts-emitter.md)
- Hierarchical TS output → [OF-020](../tasks/OF-020-hierarchical-ts-bindings.md)
- Richer external-type renderings (`moment.Moment`-style imports) → [OF-022](../tasks/OF-022-richer-external-type-renderings.md)
- Shape-changing serde attrs (`tag`, `content`, `untagged`, `flatten`) — phase 2

Naturally closes when this epic ships:

- [OF-018](../tasks/OF-018-ts-fallback-mistokenizes-generics.md) — TS fallback mis-tokenizes generics. Resolved when the `FallbackRecord` emitter is deleted in PR 6 (per OF-015's hard-error decision).

## Discovery context

- OF-015 stays as a task in `tasks/` even though this epic supersedes it
  spiritually — the design-pass artefact is more useful preserved than
  consolidated into the epic doc.
- The epic stays unnumbered (`ts-pipeline.md`, not `E0001.md`)
  because inbound links across `docs/` and the planning README pin the
  current filename; the immutable id `E0001` lives in frontmatter. Move
  the file to `E0001.md` once a renaming sweep is scheduled.
