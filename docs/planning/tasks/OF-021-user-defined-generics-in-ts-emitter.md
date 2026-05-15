---
status: open
---
# OF-021 - Support user-defined generic types in `ontogen-ts`

- **Severity:** Low. Speculative future work; only earns its keep if real consumers hit the phase-1 rejection often enough that the concrete-type-alias workaround stops being acceptable.
- **Status:** Open. Filed 2026-05-14 alongside the OF-015 design pass; not on the OF-015 critical path.
- **Related:** [OF-015](./OF-015-productionize-typescript-generation.md) (phase-1 ontogen-ts rejects user-defined generics with an `UnsupportedShape` error and points users at concrete type aliases or `#[ontogen::ts_opaque]`).

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
