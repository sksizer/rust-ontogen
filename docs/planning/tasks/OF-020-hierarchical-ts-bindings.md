---
schema_version: '3'
status: planning/proposed
impact: low
complexity: large
last_reviewed: '2026-05-26'
low_confidence: true
definition_gap: |
  Speculative follow-up. Implementation-ready template sections are
  populated in shape (Goal / Today / Proposed / Approach / Files to
  touch / Acceptance criteria / Out of scope / Dependencies / Discovery
  context) but three core design questions remain unresolved and the
  `## Approach` deliberately starts with a design pass rather than a
  concrete code change: (1) where schema-known types live in the
  nested layout (`_entities/` vs flat sibling), (2) how re-exported
  types canonicalize to a single owning module, (3) backward-compat
  strategy with the flat-bindings + `#[ontogen::ts_name]` baseline
  shipped by OF-015. Do not promote to open/ready until a real
  consumer hits collision-fatigue with the flat workflow AND a
  design pass closes the three open questions. `low_confidence: true`
  is set because the `Approach` is design-then-implement, not a
  direct execution plan.
---
# OF-020 - Hierarchical TS bindings output for codebases with name collisions at scale

- **Severity:** Low. Speculative future work — only earns its keep if a real consumer hits collision-fatigue with the flat-bindings + `#[ontogen::ts_name]` approach OF-015 ships. iron-log and Pumice are both well below that threshold today.
- **Status:** Open. Filed 2026-05-14 alongside the OF-015 design pass; not on the OF-015 critical path.
- **Related:** [OF-015](./OF-015-productionize-typescript-generation.md) (phase-1 ontogen-ts ships with flat `bindings.ts` + hard-error-on-collision + `#[ontogen::ts_name]` annotation as the named fix path). OF-020 is the "what if collisions become common enough that annotation-per-collision is no longer the right UX" follow-up.

## Goal

Give ontogen-ts an opt-in directory emission mode that mirrors the source Rust module structure, so consumers with large codebases and many same-terminal-ident collisions can disambiguate by import path (Rust-style) rather than annotating every collision site with `#[ontogen::ts_name]`.

## Today

| Location | Role today |
|---|---|
| `crates/ontogen-ts/src/emit.rs` (`emit`, `emit_with_imports`) | Single public entry point. Returns `Result<String, Vec<EmitError>>` — one flat TS source string with every reachable type concatenated in topological order. No per-module partitioning. |
| `crates/ontogen-ts/src/emit.rs` (name-collision detection, ~line 111) | Post-`ts_name` resolution, builds a `BTreeMap<String, Vec<TypePath>>`. Two distinct `TypePath`s mapping to the same TS name produce a hard `EmitError::NameCollision { name, paths }`. The user-facing escape hatch is `#[ontogen::ts_name = "..."]`. |
| `src/clients/config.rs::ClientGenerator` (line ~80) | Enum with three variants — `HttpTauriIpcSplit { output, bindings_path }`, `HttpTs { output, bindings_path }`, `AdminRegistry { output }`. `bindings_path` is a single `PathBuf` pointing at one flat file. |
| `src/clients/mod.rs` (lines 120, 255, 261) | `generate_clients` reads `bindings_path` as a single file path from `HttpTs` / `HttpTauriIpcSplit` and feeds it to the downstream emitters. |
| `src/clients/generators/transport.rs::generate` (line 80) | Takes `bindings_path: &Path`, calls `fs::read_to_string(bindings_path)`, then emits a single `transport.ts` referencing types from that flat file via a single import. |
| `src/clients/generators/ts_client.rs::generate` (line 21) | Same shape as `transport.rs::generate` — single-file read of `bindings.ts`, single-file import in the emitted `ts_client.ts`. |
| `src/clients/generators/mod.rs` (line 60) | The shared `bindings_path: PathBuf` field on the per-generator config struct passed to the downstream emitters. |

## Proposed

Introduce a `BindingsLayout` config knob and a parallel nested emission path. Phase-1 flat emission remains the default and the only mode iron-log/Pumice use. Opting into `BindingsLayout::Nested { dir }` causes ontogen-ts to emit a directory tree mirroring the source Rust module hierarchy, with one `index.ts` per module exporting the types defined there, relative cross-module imports between files, and a synthetic `_entities/` (or equivalent — see open question 1) location for schema-known types that don't have a natural user module. Downstream `transport.ts` / `ts_client.ts` emitters import from per-module sub-paths in nested mode. No bare aggregator re-exports — the whole point is import-path disambiguation.

## Approach

1. **Design pass to close the three open questions before any code change.** Decide (a) where schema-known entities live in the nested layout (synthetic `_entities/` directory vs flat `bindings/types.ts` sibling); (b) the canonicalization rule for types reachable via `pub use` re-exports (default: emit at the definition site, treat re-export aliases as a separate concern); (c) the backward-compat strategy (default: coexist via `BindingsLayout` variant — `Flat` is the indefinite default, `Nested` is strict opt-in, no auto-promotion). Capture the decisions in this task body and only then proceed.

2. **Add the `BindingsLayout` enum and plumb it through `ClientGenerator`.** New enum on `src/clients/config.rs` with `Flat { path: PathBuf }` and `Nested { dir: PathBuf }` variants. Update the `HttpTs` / `HttpTauriIpcSplit` variants to carry a `BindingsLayout` instead of (or alongside) `bindings_path: PathBuf` — the exact shape (replace vs add a sibling field) depends on the backward-compat decision in step 1.

3. **Add a directory-mode emitter to ontogen-ts.** New entry point in `crates/ontogen-ts/src/emit.rs` (or a new module like `crates/ontogen-ts/src/emit/nested.rs`) with signature roughly `pub fn emit_nested(roots, type_pool, imports, config) -> Result<HashMap<PathBuf, String>, Vec<EmitError>>`. Internally: walk reachable types as today, but partition by source module (`TypePath` minus terminal segment), then emit one `index.ts` per module containing the topologically-ordered subset, with relative `import { Foo } from '../bar';` lines for cross-module references. Schema-known types route to the synthetic location decided in step 1.

4. **Rework name-collision semantics under nested mode.** Within a single module, terminal-ident collisions still hard-error (the user can't have two `Stats` in `foo/` either). Across modules, terminal-ident collisions are *expected* and resolved by import path — no `EmitError::NameCollision`. The existing flat-mode collision check stays as-is.

5. **Wire downstream emitters.** Update `src/clients/generators/transport.rs::generate` and `ts_client.rs::generate` to accept a `BindingsLayout` instead of (or alongside) `bindings_path: &Path`. In `Nested` mode, the emitted `transport.ts` / `ts_client.ts` issue per-module imports (`import { Stats } from './bindings/foo';`) instead of one flat import. The per-type-to-module mapping is read from the directory layout produced by step 3 (or returned as a side-channel from `emit_nested`).

6. **Tests.** Unit test in `crates/ontogen-ts/`: two fixture modules each defining a `Stats` struct reachable from the same root set; `emit_nested` produces two files (`foo/index.ts`, `bar/index.ts`) with no flat-collision error, and any cross-module reference resolves to a relative import. Integration test alongside `examples/iron-log/` confirming nested mode is opt-in and the default flat output is byte-identical to today's.

7. **Documentation.** Update `site/` (TS-bindings guide once OF-015 PR 8 lands) with a "flat vs nested" section: when to pick which, the explicit-import pattern as the canonical use, and a migration recipe for users who outgrow flat.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `docs/planning/tasks/OF-020-hierarchical-ts-bindings.md` | modify | (this task) record design-pass decisions resolving the three open questions before any code change. |
| `src/clients/config.rs` | modify | introduce `BindingsLayout { Flat { path }, Nested { dir } }`; update `ClientGenerator::HttpTs` / `HttpTauriIpcSplit` to carry it (exact shape pending the backward-compat decision in step 1). |
| `crates/ontogen-ts/src/lib.rs` | modify | re-export the new nested-mode entry point alongside `emit` / `emit_with_imports`. |
| `crates/ontogen-ts/src/emit.rs` (or new `crates/ontogen-ts/src/emit/nested.rs`) | new or modify | `emit_nested(roots, type_pool, imports, config) -> Result<HashMap<PathBuf, String>, Vec<EmitError>>` + module-partitioning helpers + relative-import resolver. |
| `crates/ontogen-ts/src/order.rs` | modify | likely small touch — surface per-module partitioning during reachable-set walk so `emit_nested` doesn't re-scan. Confirm during step 3. |
| `src/clients/mod.rs` | modify | dispatch on `BindingsLayout` when handling `HttpTs` / `HttpTauriIpcSplit`; `Flat` keeps the existing `bindings_path` flow, `Nested` calls into the new directory writer. |
| `src/clients/generators/mod.rs` | modify | update the shared `bindings_path` field on the per-generator config struct (or replace with a `BindingsLayout`). |
| `src/clients/generators/transport.rs` | modify | accept `BindingsLayout`; in `Nested` mode emit per-module imports in the generated `transport.ts` instead of one flat import. |
| `src/clients/generators/ts_client.rs` | modify | same shape as `transport.rs` — accept `BindingsLayout` and emit per-module imports in `Nested` mode. |
| `src/clients/generators/ts_bindings.rs` | modify | the long-tail root-set computation likely needs no behavior change, but verify; may need to surface module-of-origin for `emit_nested` to consume. |
| `crates/ontogen-ts/tests/` | new | nested-mode integration test with two modules defining colliding terminal idents and a cross-module reference. |
| `site/` (TS-bindings guide page) | modify | document flat vs nested, when to pick which, migration recipe. Exact path depends on where OF-015 PR 8 lands the guide. |

## Acceptance criteria

- [ ] AC-1: The three open questions in `## Open questions` are answered in writing in this task body (or a linked design doc) before any code lands.
- [ ] AC-2: `BindingsLayout` enum exists on `src/clients/config.rs` with `Flat` and `Nested` variants, is documented with rustdoc, and the `ClientGenerator::HttpTs` / `HttpTauriIpcSplit` variants carry it (shape per the design pass).
- [ ] AC-3: `crates/ontogen-ts/` exposes a `emit_nested` (or equivalent) entry point that returns `Result<HashMap<PathBuf, String>, Vec<EmitError>>` — one file per Rust module, schema-known types routed to the location decided in AC-1.
- [ ] AC-4: A `crates/ontogen-ts/` test emits nested output for a fixture with two modules each defining a same-terminal-ident type reachable from the same root set; result is two `index.ts` files (no flat collision error) with any cross-module reference rendered as a relative import.
- [ ] AC-5: Within a single module, terminal-ident collisions still hard-error via `EmitError::NameCollision` — `#[ontogen::ts_name]` is still the per-module disambiguation tool.
- [ ] AC-6: `src/clients/generators/transport.rs` and `ts_client.rs` accept `BindingsLayout`; in `Nested` mode their emitted output imports from per-module sub-paths and there is a test covering at least one cross-module case.
- [ ] AC-7: `BindingsLayout::Flat` remains the default; `examples/iron-log/` builds with byte-identical (or trivially-equivalent) generated TypeScript — no regression for the phase-1 consumer.
- [ ] AC-8: User-facing docs cover flat vs nested, when to pick which, and a migration recipe for outgrowing flat.
- [ ] AC-9: `just full-check` passes on the rust-ontogen branch.

## Out of scope

- Auto-promotion from flat to nested based on a collision threshold — magic; users opt in explicitly.
- Per-collision granularity (some types nested, some flat) — premature complication.
- Bare `re-export` aggregator at `bindings/index.ts` that re-exports everything — defeats the whole import-path-disambiguation point; explicit per-module imports are the canonical use.
- Hard removal of `BindingsLayout::Flat` — phase-1 consumers stay on flat indefinitely.

## Dependencies

- [OF-015](./OF-015-productionize-typescript-generation.md) — must ship first; nested mode is layered on top of the OF-015 phase-1 flat baseline (`#[ontogen::ts_name]` + hard-error-on-collision).

## Discovery context

Filed 2026-05-14 alongside the OF-015 design pass. The design pass had to decide what ontogen-ts does when two Rust types with the same terminal ident reach the TS surface — TS has a flat module namespace and emits a duplicate-identifier `tsc` error. OF-015 chose the small-N answer (hard error + `#[ontogen::ts_name]` annotation for the rare collision) because iron-log has zero collisions and Pumice's count is small. OF-020 was filed as the large-N answer: emit a directory tree mirroring Rust modules so consumers disambiguate by import path the same way Rust does. It is explicitly speculative — it only earns its keep if a real consumer hits collision-fatigue with the annotation workflow. Pairs naturally with the still-open question from OF-015 about whether ontogen-ts publishes to crates.io; if external consumers exist outside ontogen's own pipeline, nested mode might earn its keep sooner.

## Problem

Phase-1 ontogen-ts emits a single flat `bindings.ts` file. TS at module scope has a flat namespace, so two Rust types with the same terminal ident (e.g., `crate::foo::Stats` and `crate::bar::Stats`) reachable from API roots collide on the TS side and produce a duplicate-identifier `tsc` error.

OF-015 phase-1 handles this with:

- **Hard error at ontogen-ts emit time** with a structured `NameCollision { name, paths }` error.
- **`#[ontogen::ts_name = "FooStats"]` annotation** for the user to disambiguate in TS without touching the JSON wire (our attribute, our semantics — serde ignores it).

That's the right answer for the codebases we know today (iron-log: zero collisions; Pumice: unclear but small). It scales poorly if a future consumer has a medium-to-large codebase with parallel sub-systems that name types similarly (`auth::Response` / `payment::Response`, `users::Settings` / `app::Settings`, etc.). At that point, requiring an annotation per collision becomes friction.

## Direction (sketch, not commitment)

Emit a TS *directory* instead of a single file, mirroring the user's Rust module structure:

```
bindings/
  _entities/          # ontogen-generated entity types + DTOs (no natural user module)
    index.ts
  foo/
    index.ts          # exports Stats, OtherFooType, ...
  bar/
    index.ts          # exports Stats, OtherBarType, ...
  index.ts            # re-exports / aggregator (optional)
```

Cross-module references emit relative imports between files. Schema-known types live in a synthetic `_entities/` (or similar) location.

Consumers import with the same disambiguation Rust gives them:

```ts
import { Stats } from '~/bindings/foo';
import { Stats as BarStats } from '~/bindings/bar';
```

The `ClientGenerator::HttpTs { bindings_path: PathBuf }` API would need to evolve (or coexist with) a directory-mode variant:

```rust
pub enum BindingsLayout {
    Flat   { path: PathBuf },        // phase 1 default
    Nested { dir:  PathBuf },        // this ticket
}
```

## Location

- `ontogen-ts` (assumes OF-015 has shipped) — emitter needs a directory-mode that:
  - Tracks each emitted type's source module path
  - Emits one TS file per Rust module containing the closure of types defined in that module
  - Generates relative imports between files for cross-module references
  - Handles schema-known types' synthetic location
- `src/servers/config.rs::ClientGenerator` — add the `BindingsLayout` variant (or equivalent).
- `src/servers/generators/transport.rs` + `ts_client.rs` — update the `transport.ts` / `ts_client.ts` emitters to import from the right sub-paths instead of one flat file.
- `site/...` — document the directory layout and when to pick which mode.

## Scope

In:

1. **Directory-mode emitter** in ontogen-ts. New entry point: `emit_nested(roots, type_pool, config) -> Result<HashMap<PathBuf, String>, Vec<EmitError>>`. Returns a map of relative paths → TS source per file.

2. **`BindingsLayout` config knob** on `ServersConfig` (or `ClientGenerator`). Phase-1 flat layout is the default; nested is opt-in.

3. **Downstream emitter wiring** — `transport.ts` and `ts_client.ts` import from the right per-module sub-paths in nested mode.

4. **Documentation** — when to use flat vs nested, how to choose, migration recipe for users who outgrow flat.

Out:

- **Auto-promotion from flat to nested** based on collision count. Magic; users should opt in explicitly.
- **Per-collision granularity** (some types nested, some flat). Premature complication.
- **Bare `re-export` aggregator at `bindings/index.ts`** that re-exports everything. Defeats the point of the layout. Document the explicit-import pattern as the canonical use.

## Effort

Medium. The emitter changes are the bulk of it — tracking per-module type ownership, generating relative imports, dedup-by-module rather than dedup-by-flat-name. The downstream `transport.ts` / `ts_client.ts` wiring is straightforward but touches several generators. Probably 3-4 dev days post-OF-015 if needed.

## Open questions

- **Where do schema-known types live in the nested layout?** A synthetic `_entities/` directory? A flat `bindings/types.ts` co-existing with per-module subdirs? The former is more consistent; the latter is simpler to emit. (Referenced from `## Approach` step 1.)
- **What about types that span modules via re-exports?** If `crate::foo::Stats` is `pub use`d at `crate::api::Stats`, which module does it emit under? Canonical path (where it's defined) is probably right, with re-export aliases as a separate concern. (Referenced from `## Approach` step 1.)
- **Backward compat for OF-015 phase-1 consumers**: hard cutover or coexist? Probably coexist via the `BindingsLayout` variant — phase-1 consumers stay on `Flat` indefinitely; nested is opt-in only. (Referenced from `## Approach` step 1.)

## Notes

- This ticket is *speculative* — only file work against it if a real consumer hits collision-fatigue. The `#[ontogen::ts_name]` annotation from OF-015 phase 1 covers the small-N case cleanly; OF-020 is the answer for large-N.
- Pairs naturally with the "decide whether ontogen-ts publishes to crates.io" question from OF-015. If ontogen-ts is published and external consumers exist outside ontogen's own pipeline, directory-mode emission might earn its keep faster than within ontogen's current consumer set.
