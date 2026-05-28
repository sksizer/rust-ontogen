---
schema_version: '3'
status: planning/proposed
impact: low
complexity: large
last_reviewed: '2026-05-26'
low_confidence: true
definition_gap: |
  Three unresolved design decisions block promotion to open/ready: (1)
  name-mangler spec for nested generics — terminal-ident-only with an
  `Of` separator (`PaginatedHashMapOfStringAndVecOfWorkout`), or
  full-path encoding, or hash-suffix for stability; the spec must not
  silently produce collisions. (2) Bound-handling policy for strategy
  B (`#[ontogen::ts_generic]`) — drop bounds silently (mirrors the
  silent-untyping foot-gun OF-006 was trying to fix) vs. error on
  bounded generics and force the user to monomorphize; current lean is
  error. (3) Whether strategy A (monomorphize) and strategy B (TS
  generic) may coexist on the same Rust type — i.e. can a user have
  `#[ontogen::ts_generic] struct Paginated<T>` AND
  `pub type PaginatedWorkouts = Paginated<Workout>` that emits as a
  concrete alias alongside the generic. Probable answer is yes (the
  alias hides the generic), but the resolution rule needs to be
  written down. Also still speculative: Pumice empirical question in
  Notes — until a real consumer demonstrates the concrete-type-alias
  workaround from OF-015 is no longer acceptable, this stays parked.
---
# OF-021 - Support user-defined generic types in `ontogen-ts`

- **Severity:** Low. Speculative future work; only earns its keep if real consumers hit the phase-1 rejection often enough that the concrete-type-alias workaround stops being acceptable.
- **Status:** Open. Filed 2026-05-14 alongside the OF-015 design pass; not on the OF-015 critical path.
- **Related:** [OF-015](./OF-015-productionize-typescript-generation.md) (phase-1 ontogen-ts rejects user-defined generics with an `UnsupportedShape` error and points users at concrete type aliases or `#[ontogen::ts_opaque]`).

## Goal

Lift ontogen-ts from "user-defined generics are rejected" to first-class support, defaulting to monomorphization (one concrete TS type per reachable instantiation) and offering an opt-in `#[ontogen::ts_generic]` attribute that preserves the generic in the emitted TS surface.

## Today

| Location | Role today |
|---|---|
| `crates/ontogen-ts/src/emit.rs` (`emit_type`, `match_container`, `Container`) | Hardcodes the supported container generics — `Option<T>`, `Vec<T>`, `HashMap<K, V>` / `BTreeMap<K, V>`, `HashSet<T>` / `BTreeSet<T>` — via the `Container` enum and the `match_container` helper. Anything else with a `PathArguments::AngleBracketed` arg list falls through and is eventually rejected. |
| `crates/ontogen-ts/src/emit.rs` (lines ~248, ~261, ~291) | The `UnsupportedShape { type_path, reason }` rejection sites. A `Paginated<T>` referenced from a root reaches the fall-through and surfaces as `UnsupportedShape`. |
| `crates/ontogen-ts/src/types.rs` (`EmitError::UnsupportedShape`, `EmitConfig`) | Defines the rejection variant the emitter raises today and the config surface (`external_types`, `bigint_behavior`, `case_default`) that would need an opt-in knob if we go config-driven instead of attribute-driven for strategy B. |
| `crates/ontogen-ts/src/pool.rs` (`scan_src_dir_with_imports`, `collect_items`) | Collects `ItemStruct` / `ItemEnum` / `ItemType` into the type pool keyed by canonical `TypePath`. Today it stores the raw `syn::Item` including `Generics`; nothing downstream consumes that yet because every generic is rejected before emission. |
| `crates/ontogen-ts/src/order.rs` (`DepCollector::visit_type_path`) | Already recurses into generic args via `syn::visit::visit_type_path`, so `Paginated<Workout>` records a `Workout` dep edge — but no edge is recorded for the *instantiation* itself, and no notion of "which concrete arg tuples reach this generic" is collected. |
| `crates/ontogen-ts/src/attr.rs` (`OntogenAttrs`, `parse_ontogen_attrs`) | The attribute parser that surfaces `#[ts_opaque]` / `#[ts_name]` from a `syn::Item`'s attributes. The natural extension point for a third attribute `#[ts_generic]`. |
| `crates/ontogen-macros/src/lib.rs` (`ts_opaque`, `ts_name`) | Defines the proc-macro attributes that ontogen-ts reads. New `#[ontogen::ts_generic]` (no-op at Rust compile time, validated at parse, consumed by ontogen-ts) lives here. |
| `src/clients/generators/transport.rs`, `src/clients/generators/ts_client.rs` | Emit the TS client/transport surface at API call sites. Today they reference types by their emitted TS name; if strategy B is enabled, they need to emit `Paginated<Workout>` syntax (with substituted args) rather than the mangled `PaginatedWorkout` name. |
| `site/src/content/docs/guides/typescript-bindings.mdx` | Current TS bindings guide; documents the supported subset and the `#[ontogen::ts_opaque]` / `#[ontogen::ts_name]` escape hatches. The natural home for the new strategy A vs. strategy B explainer. |

## Proposed

ontogen-ts walks each reachable user-defined generic instantiation and emits a concrete TS alias per instantiation (strategy A, default). Types annotated `#[ontogen::ts_generic]` instead emit once as a TS generic (`export type Paginated<T> = { ... }`) and downstream emitters substitute generic args at use sites (strategy B). Bounded generics under strategy B follow the resolved bound-handling policy. The concrete-type-alias workaround documented in OF-015 remains a valid escape hatch.

## Approach

1. **Resolve open design decisions first.** This step is non-negotiable and gates every step below. Land three decisions in writing (in this task body, then in the docs guide):
   - **Name-mangler spec for strategy A.** Pick one of: terminal-ident-only with `Of`/`And` separators (`PaginatedHashMapOfStringAndVecOfWorkout`); full-path encoding; or a deterministic short-hash suffix (`Paginated_abc123`). Acceptance condition: the rule produces collision-free names for any two distinct concrete-arg tuples reachable from the same generic, and is stable across emit runs.
   - **Bound-handling policy for strategy B.** Pick one of: silently drop bounds, document the loss; or error on bounded generics and force the user to monomorphize. Current lean is error (matches OF-006's silent-untyping aversion).
   - **Coexistence of strategy A and strategy B on the same type.** Pick: yes, concrete type aliases over a `#[ts_generic]` type emit as non-generic concrete aliases (the alias hides the generic); or no, mark the conjunction as an `UnsupportedShape`. Probable answer: yes, with the alias winning.
2. **Add `#[ontogen::ts_generic]` to `ontogen-macros`.** No-op at Rust compile time; argument shape is the bare form `#[ontogen::ts_generic]` (no args). Mirror the parse-and-validate pattern from `ts_opaque` / `ts_name`.
3. **Extend `attr.rs` to surface the new attribute.** Add a `ts_generic: bool` field on `OntogenAttrs`; parse it alongside the existing `ts_opaque` / `ts_name` recognition.
4. **Build an instantiation collector.** During the `emit` composition step (top of `emit.rs`), walk every reachable `Type::Path` whose terminal ident resolves to a pool entry with non-empty `Generics::params`, and record the `(generic_path, [concrete_arg_types])` tuple. The collector dedups identical tuples and tracks the originating reference for diagnostics.
5. **Implement strategy A (monomorphization), default path.** For each `(generic, args)` tuple where the generic does NOT carry `#[ts_generic]`: clone the generic's `syn::Item`, substitute the type-param idents with the concrete arg `Type`s, run the existing struct/enum emitter, and emit under the mangled name from step 1.
6. **Implement strategy B (TS-generic emission), opt-in path.** For each generic that DOES carry `#[ts_generic]`: emit once as `export type Paginated<T> = { ... }`, mapping each Rust type-param ident to a TS type-param ident verbatim. Apply the bound-handling policy from step 1.
7. **Wire downstream emitters.** Update `src/clients/generators/transport.rs` and `src/clients/generators/ts_client.rs` so that an API-call site referencing `Paginated<Workout>` where `Paginated` is `#[ts_generic]` emits as `Paginated<Workout>` in TS; where it isn't, emit the mangled name.
8. **Lift the `UnsupportedShape` gate.** The rejection sites in `emit.rs` (~lines 248/261/291) need to stop firing for user-defined-generic instantiations now that we have an emission path. Runtime-coordination wrappers (`RefCell`, `Mutex`, `RwLock`) and other genuinely-unsupported shapes still error.
9. **Test fixtures.** Cover: simple `Paginated<Workout>` monomorphization; multiple instantiations of the same generic (`Paginated<Workout>` + `Paginated<Tag>`); nested generics for the name-mangler decision (`Paginated<HashMap<String, Vec<Workout>>>`); `#[ts_generic]` opt-in single-emission; bound-handling per the resolved policy; concrete alias over a `#[ts_generic]` type per the coexistence decision.
10. **Update the TS bindings guide.** Rewrite `site/src/content/docs/guides/typescript-bindings.mdx` with the two-strategy explainer, the per-type opt-in attribute, and a side-by-side example showing the same Rust generic emitted both ways.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `crates/ontogen-macros/src/lib.rs` | modify | Add `#[proc_macro_attribute] pub fn ts_generic` mirroring `ts_opaque` / `ts_name`; no-op at Rust compile time, validates that the attribute takes no arguments. |
| `crates/ontogen-ts/src/attr.rs` | modify | Extend `OntogenAttrs` with `ts_generic: bool`; recognise the attribute name in `parse_ontogen_attrs` alongside `ts_opaque` / `ts_name`. |
| `crates/ontogen-ts/src/emit.rs` | modify | (a) build the instantiation collector during the top-level `emit` walk; (b) add the name-mangler implementing the step-1 spec; (c) add the type-param substitution helper; (d) extend `match_container` / the path-resolution branch to recognise pool-resident generics and dispatch to monomorphization vs. TS-generic emission; (e) loosen the `UnsupportedShape` gates for cases now handled. |
| `crates/ontogen-ts/src/types.rs` | modify | Likely extend `EmitConfig` with strategy-B knobs (e.g. bound-handling policy default, or a `monomorphize_default: bool` override) only if the resolved-decisions step 1 calls for it. Otherwise touch only diagnostics on `EmitError`. |
| `crates/ontogen-ts/src/order.rs` | modify | Extend `DepCollector` so it records edges for the generic-instantiation tuples the collector in `emit.rs` consumes — today's recursion records the arg-type dep but not the instantiation. Detail depends on whether the instantiation set is computed inside `order` or `emit`. |
| `crates/ontogen-ts/tests/` | new | Fixture suite covering the cases enumerated in Approach step 9. Add a directory of new fixture files; don't displace existing fixtures. |
| `src/clients/generators/transport.rs` | modify | At call-site emission, branch on `#[ts_generic]`: emit `Paginated<Workout>` for strategy B, mangled name for strategy A. |
| `src/clients/generators/ts_client.rs` | modify | Same branch as transport — emit generic-instantiation syntax at use sites for strategy-B types. |
| `site/src/content/docs/guides/typescript-bindings.mdx` | modify | Document the two strategies, the per-type opt-in attribute, side-by-side example, and the readability vs. flexibility tradeoff. |

## Acceptance criteria

- [ ] AC-1: `EmitConfig` accepts user-defined generic types reachable from a root without raising `EmitError::UnsupportedShape` for the generic itself. (Runtime-coordination wrappers and other genuinely-unsupported shapes still error.)
- [ ] AC-2: With no opt-in attribute, a generic `pub struct Paginated<T> { items: Vec<T>, total: u64 }` instantiated as `Paginated<Workout>` and `Paginated<Tag>` emits two concrete TS aliases under the name-mangler spec resolved in Approach step 1 (e.g. `export type PaginatedWorkout = { ... }` and `export type PaginatedTag = { ... }`).
- [ ] AC-3: With `#[ontogen::ts_generic]` on `Paginated<T>`, ontogen-ts emits exactly one TS declaration `export type Paginated<T> = { items: T[]; total: number };` regardless of how many times the type is instantiated, and downstream emitters (`transport.rs` / `ts_client.rs`) emit `Paginated<Workout>` syntax at use sites.
- [ ] AC-4: The bound-handling policy resolved in Approach step 1 is exercised by a fixture: a bounded generic `pub struct Wrapper<T: Display>` annotated `#[ontogen::ts_generic]` either silently emits with the bound dropped (policy A) or surfaces a typed `EmitError` (policy B); whichever policy ships, the fixture pins it.
- [ ] AC-5: User-facing documentation in `site/src/content/docs/guides/typescript-bindings.mdx` covers both strategies with the same Rust → TS example shown both ways, and documents the per-type opt-in attribute.
- [ ] AC-6: `just full-check` passes on the rust-ontogen branch.

## Out of scope

- **Default-type-param support** (`Cache<'a, K, V = String>`). Needs separate thinking about a TS analogue; defer to a follow-up.
- **Lifetime-parameterized generics** (`Holder<'a, T>`). The wire never carries Rust lifetimes; ontogen-ts strips them before emission. Proving correctness here is non-trivial and out of phase.
- **HKT or trait-object generic args** (`Box<dyn Trait>` inside a generic). Out of any phase. Document as a known limitation in the guide.
- **`T` as a bare type parameter in `external_types`** (i.e. "any `T` here renders as `unknown`"). Document as rejection-by-design in the guide.

## Dependencies

- [OF-015](./OF-015-productionize-typescript-generation.md) — closed/done, shipped 2026-05-20. This task assumes the OF-015 phase-1 baseline (build-time AST emitter, attribute parser, container handling) is in place.

## Discovery context

Filed 2026-05-14 alongside the OF-015 design pass as a deliberately-deferred follow-up: OF-015 chose to ship phase-1 with user-defined generics rejected (`UnsupportedShape` + concrete-type-alias workaround) rather than block the larger TS-pipeline lift on generic support. The acceptability of that workaround is empirical — Pumice's API surface today has a small number of generic instantiations, and the alias workaround is tolerable at that scale. This task earns priority when a real consumer's instantiation count makes the alias surface painful to maintain, or when the generic-as-semantic-surface argument outweighs the simplicity argument. Until then, it stays parked at `planning/proposed`.

## Problem

Phase-1 ontogen-ts only handles a hardcoded set of container generics — `Option<T>`, `Vec<T>`, `HashMap<K, V>`, `BTreeMap<K, V>` — because each has a known TS rendering the emitter can hardcode (`T | null`, `T[]`, etc.). User-defined generic types like `pub struct Paginated<T> { items: Vec<T>, total: u64 }` are rejected with `UnsupportedShape`.

The phase-1 escape hatch (assuming the type-alias resolution decision goes "follow + inline" — see [OF-015](./OF-015-productionize-typescript-generation.md) open questions) is concrete type aliases:

```rust
pub type PaginatedWorkouts = Paginated<Workout>;
```

ontogen-ts sees `PaginatedWorkouts` as a non-generic alias resolving to a concrete `Paginated<Workout>` shape, follows the alias, and emits `export type PaginatedWorkouts = { items: Workout[]; total: number }`. The user writes one alias per instantiation; emission Just Works.

That workaround is acceptable for small instantiation counts. It breaks down when:

- A consumer has many generic types each instantiated several ways (`Paginated<Workout>`, `Paginated<Tag>`, `Paginated<Exercise>`, `Paginated<Session>`, ...).
- The generic is part of an API surface the consumer wants to evolve without manually maintaining a parallel alias surface.
- The generic carries semantic meaning the consumer wants reflected in TS (`Paginated<T>` as a *generic* TS type is more communicative than `PaginatedWorkouts` / `PaginatedTags` / ...).

OF-021 is the lift from "rejected with workaround" to "first-class support."

## Direction (sketch, not commitment)

Two materially different strategies, each with sub-decisions:

### Strategy A: Monomorphize at emit time

Walk every generic instantiation reachable from a root. Emit a concrete TS type per instantiation, name-mangled from the generic and its type args.

```ts
// Rust: pub struct Paginated<T> { items: Vec<T>, total: u64 }
// Roots reach: Paginated<Workout>, Paginated<Tag>
//
// Emitted TS:
export type PaginatedWorkout = { items: Workout[]; total: number };
export type PaginatedTag     = { items: Tag[]; total: number };
```

Pros:
- TS output is fully concrete; consumers don't deal with TS generics.
- Trivial to type-check on the TS side (no generic-bound surprises).
- Works for any Rust generic shape, regardless of bounds or defaults — they're erased at monomorphization.

Cons:
- Output size scales with instantiation count, not type count. `Paginated<T>` used 8 ways = 8 type definitions.
- Name mangling is fragile (`Paginated<HashMap<String, Vec<Workout>>>` produces ugly names; needs an opinionated mangler).
- The TS surface no longer mirrors the Rust surface (Rust has `Paginated<T>`; TS has `PaginatedWorkout` / `PaginatedTag` / ...). Mental-model drift.

### Strategy B: Emit as TS generics

Emit `Paginated<T>` once as a TS generic type; consumers instantiate at use sites in their TS code.

```ts
// Rust: pub struct Paginated<T> { items: Vec<T>, total: u64 }
//
// Emitted TS (once):
export type Paginated<T> = { items: T[]; total: number };

// Used at API call sites (already-generated transport.ts):
function listWorkoutsPaginated(): Promise<Paginated<Workout>> { ... }
```

Pros:
- One emitted definition per Rust generic type, regardless of instantiation count.
- TS surface mirrors Rust surface — semantic communication preserved.
- Smaller output, cleaner mental model.

Cons:
- TS generics don't have Rust's trait bounds. Bounded generics either lose information silently (`pub struct Wrapper<T: Display>` → `type Wrapper<T> = ...` with `T` unconstrained in TS) or error.
- Some Rust types don't have a clean TS-generic analogue (e.g., `T` constrained by a complex bound, lifetimes, defaults with specific types).
- TS-side instantiation has to be sound — if a Rust generic only ever holds types from a fixed allowlist, TS's open type parameter doesn't capture that.
- Requires the downstream emitters (`transport.ts`, `ts_client.ts`) to know about generic argument substitution at API-call sites.

### Probable answer: hybrid, with the user opting in

Default = strategy A (monomorphize) — it's simpler and produces correct TS without bound/lifetime gotchas. Provide a knob (`#[ontogen::ts_generic]` attr or per-type config) to opt a type into strategy B if the user wants the generic preserved. This avoids dictating one mental model for all consumers.

## Location

- `ontogen-ts` (assumes OF-015 has shipped):
  - Type collection: extend to detect user-defined generics in the type pool.
  - Emitter: implement monomorphization (strategy A) as default; optional strategy-B emission for types tagged with the opt-in attr.
  - Name mangler for monomorphized instantiations.
- `ontogen-macros`:
  - New `#[ontogen::ts_generic]` attribute (opts a type into strategy B).
- `src/servers/generators/transport.rs` + `ts_client.rs`:
  - If strategy B is on, emit `Paginated<Workout>` syntax at use sites instead of the mangled name.
- `site/...`:
  - Document the two strategies, when to pick which, and the per-type opt-in attr.

## Scope

In:

1. **Strategy A (monomorphization) as default.** Walk reachable generic instantiations, emit one TS type per concrete instantiation, name-mangled from the base name + type args.

2. **Strategy B (generic TS) as opt-in** via `#[ontogen::ts_generic]`. Emit one generic TS type; downstream emitters instantiate at use sites.

3. **Name mangler spec.** `Paginated<Workout>` → `PaginatedWorkout`. `HashMap<String, Vec<Workout>>` → ... pick a rule. Probably terminal-ident-only with separator + `Of`, or full-path encoding. Decision-driver: what produces readable names for the common cases without being ambiguous on edge cases.

4. **Bound handling for strategy B.** Either silently drop bounds (TS has no equivalent, document the limitation) or error on bounded generics (force the user to monomorphize). Lean error — silent information loss is exactly the silent-untyping foot-gun OF-006 was trying to fix.

5. **Documentation.** A guide page covering both strategies with the same Rust → TS example shown both ways; user picks based on the readability vs. flexibility tradeoff.

Out:

- **Default-type-param support** (`Cache<'a, K, V = String>`). Probably also defer — needs separate thinking about TS analogue.
- **Lifetime-parameterized generics** (`Holder<'a, T>`). The wire never carries Rust lifetimes; ontogen-ts should strip them before emission, but proving correctness here is non-trivial.
- **HKT or trait-object generic args** (`Box<dyn Trait>` inside a generic). Out of any phase. Document as a known limitation.

## Effort

Medium-large. Strategy A alone is maybe 2 dev days (collection + monomorphization + mangler + tests). Strategy B is comparable but with the additional downstream-emitter integration. Both together with the opt-in attribute + docs is probably 4-5 dev days. Bound handling decisions add another ½-1 day depending on which way we go.

## Open questions

- **Name mangler spec for nested generics.** `Paginated<HashMap<String, Vec<Workout>>>` — what should this name? `PaginatedHashMapOfStringAndVecOfWorkout`? Encode the generic args' arity? Hash them for stability? Real consumers might never have nesting this deep, but the spec should not silently produce collisions.
- **Whether strategy A and strategy B can coexist on the same type.** If a user has `Paginated<T>` with `#[ontogen::ts_generic]`, can they *also* have `pub type PaginatedWorkouts = Paginated<Workout>` that emits as a non-generic concrete alias? Probably yes — the alias hides the generic.
- **Whether to support `T` as a bare type parameter in `external_types`.** I.e., letting a user say "any `T` here renders as `unknown` in TS" — unlikely useful but worth a sentence on rejection.

## Notes

- Per-Pumice empirical question: how many user-defined generics show up in Pumice's API surface today, and how are they instantiated? If the count is small (<5 distinct generics, <15 total instantiations), the concrete-type-alias workaround from phase 1 is probably acceptable indefinitely and this ticket stays open as planning material. If higher, OF-021 earns priority.
- Lifetimes are out of scope. Lifetime-parameterized wire types are vanishingly rare; the ontogen pipeline already enforces `'static` boundaries in most contexts. Ontogen-ts should strip lifetimes from generic-arg lists silently or error if encountered.
