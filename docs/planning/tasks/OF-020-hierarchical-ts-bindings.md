---
schema_version: '2'
status: planning/proposed
---
# OF-020 - Hierarchical TS bindings output for codebases with name collisions at scale

- **Severity:** Low. Speculative future work — only earns its keep if a real consumer hits collision-fatigue with the flat-bindings + `#[ontogen::ts_name]` approach OF-015 ships. iron-log and Pumice are both well below that threshold today.
- **Status:** Open. Filed 2026-05-14 alongside the OF-015 design pass; not on the OF-015 critical path.
- **Related:** [OF-015](./OF-015-productionize-typescript-generation.md) (phase-1 ontogen-ts ships with flat `bindings.ts` + hard-error-on-collision + `#[ontogen::ts_name]` annotation as the named fix path). OF-020 is the "what if collisions become common enough that annotation-per-collision is no longer the right UX" follow-up.

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

- **Where do schema-known types live in the nested layout?** A synthetic `_entities/` directory? A flat `bindings/types.ts` co-existing with per-module subdirs? The former is more consistent; the latter is simpler to emit.
- **What about types that span modules via re-exports?** If `crate::foo::Stats` is `pub use`d at `crate::api::Stats`, which module does it emit under? Canonical path (where it's defined) is probably right, with re-export aliases as a separate concern.
- **Backward compat for OF-015 phase-1 consumers**: hard cutover or coexist? Probably coexist via the `BindingsLayout` variant — phase-1 consumers stay on `Flat` indefinitely; nested is opt-in only.

## Notes

- This ticket is *speculative* — only file work against it if a real consumer hits collision-fatigue. The `#[ontogen::ts_name]` annotation from OF-015 phase 1 covers the small-N case cleanly; OF-020 is the answer for large-N.
- Pairs naturally with the "decide whether ontogen-ts publishes to crates.io" question from OF-015. If ontogen-ts is published and external consumers exist outside ontogen's own pipeline, directory-mode emission might earn its keep faster than within ontogen's current consumer set.
