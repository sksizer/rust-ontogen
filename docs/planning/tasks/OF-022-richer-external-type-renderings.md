---
schema_version: '2'
status: planning/proposed
---
# OF-022 - Richer external-type renderings in `ontogen-ts` (imported TS types, not just primitives)

- **Severity:** Low. Speculative future work; phase-1 ontogen-ts ships with a primitive-rendering model that covers chrono / time / uuid / url / std-net / serde_json::Value, all rendered as TS `string` or `unknown`. OF-022 is the generalization to renderings that require *importing* a TS type from a package.
- **Status:** Open. Filed 2026-05-14 alongside the OF-015 design pass; not on the OF-015 critical path.
- **Related:** [OF-015](./OF-015-productionize-typescript-generation.md) phase-1 ships the primitive-only model; OF-022 generalizes it.

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
    Primitive(&'static str),

    /// Render as a type imported from a TS package.
    /// Emitter collects all `Imported` renderings, deduplicates by
    /// (module, name), and emits `import type { Name } from 'module'`
    /// at the top of bindings.ts. Field sites reference the local
    /// name (with the rename if provided).
    Imported {
        module: &'static str,       // 'moment', 'luxon', 'whatwg-url'
        name:   &'static str,       // 'Moment', 'DateTime', 'Url'
        local_name: Option<&'static str>, // import as 'ParsedUrl' instead
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
    external_types: HashMap::from([
        ("chrono::DateTime".into(), ExternalTypeRendering::Imported {
            module: "moment",
            name:   "Moment",
            local_name: None,
        }),
        ("url::Url".into(),         ExternalTypeRendering::Imported {
            module: "whatwg-url",
            name:   "Url",
            local_name: Some("ParsedUrl"),  // avoid collision with built-in Url
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
