---
type: task
schema_version: '3'
status: in-progress
created: '2026-05-20'
impact: medium
complexity: medium
autonomy: supervised
tags:
- follow-up
- ontogen-ts
- ts-pipeline
related:
- OF-015
- OF-015-pr-7
readiness_verified_at: '2026-05-23T21:09:40Z'
last_reviewed: '2026-05-23'
---
# ontogen-ts: include transitively-referenced field types of schema entities in the long-tail root set

## Goal

Schema entities can reference user-defined types in their fields (e.g. `TimerSession.interval_kind: IntervalKind`). The schema-known emitter renders the field as a bare TS ident (`interval_kind: IntervalKind`) but never emits the body of `IntervalKind` itself — that's the long-tail emitter's job. Today the long-tail root set is derived only from API endpoint params/returns; it does not include the closure of field types referenced by schema entities. The result: consumers like Pumice carry a `build.rs` workaround that appends type aliases by hand for any entity field whose type isn't separately reachable from an API surface.

## Today

`src/clients/mod.rs::generate_clients` (around line 123 — `generators::ts_bindings::long_tail(&modules, config, &config.schema_entities)`) computes the long-tail set from the parsed API modules and the schema-entities table. The `long_tail` function (now at `src/clients/generators/ts_bindings.rs:63` after the clients/servers split in #69) inspects API signatures only — entity field types whose definitions live outside the schema-known surface are never added to the root set. The pool now contains them (after OF-015 PR 7's `pool_extra_roots`), but they don't get emitted because they aren't roots.

Observed in Pumice (`/Users/sksizer/Developer/Pumice/src-tauri/build.rs`, function `append_pumice_enum_aliases`): three enum aliases (`IntervalKind`, `SessionStatus`, `CompletionKind`) referenced by `TimerSession` fields are appended to the generated `types.ts` by hand because ontogen-ts doesn't emit them. The same workaround would be needed by any consumer whose entity fields point at non-trivial sibling-crate types.

## Proposed

`ts_bindings::long_tail` (or a sibling pass in `generate_transport`) walks every `EntityDef` in `config.schema_entities`, collects the type idents referenced by each field that aren't already part of the schema-known surface, and adds them to the long-tail root set. ontogen-ts then emits their bodies natively; downstream `build.rs` files like Pumice's `append_pumice_enum_aliases` can be deleted.

Behavior for iron-log: unchanged (iron-log doesn't have entity fields pointing at non-schema-known types).

## Approach

1. **Identify the gap site.** Read `src/clients/generators/ts_bindings.rs::long_tail` and confirm the input set it currently computes. Note any existing handling of schema-known types so the new pass doesn't double-emit. (Pre-#69 this file lived under `src/servers/generators/`; the clients/servers split relocated it.)

2. **Walk schema entity fields.** For each `EntityDef` in `config.schema_entities`, iterate `entity.fields`. For each field whose `FieldType` references a non-primitive, non-schema-known ident, add that ident to a candidate set. The classifier already knows what counts as schema-known (it's the existing partition that drives `ts_bindings::emit`); reuse that boundary.

3. **Merge candidates into the long-tail root set.** The result of step 2 is unioned with the existing API-derived long-tail set before `ontogen_ts::emit` runs (call-site: `src/clients/mod.rs::generate_clients` around line 123, where the returned `long_tail` vec drives the `roots` construction). Pool membership is verified the same way the existing long-tail names are (bare-ident match, then terminal-segment fallback). Unresolved candidates surface as `UnresolvedReference` errors with the same hard-error semantics PR 4 established.

4. **Verify against iron-log.** `cd examples/iron-log/src-tauri && cargo build` must succeed unchanged — no new types should appear in `examples/iron-log/src-nuxt/app/generated/types.ts` because iron-log's entity fields are all primitives / schema-known types.

5. **Verify against Pumice** (cross-repo, supervised). Restore the `validate-ontogen-ts` branch's [patch] block and `pool_extra_roots` setup, point at the new feature branch, and confirm `IntervalKind` / `SessionStatus` / `CompletionKind` now appear in ontogen-ts's emitted long-tail section. Pumice's `append_pumice_enum_aliases` becomes a no-op (the marker guard short-circuits; in a follow-up PR Pumice can delete the helper entirely).

6. **Document.** Update `docs/planning/tasks/OF-015-pr-8-user-facing-docs.md` (or the typescript-bindings guide once PR 8 lands) to call out that entity field types are part of the root set, not just API surface types.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `src/clients/generators/ts_bindings.rs` | modify | extend `long_tail` (or add a sibling pass) to include the field-type closure of `EntityDef`s. |
| `src/clients/mod.rs` | modify | confirm the field-type-derived names flow through to the existing `roots`/`pool` plumbing in `generate_clients` (line ~123). Likely no edit needed if `long_tail` returns the merged set. |
| `tests/` | modify | add a fixture entity with a typed-enum field referencing a non-schema-known type and assert the emitted bindings include both the entity and the enum definition. |
| `docs/planning/tasks/OF-015-pr-8-user-facing-docs.md` | new | once PR 8 ships, note this in the supported-subset section of the new TS-bindings guide. |


## Acceptance criteria

- [ ] AC-1: An ontogen-ts unit/integration test exercising an `EntityDef` with a field whose type ident is defined only in the pool (not in the schema-known surface) — the emitted bindings include both the entity rendering and the type body.
- [ ] AC-2: `cargo build` in `examples/iron-log/src-tauri/` succeeds with byte-identical (or trivially-equivalent) generated TypeScript — no behavioral regression.
- [ ] AC-3: Pumice's `validate-ontogen-ts` branch (or its successor), pointed at this task's PR, builds clean AND `IntervalKind`/`SessionStatus`/`CompletionKind` are emitted by ontogen-ts (verified by reading `src-nuxt/app/generated/types.ts` and confirming Pumice's `append_pumice_enum_aliases` short-circuits via its marker guard).
- [ ] AC-4: `just full-check` passes on the rust-ontogen branch.

## Out of scope

- **Deleting `append_pumice_enum_aliases` from Pumice.** Lives on the Pumice side; a follow-up PR there once this task lands and a new ontogen tag is cut.
- **Cross-crate `pub use` resolution in the resolver** — separate concern. This task only widens the root-set derivation; `pool_extra_roots` (OF-015 PR 7) is still how sibling-crate types get into the pool.
- **Auto-deriving the root set for non-schema-known entity types referenced indirectly** (e.g., a long-tail type whose field references another long-tail type). The existing recursive walker in ontogen-ts already handles transitive references *inside the pool* during emission; this task only adds the schema-entity field types as additional explicit roots.

## Dependencies

- OF-015 PR 7 (`pool_extra_roots`) — already merged (#67). The pool must be wide enough to contain the field types before they can be promoted to roots. For sibling-crate types, the consumer still has to set `pool_extra_roots` themselves.

## Discovery context

- Surfaced by OF-015 PR 7's Pumice validation run on 2026-05-19. Pumice's build was clean (zero `EmitError`s) only because `append_pumice_enum_aliases` carries the gap; without that workaround, the generated TS would fail to type-check (`IntervalKind` referenced but undefined).
- Filed 2026-05-20 as a follow-up so PR 7 could close on its scoped ACs. See PR 7's "Observed gaps not surfaced as errors" section.

## Post-mortem

_Captured by /sdlc:task-work on 2026-05-23. PR: pending._

### Acceptance criteria coverage

- AC-1: auto — `tests/ts_entity_field_type_closure.rs::entity_field_type_appears_in_emitted_long_tail` plus the in-module unit test `long_tail_includes_entity_field_type_idents_not_in_schema_known_surface` pin both the integration and unit-level contract.
- AC-2: agent-manual — sub-agent ran `cargo build` in `examples/iron-log/src-tauri/`; `diff -u` of `examples/iron-log/src-nuxt/app/generated/types.ts` pre/post was empty (byte-identical TS).
- AC-3: deferred-user — Pumice cross-repo validation. The worktree has no path to the Pumice repo and the task spec explicitly marks AC-3 as supervised cross-repo work. Next step: `/sdlc:cross-repo-task-pr` (or manual) to restore Pumice's `validate-ontogen-ts` branch + `[patch]` block pointing at this PR, confirm `IntervalKind` / `SessionStatus` / `CompletionKind` appear in the emitted long-tail, and confirm `append_pumice_enum_aliases` short-circuits via its marker guard.
- AC-4: auto — final `/Users/sksizer2/.claude/plugins/sdlc/scripts/run_quality_checks.py --diff-against-baseline 5184b18 --line` returned `OK 1/1 (baseline-gated; pre-existing findings ignored)`.

### What worked

- The v2-to-v3 task migration produced a clean, table-form task body; no placeholder phrases, no parser errors after the post-#69 path refresh.
- Baseline capture against `origin/main` (5184b18) was a no-op against quality state (0 pre-existing findings), so the gate's per-verb diff was trivial.
- Sub-agent dispatch landed three focused commits (feat → test → docs) with conventional-commits subjects; no monolithic dump.
- The classifier-reuse strategy (consult the existing schema-known partition rather than re-deriving) kept the diff small — three private helpers + a fold in `long_tail`.

### Friction and automation gaps

- start_task.py rebase conflict on the `readiness_verified_at` / `last_reviewed` adjacency — the existing "lift readiness_verified_at from worktree → main" mechanism prevented the value-level conflict but not the line-adjacency one. When the verify commit's diff carries OTHER frontmatter additions (here: bundled body edits driven by a relevance-check refresh), git's 3-way hunk merge declares a conflict even though both sides agree semantically. Automation gap: start_task.py could also lift `last_reviewed:` (or any pending body-edit overlap) so the verify commit's diff has empty intersection with the start commit's diff at hunk granularity. Alternatively, the verify commit could be authored relative to a base that includes the start-commit's frontmatter shape so the rebase is trivially a fast-forward.
- Step 2's relevance check (post-#69 path drift) had no mechanism to atomically bundle the body edits into the existing flow. Today the only path is: edit the file on main → uncommitted → manually `cp` to the worktree → ensure-ready commits on the worktree → start_task.py re-bundles on main → rebase. The two file copies (main and worktree) drift in lock-step but with no automation that confirms they match before ensure-ready runs. Gap: a Step 2.5 helper that says "your relevance edits are in main; sync them to the worktree and stage in both checkouts" would remove the manual cp.
- Files-to-touch row for `docs/planning/tasks/OF-015-pr-8-user-facing-docs.md` was marked `kind: new` but that task file already exists (PR 8 shipped). The `new` label is what the v2-to-v3 transform inferred from the row's prose ("once PR 8 ships, note this…"), which read as future tense; in reality this is a `modify` on an existing OF-015-pr-8 doc OR — more cleanly — a different file entirely (the typescript-bindings guide under `site/`). The sub-agent correctly updated `site/.../typescript-bindings.mdx` rather than the OF-015-pr-8 task. Gap: the v2-to-v3 migration's prose-disambiguation heuristic could be tightened (a "new" cell that points at an existing file is a red flag the migration could surface as a `definition_gap`), and follow-up authors should remember the Files-to-touch row is the *spec*, not the implementation pointer.

