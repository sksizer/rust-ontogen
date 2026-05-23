---
type: task
schema_version: '3'
status: planning/proposed
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
---
# ontogen-ts: include transitively-referenced field types of schema entities in the long-tail root set

## Goal

Schema entities can reference user-defined types in their fields (e.g. `TimerSession.interval_kind: IntervalKind`). The schema-known emitter renders the field as a bare TS ident (`interval_kind: IntervalKind`) but never emits the body of `IntervalKind` itself — that's the long-tail emitter's job. Today the long-tail root set is derived only from API endpoint params/returns; it does not include the closure of field types referenced by schema entities. The result: consumers like Pumice carry a `build.rs` workaround that appends type aliases by hand for any entity field whose type isn't separately reachable from an API surface.

## Today

`src/servers/mod.rs::generate_transport` (around line 262 — `generators::ts_bindings::long_tail(...)`) computes the long-tail set from the parsed API modules and the schema-entities table. The function inspects API signatures only — entity field types whose definitions live outside the schema-known surface are never added to the root set. The pool now contains them (after OF-015 PR 7's `pool_extra_roots`), but they don't get emitted because they aren't roots.

Observed in Pumice (`/Users/sksizer/Developer/Pumice/src-tauri/build.rs`, function `append_pumice_enum_aliases`): three enum aliases (`IntervalKind`, `SessionStatus`, `CompletionKind`) referenced by `TimerSession` fields are appended to the generated `types.ts` by hand because ontogen-ts doesn't emit them. The same workaround would be needed by any consumer whose entity fields point at non-trivial sibling-crate types.

## Proposed

`ts_bindings::long_tail` (or a sibling pass in `generate_transport`) walks every `EntityDef` in `config.schema_entities`, collects the type idents referenced by each field that aren't already part of the schema-known surface, and adds them to the long-tail root set. ontogen-ts then emits their bodies natively; downstream `build.rs` files like Pumice's `append_pumice_enum_aliases` can be deleted.

Behavior for iron-log: unchanged (iron-log doesn't have entity fields pointing at non-schema-known types).

## Approach

1. **Identify the gap site.** Read `src/servers/generators/ts_bindings.rs::long_tail` and confirm the input set it currently computes. Note any existing handling of schema-known types so the new pass doesn't double-emit.

2. **Walk schema entity fields.** For each `EntityDef` in `config.schema_entities`, iterate `entity.fields`. For each field whose `FieldType` references a non-primitive, non-schema-known ident, add that ident to a candidate set. The classifier already knows what counts as schema-known (it's the existing partition that drives `ts_bindings::emit`); reuse that boundary.

3. **Merge candidates into the long-tail root set.** The result of step 2 is unioned with the existing API-derived long-tail set before `ontogen_ts::emit` runs. Pool membership is verified the same way the existing long-tail names are (bare-ident match, then terminal-segment fallback). Unresolved candidates surface as `UnresolvedReference` errors with the same hard-error semantics PR 4 established.

4. **Verify against iron-log.** `cd examples/iron-log/src-tauri && cargo build` must succeed unchanged — no new types should appear in `examples/iron-log/src-nuxt/app/generated/types.ts` because iron-log's entity fields are all primitives / schema-known types.

5. **Verify against Pumice** (cross-repo, supervised). Restore the `validate-ontogen-ts` branch's [patch] block and `pool_extra_roots` setup, point at the new feature branch, and confirm `IntervalKind` / `SessionStatus` / `CompletionKind` now appear in ontogen-ts's emitted long-tail section. Pumice's `append_pumice_enum_aliases` becomes a no-op (the marker guard short-circuits; in a follow-up PR Pumice can delete the helper entirely).

6. **Document.** Update `docs/planning/tasks/OF-015-pr-8-user-facing-docs.md` (or the typescript-bindings guide once PR 8 lands) to call out that entity field types are part of the root set, not just API surface types.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `src/servers/generators/ts_bindings.rs` | modify | extend `long_tail` (or add a sibling pass) to include the field-type closure of `EntityDef`s. |
| `src/servers/mod.rs` | modify | confirm the field-type-derived names flow through to the existing `roots`/`pool` plumbing in `generate_transport`. Likely no edit needed if `long_tail` returns the merged set. |
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
