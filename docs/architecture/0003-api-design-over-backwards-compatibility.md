# ADR 0003 — API design over backwards compatibility, until 1.0

## Status

**proposed** (2026-09-25).

Phase 1 of this decision self-expires at the 1.0 milestone; phase 2 takes
effect there. See *Notes* for the revisit trigger.

Number 0002 is reserved by
[T-66TG](../planning/tasks/T-66TG-adr-0002-extended-json-schema-front-end.md),
which owns the as-yet-unwritten extended-JSON-schema ADR. Per the numbering
convention in [`README.md`](README.md), an assigned number does not move.

## Context

Ontogen is pre-1.0 — `ontogen` 0.7.1 and `ontogen-core` 0.6.0 are on
crates.io — with five in-tree consumers (the four `examples/` build scripts
plus `crates/markdown-pilot`) and a small number of known downstream pin
sites.

Its public surface is unusually broad for a project this young. It spans, at
least:

- build-time Rust types consumers construct by hand in `build.rs` —
  `ServersConfig`, `ClientsConfig`, `ApiConfig`, `ApiSurface`
- IR types that cross the crate boundary — `EntityDef`, `FieldDef`, `EnumDef`
- an error enum — `CodegenError`
- the *shape of the Rust we generate* — store methods, API modules, handlers
- the *TypeScript we emit* — transport signatures, the admin registry
- the *wire protocol* those imply — HTTP routes, IPC command names, MCP tool
  names, response envelopes

Each of those is a thing someone can depend on, and so a thing that can be
broken.

Two forces have been pulling against each other, with no rule to arbitrate
between them.

**Semver tooling pushes toward preservation.** release-plz runs with
`semver_check = true`, so cargo-semver-checks gates every release. On
[#174](https://github.com/sksizer/rust-ontogen/pull/174) it flagged
`constructible_struct_adds_field` (for `EntityDef.doc` and `FieldDef.doc`)
and `enum_variant_added` (for `CodegenError::Docs`). Both are *additive*
changes that make the API better. Both register as breaking. Every such flag
creates quiet pressure to contort the design — put the field somewhere else,
avoid the variant, wrap the type — in order to keep the check green.

**Design review pushes toward re-cutting.** The pagination design review
(`design-declared-pagination.html`) found pagination declared in four config
structs, two of which can silently diverge, enforced by a signature contract
that exists only inside a parser. Five separate review findings across
[#159](https://github.com/sksizer/rust-ontogen/pull/159) and
[#166](https://github.com/sksizer/rust-ontogen/pull/166) were that unwritten
contract failing in a new way each time. Later exploration widened the
finding: `ServersConfig` and `ClientsConfig` duplicate not one field but
twelve — `api_dir`, `state_type`, `service_import_path`,
`types_import_path`, `state_import`, `naming`, `route_prefix`,
`sse_route_overrides`, `store_type`, `store_import`, `extra_surfaces` and
`pagination` — and both stages independently scan their own copy of
`api_dir`. Pagination is one instance of a bug class with twelve.

These two forces have never been weighed explicitly. In the absence of a
rule, the decision defaults to whichever option is locally cheaper — and
preservation is always locally cheaper, because it requires no work at all.
That default is how the four-struct divergence accreted. Nobody chose it. It
is the residue of never having chosen.

## Decision

Two phases, one rule each.

### 1. Until 1.0 — API design trumps backwards compatibility

When a cleaner surface requires a break, take the break. No deprecation
shims, no transitional aliases, no `foo2()` beside `foo()`, no compatibility
flags, no "keep the old path working for one more release." Bump the version
honestly and write the migration into the changelog.

The justification is arithmetic, and it is the design doc's own: with five
in-tree consumers and a handful of pin sites, the shim is more code than the
migration it defers — and unlike the migration, the shim is permanent.

### 2. From 1.0 — the ability to evolve the API is part of the API

Breaks become expensive. At that point a surface that cannot absorb a new
field, a new variant or a new mode without a major bump is not finished,
however clean it looks. Evolvability stops being a compatibility concern and
becomes a design criterion, judged in review alongside naming and coherence.

### The corollary that binds the two phases

**Installing an evolvability mechanism is itself a breaking change.**

`#[non_exhaustive]` is the clearest case. Per the Rust reference, a
non-exhaustive type "cannot be constructed with a StructExpression
(*including with functional update syntax*)" outside its defining crate. So
adding it breaks every struct literal in consumer code *and* every
`..base` functional update. It can only be added while breaks are free.

The two phases are therefore not merely sequential, they are causally
linked: **phase 2 is only affordable if phase 1 is spent installing the
hinges.** The pre-1.0 window is not just for getting the shape right. It is
for putting in the joints the shape will later need to bend at, while
bending is still free.

### The hinges

What this rule obliges us to install before 1.0:

| Hinge | What it lets us add later without a break |
|---|---|
| `#[non_exhaustive]` on a consumer-constructed struct | a field |
| `#[non_exhaustive]` on a consumer-matched enum | a variant |
| An open string discriminant instead of a boolean | a third state |
| A nested capability object instead of flat sibling flags | a capability |
| A reserved slot in an annotation grammar | a deferred parameter |
| A constructor or builder instead of a public struct literal | *required by rows 1–2* |

The last row is a consequence, not an independent choice: `#[non_exhaustive]`
without a constructor is a type nobody outside the crate can build at all.

Three of these six were already applied in the pagination design before this
ADR named them — `mode` as an open string so `cursor` is additive later, one
nested `list` capability object replacing four flat registry flags, and
`order_by`/`order` reserved in the attribute grammar before ordering is
implemented. That is the evidence the instinct is sound, and the reason to
promote it from instinct to rule.

### What this rule is not

**It is not licence to break casually.** A break still has to buy a better
API. Churn that improves nothing is still churn, and "we're pre-1.0" is not
an argument for it. The rule resolves a tie in favour of design; it does not
manufacture the tie.

**It is not "ignore semver."** `semver_check = true` stays on. The rule is
"do not *avoid* breaks," not "do not *detect* them." cargo-semver-checks
should keep flagging; its output drives the version bump rather than a
redesign. Expect frequent minor bumps pre-1.0 — under Cargo's pre-1.0 rules a
`0.x` minor bump *is* the breaking bump, and emitting it is the tool working
correctly, not a problem to route around.

**It is not blind to blast radius.** The rule covers the generated surface as
well as the build-time Rust API, but the two fail differently, and the
changelog must distinguish them:

- A **build-time break** — a changed config struct, a renamed IR field — is
  caught at compile time, by whoever runs the build, before anything ships.
- A **generated-protocol break** — a changed list envelope, a renamed route,
  a new MCP tool name — fails at *runtime*, for already-deployed clients that
  were never rebuilt.

Same freedom to break. Different consequence for getting the order wrong.
Protocol breaks are therefore called out in the changelog as deploy-ordering
hazards, naming which side must be deployed first. This is live immediately:
0.8.0 changes the list response envelope.

## Consequences

### Positive

- The pagination redesign can be taken as a clean cut, which is what the
  design review concluded it needed.
- Field and variant additions stop being design constraints. `EntityDef` can
  grow a field because the field belongs there, not after an argument about
  what cargo-semver-checks will say.
- The twelve-field config duplication becomes fixable rather than permanent.
- 1.0 ships with its hinges already in place, which is the only time they can
  be installed for free.

### Negative

- Consumers pinned to a `0.x` do real migration work at each minor. Accepted:
  five in-tree, a handful known downstream, and each migration is mechanical
  and documented.
- `#[non_exhaustive]` costs literal-construction ergonomics — no struct
  literals, no `..base`. This is mitigated by shipping good builders, **not**
  by skipping the attribute. If the builder is bad, fix the builder.
- More minor bumps, and a changelog that has to carry migration notes rather
  than just a feature list.
- The rule has an expiry date, which means it will at some point need to be
  consciously retired rather than quietly outgrown. Hence the revisit trigger
  below.

### Owed work

This decision creates an obligation: a **pre-1.0 evolvability sweep** — an
audit of every public type, asking "can this grow?", and installing the
hinge where the answer is no.

It is dated, because the window closes at 1.0. Its failure mode is not that
it becomes overdue and visible; it is that it *expires silently* and we
discover at 1.1 that a type is frozen. It must therefore be carried as
tracked work with the 1.0 milestone as its deadline — not left recorded only
here, where nothing will surface it in time.

## Alternatives considered

### A. Compatibility-first, even pre-1.0

Preserve the existing surface; add new capability alongside old. Rejected
because it is the status quo, and the status quo produced the four-struct
divergence, the unwritten signature contract, and five review findings that
were all the same bug wearing different clothes. The design review's own
arithmetic — the shim is more code than the migration it defers — settles it.

### B. Install hinges lazily, as each need arises

Wait until something actually needs to grow, then make it growable.
Rejected: installing a hinge is itself a break, so post-1.0 the need arises
at exactly the moment it can no longer be met. This is the alternative that
sounds most reasonable and fails most completely.

### C. `#[doc(hidden)] __non_exhaustive: ()` marker fields

The pre-attribute idiom for the same goal. Rejected: the real attribute
exists, cargo-semver-checks understands it, and the marker-field version
leaks an ugly private field into documentation and error messages while
offering strictly less.

### D. Drop `semver_check` from release-plz

If we are willing to break, why gate on break detection? Rejected, and worth
stating explicitly because it is the tempting misreading of this ADR. We
want breaks *detected and versioned*, not avoided. Removing the check would
lose the signal that drives the version bump and the changelog note — the
two things that make a break survivable for the people downstream.

## Notes

- **Revisit trigger: the 1.0 milestone.** Phase 1 self-expires there. At that
  point this ADR should be superseded by a new-numbered ADR recording the
  post-1.0 compatibility policy in full, rather than edited in place — per the
  supersession convention in [`README.md`](README.md).
- **Prerequisite reading for the sweep:** the hinge table above is the
  checklist.
- Related: `design-declared-pagination.html` (the design review that forced
  the question); backlog item `B-PNHJ`; ADR
  [0001](0001-markdown-as-store-backend.md), whose cross-backend semantic
  contract is itself an example of a surface that had to be got right before
  it had two implementations.
