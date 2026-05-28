---
schema_version: '3'
status: planning/proposed
impact: low
complexity: small
last_reviewed: '2026-05-26'
definition_gap: |
  Three minor open questions remain (default-set evolution, default-export
  import syntax, OF-020 per-file dedup when hierarchical TS output lands).
  None block implementation; each is small enough to settle when the work
  is picked up. Task stays planning/proposed because the work is
  speculative — promote to open/ready only when a real consumer asks for
  Moment/Luxon-style imported renderings.
---
# OF-022 - Richer external-type renderings in `ontogen-ts` (imported TS types, not just primitives)

- **Severity:** Low. Speculative future work; phase-1 ontogen-ts ships with a primitive-rendering model that covers chrono / time / uuid / url / std-net / serde_json::Value, all rendered as TS `string` or `unknown`. OF-022 is the generalization to renderings that require *importing* a TS type from a package.
- **Status:** Open. Filed 2026-05-14 alongside the OF-015 design pass; not on the OF-015 critical path.
- **Related:** [OF-015](./OF-015-productionize-typescript-generation.md) phase-1 ships the primitive-only model; OF-022 generalizes it.

## Goal

Generalize `EmitConfig::external_types` from a primitive-only `String` rendering to a richer `ExternalTypeRendering` enum so consumers can map Rust external types (e.g. `chrono::DateTime`) to *imported* TS types (e.g. `Moment` from `moment`), with the emitter collecting + deduplicating `import type` declarations at the top of the generated file.

## Today

Phase-1's external-types pipeline is primitive-only:

```rust
// EmitConfig.external_types (crates/ontogen-ts/src/types.rs, around line 186)
pub external_types: BTreeMap<String, String>,
```

```rust
// DEFAULT_EXTERNAL_TYPES (crates/ontogen-ts/src/external.rs, lines 25-45)
("chrono::DateTime", "string"),
("uuid::Uuid",       "string"),
("url::Url",         "string"),
("serde_json::Value", "unknown"),
// ...
```

| Location | Role today |
|---|---|
| `crates/ontogen-ts/src/external.rs` | Owns `DEFAULT_EXTERNAL_TYPES: &[(&str, &str)]` (canonical-path -> primitive TS rendering) and `resolve(canonical, user_overrides) -> Option<String>`. User-provided entries in `EmitConfig::external_types` win on conflict; matching ignores generic args (caller strips before lookup). |
| `crates/ontogen-ts/src/types.rs` (around line 186) | `EmitConfig::external_types: BTreeMap<String, String>` — canonical-path key -> TS rendering string. No import metadata can be carried because the value is a primitive `String`. |
| `crates/ontogen-ts/src/emit.rs` (around line 304, inside `emit_type`) | Sole consumer site: strips generic args + `crate::` prefix, constructs a `TypePath`, calls `external::resolve(...)` and returns the rendering verbatim. No prelude / import-collection machinery exists today; the emitter assembles output by joining per-type render strings with `"\n\n"` in `emit_with_imports`. |
| `crates/ontogen-ts/tests/end_to_end.rs` (around line 151) | `emit_user_override_wins_on_external_types` pins the current primitive-string contract; any value-type change must keep this passing (modulo a `.into()` ergonomic touch). |

## Proposed

Replace the `String` value type in `EmitConfig::external_types` with `ExternalTypeRendering`, a two-variant enum that carries either a primitive TS string (`Primitive`) or an `import type`-backed rendering (`Imported { module, name, local_name }`). The emitter collects every `Imported` rendering surfaced during type emission, dedups by `(module, name, local_name)`, and prepends `import type { ... } from '...'` declarations to the final output. A `From<&str> for ExternalTypeRendering` impl wraps bare strings as `Primitive`, preserving backward compat for existing consumers and the `DEFAULT_EXTERNAL_TYPES` table.

## Approach

1. **Introduce the enum in `external.rs`.** Add `pub enum ExternalTypeRendering { Primitive(String), Imported { module: String, name: String, local_name: Option<String> } }` (owned `String` rather than `&'static str` so user configs built at runtime work). Implement `From<&str>` and `From<String>` for `ExternalTypeRendering` mapping to `Primitive(s.into())`.
2. **Migrate `DEFAULT_EXTERNAL_TYPES` lookup.** Keep the const table as `&[(&str, &str)]` (still all primitives in the shipped default set); have `resolve(...)` build an `ExternalTypeRendering::Primitive(...)` on the fly. Return `Option<ExternalTypeRendering>` instead of `Option<String>`.
3. **Change `EmitConfig::external_types` value type** from `BTreeMap<String, String>` to `BTreeMap<String, ExternalTypeRendering>` in `types.rs`. Update the doc comment.
4. **Update the call site in `emit.rs`.** Where `resolve` returns today, dispatch on the variant: `Primitive(s)` -> return `s` (no change to current behavior); `Imported { module, name, local_name }` -> register `(module, name, local_name)` with a per-emission `ImportCollector` and return `local_name.unwrap_or(name).clone()` as the field-site rendering.
5. **Wire the import collector through `emit_with_imports`.** Today `emit_with_imports` accumulates `outputs: Vec<String>` and joins with `"\n\n"`. Thread a `&mut ImportCollector` (or a `RefCell` if avoiding `&mut` plumbing is preferred) through `emit_type`. After per-type emission completes, build the import-declaration block from the collector and prepend it (separator: `"\n\n"` so it reads naturally above the first `export type`).
6. **Dedup rule.** Two `Imported` entries with the same `(module, name, local_name)` triple emit one import line. Two entries with the same `(module, name)` but different `local_name` produce two import lines (a renamed and a non-renamed variant). Two entries with the same `local_name` but different `(module, name)` is a collision — error with a new `EmitError::ExternalImportCollision { local_name, sources: Vec<(String, String)> }` variant (added to `types.rs`).
7. **Import-line format.** `import type { Name } from 'module';` for the no-rename case; `import type { Name as LocalName } from 'module';` for renames. Multiple types from the same `module` collapse into one `import type { A, B } from 'module';` line (alphabetical, for stable diffs).
8. **Tests.** Add fixtures + unit tests for: single `Imported` rendering; two types sharing module/name (one import); two types same module different names (one combined import); `local_name` rename; collision error.
9. **Backward-compat smoke test.** Existing `emit_user_override_wins_on_external_types` test should compile with at most a `.into()` change on the override value; ideally compiles unchanged via the `From<&str>` impl on the map's value.
10. **Docs.** Add a guide section at `site/src/content/docs/guides/typescript-bindings.mdx` covering advanced external-type rendering: when to use `Imported` (Moment/Luxon/Temporal/branded-type patterns), the dedup behavior, and the collision rule.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `crates/ontogen-ts/src/external.rs` | modify | introduce `pub enum ExternalTypeRendering` with `Primitive` and `Imported` variants; add `From<&str>` / `From<String>` impls; change `resolve` return type to `Option<ExternalTypeRendering>`; update inline tests. |
| `crates/ontogen-ts/src/types.rs` | modify | change `EmitConfig::external_types` value type from `String` to `ExternalTypeRendering`; add `EmitError::ExternalImportCollision { local_name, sources }` variant. |
| `crates/ontogen-ts/src/emit.rs` | modify | add `ImportCollector` (in-emit data structure), dispatch on `ExternalTypeRendering` variants in `emit_type`, thread the collector through `emit_with_imports`, prepend import-declaration block to the joined output, raise `ExternalImportCollision` when two distinct `(module, name)` pairs map to the same `local_name`. |
| `crates/ontogen-ts/src/lib.rs` | modify | re-export `ExternalTypeRendering` from `crate::external` at the crate root so consumers can name it without reaching through `external::`. |
| `crates/ontogen-ts/tests/end_to_end.rs` | modify | add end-to-end tests: imported rendering produces correct import + field-site reference; dedup; rename; module-collapse; collision error. Update `emit_user_override_wins_on_external_types` if the `.into()` ergonomics need a touch. |
| `crates/ontogen-ts/tests/fixtures/external_imported_*.rs` and `.ts` | new | golden fixtures for: single imported rendering, two-type dedup, rename, multi-name single-module import-line collapse. |
| `site/src/content/docs/guides/typescript-bindings.mdx` | modify | add an "Advanced external-type rendering" section covering `Imported` renderings, dedup behavior, collision rule, and a canonical example (Moment is the obvious one). |

## Acceptance criteria

- [ ] AC-1: `pub enum ExternalTypeRendering { Primitive(String), Imported { module: String, name: String, local_name: Option<String> } }` exists in `crates/ontogen-ts/src/external.rs` and is re-exported from the crate root.
- [ ] AC-2: `From<&str> for ExternalTypeRendering` wraps the input as `Primitive(s.into())`; an existing consumer's `external_types.insert("uuid::Uuid".into(), "string".into())` still compiles (backward-compat assertion via the pre-existing `emit_user_override_wins_on_external_types` test or a near-equivalent).
- [ ] AC-3: Unit test: two field types both configured to render as `Imported { module: "moment", name: "Moment", local_name: None }` produce exactly one `import type { Moment } from 'moment';` line at the top of the emitted output; the field sites reference the local name `Moment`.
- [ ] AC-4: Unit test: `Imported { module: "whatwg-url", name: "Url", local_name: Some("ParsedUrl") }` emits `import type { Url as ParsedUrl } from 'whatwg-url';` and the field site renders as `ParsedUrl`.
- [ ] AC-5: Unit test: two `Imported` renderings from the same `module` with different `name`s collapse to a single combined import line (alphabetical: `import type { A, B } from 'module';`).
- [ ] AC-6: Unit test: two distinct `(module, name)` pairs mapped to the same `local_name` produce an `EmitError::ExternalImportCollision` with both sources listed.
- [ ] AC-7: Existing `DEFAULT_EXTERNAL_TYPES` defaults continue to resolve as `Primitive(...)` and existing emitter behavior on primitives is byte-identical to today.
- [ ] AC-8: Guide section at `site/src/content/docs/guides/typescript-bindings.mdx` covers `Imported` rendering with at least one worked example.
- [ ] AC-9: `just full-check` passes on the rust-ontogen branch.

## Out of scope

- **Value-transforming wrappers** that change the wire shape (e.g., rendering `DateTime` as `{ value: string, timezone: string }`). The wire shape is owned by the Rust serde impl, not the TS rendering side.
- **Per-field overrides** (rendering `DateTime` differently in different structs). Probably never — the rendering choice is global to the project's TS surface.
- **TS-side type narrowing / brand-type machinery** as a built-in. Users can express it themselves via `Imported { module: "./brands", name: "WorkoutId" }`; ontogen-ts doesn't need to bake brand-type logic in.
- **Default-export import syntax** (`import Moment from 'moment'` rather than `import type { Moment } from 'moment'`). `import type` forces named-only; default exports in TS package surfaces have been discouraged for years. Defer until a consumer surfaces a real need.
- **Per-file dedup under OF-020 hierarchical layout.** If OF-020 ships first the imported-rendering dedup needs to be per-file; this task assumes the single-file phase-1 layout. Cross-reference handled in OF-020's plan when that ticket gets picked up.

## Dependencies

- [OF-015](./OF-015-productionize-typescript-generation.md) phase-1 must have shipped (the primitive-rendering pipeline this task generalizes).

## Discovery context

Filed 2026-05-14 alongside the OF-015 design pass as a known future generalization of the primitive-only external-types model. Phase-1's `external_types: BTreeMap<String, String>` covers ~90% of real-world Rust wire types by rendering them as TS primitives, but doesn't support consumers who want richer typed wrappers on the TS side (Moment, Luxon, Temporal, branded ID types, project-local TS classes). The design space is well-understood — `Primitive` + `Imported` with module/name/local_name is the natural shape, and `From<&str>` keeps the migration cost at one `.into()` per existing config entry. Speculative until a real consumer asks for it; status stays `planning/proposed` until then.

## Problem

Phase-1 ontogen-ts renders external types as TS primitives:

```rust
// EmitConfig.external_types
{
  "chrono::DateTime" => "string",
  "uuid::Uuid"       => "string",
  "url::Url"         => "string",
}
```

Walker matches a canonical path → emits the primitive at the field site:

```ts
export type Workout = {
  id: string,                  // from uuid::Uuid
  started_at: string,          // from chrono::DateTime<Utc>
  source: string,              // from url::Url
};
```

That covers ~90% of real-world Rust HTTP wire types. It does *not* cover the case where a consumer wants the TS side to use a richer typed wrapper for a wire-string — e.g., `moment.Moment`, `luxon.DateTime`, `temporal.Instant`, `Url` from a TS URL-handling package, or a project-local `Branded<string, "WorkoutId">` type.

The rendering for those cases isn't a TS primitive — it's an *imported* TS type:

```ts
// What OF-022 needs to make possible:
import type { Moment } from 'moment';
import type { Url as ParsedUrl } from 'whatwg-url';

export type Workout = {
  id: string,                  // Uuid still renders as string
  started_at: Moment,          // chrono::DateTime → Moment
  source: ParsedUrl,           // url::Url → ParsedUrl
};
```

Phase-1's `external_types: HashMap<TypePath, &'static str>` (primitive only) can't express this — the value carries no import metadata.

## Direction (sketch, not commitment)

Generalize the value type from `&'static str` to a richer rendering enum:

```rust
pub enum ExternalTypeRendering {
    /// Render as a TS primitive: "string", "number", "boolean", "unknown".
    /// Phase-1 default.
    Primitive(String),

    /// Render as a type imported from a TS package.
    /// Emitter collects all `Imported` renderings, deduplicates by
    /// (module, name), and emits `import type { Name } from 'module'`
    /// at the top of bindings.ts. Field sites reference the local
    /// name (with the rename if provided).
    Imported {
        module: String,                  // 'moment', 'luxon', 'whatwg-url'
        name:   String,                  // 'Moment', 'DateTime', 'Url'
        local_name: Option<String>,      // import as 'ParsedUrl' instead
    },
}
```

Walker behavior:
- Existing primitive-match path: unchanged.
- Imported-match path: emit the configured `local_name` (or `name`) at the field site; register the (module, name, local_name) tuple with a collector.
- After all types emitted: prepend `import type { ... } from '...'` declarations for the collected imports.

Consumer config:
```rust
EmitConfig {
    external_types: BTreeMap::from([
        ("chrono::DateTime".into(), ExternalTypeRendering::Imported {
            module: "moment".into(),
            name:   "Moment".into(),
            local_name: None,
        }),
        ("url::Url".into(),         ExternalTypeRendering::Imported {
            module: "whatwg-url".into(),
            name:   "Url".into(),
            local_name: Some("ParsedUrl".into()),  // avoid collision with built-in Url
        }),
    ]),
    ..Default::default()
}
```

## Location

- `ontogen-ts` (assumes OF-015 has shipped):
  - `EmitConfig::external_types`: value type evolves from `&'static str` to `ExternalTypeRendering`. Backward-compatible via a `From<&str> for ExternalTypeRendering` impl that wraps as `Primitive(s)`.
  - Walker: extend match path to emit local name + register import.
  - Emitter: prepend import declarations to output. Deduplicate.
- `site/...`: document the richer config shape and when to use it.

## Scope

In:

1. **`ExternalTypeRendering` enum** with `Primitive` and `Imported` variants.
2. **Import collector + deduplication** in the emitter. Two types rendering as `Imported { module: "moment", name: "Moment" }` → one `import type { Moment } from 'moment'` at the top.
3. **`local_name` rename** for collision avoidance (a `Url` type from `whatwg-url` vs. the user's own `Url` type, etc.).
4. **Backward-compat conversion** so existing primitive configs (`"string"`, `"unknown"`) keep working. `From<&str>` and probably a literal-string convenience: `external_types.insert("uuid::Uuid", "string".into())` still type-checks.
5. **Documentation** — guide section on "advanced external-type rendering" covering Moment / Luxon / Temporal / branded-type patterns.

Out:

- **Value-transforming wrappers** that change the wire shape (e.g., "render `DateTime` as `{ value: string, timezone: string }` instead of just `string`"). That would require the wire shape to *also* change, which is out of ontogen's scope (ontogen doesn't synthesize serde impls). If a user wants a non-canonical wire shape they own the Rust serde impl, not the TS rendering side.
- **Per-field overrides** (rendering `DateTime` differently in different structs). Probably never — the rendering choice is global to the project's TS surface, not per-field.
- **TS-side type narrowing** (`Brand<string, "WorkoutId">` for nominal-typed string IDs) as a built-in pattern. Users can express this themselves via `Imported { module: "./brands", name: "WorkoutId" }`; ontogen-ts doesn't need to bake brand-type machinery in.

## Effort

Small. The `Imported` variant adds maybe 30 LoC to the emitter (collector + dedup + import-block emission). The bigger cost is documentation — picking a canonical example (Moment? Luxon? Temporal?) and explaining the tradeoffs. Probably 1-1.5 dev days total.

## Open questions

- **Default-set evolution**: should any of phase-1's primitive defaults be replaceable with imported renderings when OF-022 ships? E.g., a "richer mode" config flag that swaps `chrono::DateTime → "string"` for `chrono::DateTime → Imported { module: "moment", name: "Moment" }`. Probably not — too magical. User explicitly opts each type into the richer rendering they want.
- **Whether `Imported` needs to support default exports** (`import Moment from 'moment'`) in addition to named imports (`import { Moment } from 'moment'`). `import type` syntax forces named-only, which is probably fine — default exports in TS package surfaces have been discouraged for years.
- **Re-export coordination**: if the consumer's TS already has a `bindings/index.ts` that re-exports types, do imported renderings need to flow through? Probably the consumer's job, not ontogen-ts's.

## Notes

- Phase-1 ships with `ExternalTypeRendering::Primitive` as the only variant. OF-022 is *purely additive* — no breaking change to phase-1 consumers when it lands.
- Pairs naturally with OF-020 (hierarchical TS output): if a consumer is on the per-module-directory layout, imported renderings emit at the top of *each* generated file that uses them, deduplicated per-file. Implementation gets slightly more complex but the principle is unchanged.
