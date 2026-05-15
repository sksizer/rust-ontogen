---
status: in-progress
created: 2026-05-14
last_reviewed: 2026-05-15
design_source: ../tasks/OF-015-productionize-typescript-generation.md
tasks:
  - OF-015-pr-1-scaffold-and-emission.md          # shipped via #55
  # PR 2 onward filed under YYYY-MM-DD-<slug>.md as they are picked up
---
# Epic — TypeScript bindings pipeline

**Milestone:** M1 — code generation core
**Status:** in-progress (PR 1 shipped; 7 PRs remaining)
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

## Scope summary

In:
- New crate `crates/ontogen-ts/` with pool-in API
  (`emit(roots, type_pool, config) -> Result<String, Vec<EmitError>>`)
- Supported subset: named structs, C-style + externally-tagged enums,
  primitives, hardcoded containers (`Option`/`Vec`/`HashMap`/`BTreeMap`),
  smart-pointer transparency, references, external-types table
- Serde rename family (`rename`/`rename_all`/`skip`, all 8 case modes,
  our own transforms property-tested against `serde_json::to_string`)
- Macro attrs (`#[ontogen::ts_opaque]`, `#[ontogen::ts_name]`)
- Wire into `gen_servers`; delete side-car infrastructure; iron-log cleanup;
  Pumice integration; user-facing docs

Out (spawned as follow-up tickets):
- User-defined generics → [OF-021](../tasks/OF-021-user-defined-generics-in-ts-emitter.md)
- Hierarchical TS output → [OF-020](../tasks/OF-020-hierarchical-ts-bindings.md)
- Richer external-type renderings (`moment.Moment`-style imports) → [OF-022](../tasks/OF-022-richer-external-type-renderings.md)
- Shape-changing serde attrs (`tag`, `content`, `untagged`, `flatten`) — phase 2

## PR sequence

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

## Related follow-up tickets

- [OF-018](../tasks/OF-018-ts-fallback-mistokenizes-generics.md) — TS fallback mis-tokenizes generics. **Closes naturally** when the `FallbackRecord` emitter is deleted in PR 6 (per OF-015's hard-error decision).
- [OF-020](../tasks/OF-020-hierarchical-ts-bindings.md) — hierarchical TS output (per-module directory). Speculative; not on this epic's critical path.
- [OF-021](../tasks/OF-021-user-defined-generics-in-ts-emitter.md) — first-class user-defined generics. Speculative; phase 1 rejects with the concrete-type-alias workaround.
- [OF-022](../tasks/OF-022-richer-external-type-renderings.md) — richer external-type renderings (imported TS types). Speculative; phase-1 ships primitives only.

## Notes

- OF-015 stays as a task in `tasks/` even though this epic supersedes it
  spiritually — the design-pass artefact is more useful preserved than
  consolidated into the epic doc.
- The epic stays unnumbered (`ts-pipeline.md`, not `01-ts-pipeline.md`)
  because the roadmap doesn't yet enumerate epic execution order. Add a
  number prefix later if the roadmap formally schedules multiple epics in
  M1.
