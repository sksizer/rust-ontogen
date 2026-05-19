---
type: task
schema_version: '1'
status: ready
created: 2026-05-19
last_reviewed: 2026-05-19
impact: high
complexity: medium
autonomy: human-only
tags: [ontogen-ts, ts-pipeline, integration]
related: [OF-015, OF-015-pr-6]
---
# OF-015 PR 7 — Pumice integration validation

## Goal

Run ontogen-ts against Pumice's current branch before declaring OF-015 phase 1 done. Catalog any unsupported-shape errors; if any surface, backport fixes to PRs 1-4 *before* PR 6 (side-car deletion) lands so Pumice retains a working fallback throughout. After fixes, Pumice's build is clean against the new pipeline. Satisfies AC-15 of [OF-015](./OF-015-productionize-typescript-generation.md).

## Today

After PR 6, the side-car infrastructure is fully deleted and ontogen-ts is the sole long-tail emitter in ontogen. Iron-log builds clean (validated through PR 5 and confirmed through PR 6). Pumice's branch has NOT yet been exercised against ontogen-ts; its long-tail set may contain shapes the phase-1 supported subset doesn't cover. Pumice's codebase is in a separate repository (not in `rust-ontogen`).

## Approach

Two phases:

1. **Run ontogen-ts against Pumice locally.**
   - Pull Pumice's current branch.
   - Point Pumice's `Cargo.toml` at a local `path = "../rust-ontogen"` (or use a workspace-local override).
   - Run `cargo build` and observe ontogen's `cargo:warning` output.
   - Catalog every `EmitError` variant emitted: `UnsupportedShape` (Rust type → reason), `UnsupportedSerdeAttr` (Rust type → attr), `UnresolvedReference` (name → context), `NameCollision` (TS name → list of Rust paths).

2. **Reconcile gaps before PR 6's side-car deletion lands.**
   - For each catalogued error, decide: (a) backport a feature into the appropriate earlier PR (1-4), (b) recommend a user-side change (annotation with `#[ontogen::ts_opaque]` or `#[ontogen::ts_name]`, refactor into a concrete type alias, etc.), or (c) accept as a known limitation (filed as a follow-up ticket).
   - If category (a): the fix lands as an addendum commit in the relevant PR's branch (or as a follow-up commit on `main` if the PR has already merged); re-run ontogen-ts against Pumice; iterate until clean.
   - If category (b) or (c): record the recommendation; communicate to Pumice's maintainer.
   - Final state: `cargo build` on Pumice's branch succeeds with no `EmitError`s.

## Files to touch

- (Cross-repo) Pumice's working tree — temporary `Cargo.toml` path override + observation.
- `crates/ontogen-ts/src/` — addendum commits for any gap-closing features (depends on which gaps surface).
- `docs/planning/tasks/OF-015-pr-7-pumice-validation.md` — record the catalog and the reconciliation decisions in this task's body or in a sibling note.

## Acceptance criteria

These are AC-15 from OF-015 — restated here for per-PR scope:

- [ ] AC-15.1: ontogen-ts run against Pumice's current branch; every `EmitError` variant catalogued (type, attr, name, paths) in a list committed to this task or a sibling note.
- [ ] AC-15.2: Every gap classified as (a) backport, (b) user-side change, or (c) accepted limitation.
- [ ] AC-15.3: Backports land before PR 6 (side-car deletion) merges so Pumice retains a working fallback throughout. **NOTE**: if PR 6 has already merged before PR 7 runs (e.g., the user reviews/merges in order), backports land on `main` directly.
- [ ] AC-15.4: After reconciliation, `cargo build` on Pumice's branch succeeds with no `EmitError`s emitted by ontogen-ts.

## Out of scope

- **Docs** — PR 8.

## Dependencies

- [[OF-015-pr-6-delete-sidecar]] should have landed (or be close to) so the validation runs against the final ontogen-ts pipeline.
- **External dependency**: this task requires access to Pumice's repository, which is outside the `rust-ontogen` working tree. **Marked `autonomy: human-only`** — an LLM agent does not have Pumice's codebase available and cannot run the validation step. The agent can prepare a runbook (this document) and backport fixes once gaps are reported by the human running the validation.

## Discovery context

- OF-015 design pass (2026-05-14) flagged Pumice as the second real consumer of the long-tail emission path after iron-log; OF-015's migration-semantics decision explicitly relies on Pumice as a working-fallback bedrock during the cutover.
