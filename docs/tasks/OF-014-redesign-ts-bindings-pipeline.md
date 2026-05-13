---
status: open
---
# OF-014 - Redesign the TypeScript bindings / type-generation pipeline

- **Severity:** Medium
- **Status:** Open
- **Source:** Spawned from [OF-006](./OF-006-ts-bindings-fallback-warning.md) on 2026-05-12. The OF-006 warning makes the fallback observable; OF-014 is the underlying design fix.

## Problem

Today the TS surface (`transport.ts` + the per-client variant) is generated
by ontogen, but the *type definitions* it imports come from a separate
`bindings.ts` file the user has to populate themselves with
`specta::export_ts` (or `ts-rs`). That separation is the root cause of the
silent-untyping foot­gun [OF-006](./OF-006-ts-bindings-fallback-warning.md)
patched over:

- The user has to set up a second binary that calls `specta::export_ts!`
  with every entity / DTO type listed by hand. Forgetting one entity is
  invisible until something at runtime throws because the call site is
  typed `Record<string, unknown>`.
- Adding a new entity is a multi-step ritual: write the entity, register
  it with specta, run the export binary, run the build, then check that
  the cargo warning list is empty. None of that is enforced.
- The fallback path has to exist in the TS emitter (`transport.rs:146-151`,
  `ts_client.rs:70-80`) precisely because we *can't* trust the contents of
  `bindings.ts`. The "fix" we shipped in OF-006 is a warning - it doesn't
  remove the failure mode, it just makes it loud.
- The Pumice integration (the source of OF-006) hit this on the very first
  entity. It's not a power-user pitfall.

## Location

The pieces involved span both ontogen and the consuming app:

- TS emitters that consult `bindings.ts`:
  - `src/servers/generators/transport.rs:74-181` (`generate`)
  - `src/servers/generators/ts_client.rs:22-211` (`generate`)
- Configuration surface:
  - `ClientGenerator::HttpTs { bindings_path }` and
    `ClientGenerator::HttpTauriIpcSplit { bindings_path }` in
    `src/servers/config.rs` (and re-exported from
    `src/servers/mod.rs:19-21`).
- Where bindings come from in real consumers: `examples/iron-log/...`'s
  `specta::export_ts` binary (the pattern Pumice copied), and the absent
  documentation about how to set that up - the OF-006 proposal originally
  asked for that doc, but writing it would lock in the workflow we want to
  replace.
- Type-collection helpers that drive what *should* appear in bindings:
  `src/servers/types.rs::collect_ts_import` and
  `src/servers/types.rs::collect_type_import` (the latter walks `syn::Type`
  for the Rust side; the former does string-based collection for TS).

## Current behavior

1. User configures `ClientGenerator::HttpTauriIpcSplit { output, bindings_path }`.
2. User stands up a separate `specta::export_ts` (or `ts-rs`) binary that
   knows about every entity / DTO they want exported.
3. Build runs. The TS emitter scans the *generated* TS and lists every
   type it references. For each referenced type it greps `bindings.ts` for
   `export type <Name>` / `export interface <Name>`.
4. Hits get added to an `import type { ... } from '<bindings>'`.
5. Misses get a placeholder + (post-OF-006) a cargo warning.

The user-visible failure modes:

- Missing `specta::export_ts!` registration → silent untyping (OF-006 turned
  this from silent to a warning).
- Bindings file written by a different generator (`ts-rs` vs `specta` vs
  hand-rolled) → identical user-facing brittleness because the contract is
  "export type X" string-grep.
- No story for transitive types: if entity `Workout` references `Exercise`,
  `Exercise` only appears in `bindings.ts` if the user listed it
  separately - the emitter doesn't tell specta what to export.

## Proposed direction (sketch, not a commitment)

Three plausible shapes; pick one in the design pass:

1. **Ontogen owns the bindings.** Drive `specta` (or our own ts emitter)
   from the same `schema_entities` / scanned-API metadata the rest of the
   pipeline already has. `bindings_path` becomes an *output*, not an
   *input*. No fallback, no warning - if a type is referenced and isn't
   in the schema, that's a hard error.

2. **Ontogen tells the user what to register.** Keep the user's
   `specta::export_ts` binary, but emit a Rust file (or a build-script
   diagnostic) that lists exactly the types ontogen expects to find in
   `bindings.ts`. The user's export binary `include!()`s it. Misses become
   compile errors, not runtime placeholders.

3. **Status quo + better docs.** Write the e2e guide the OF-006 proposal
   asked for. Cheapest, but doesn't address the underlying coupling - it
   just teaches the ritual.

Option 1 is the most ambitious and probably the right end state; option 2
is a stepping stone that doesn't require us to write a TS type emitter
ourselves.

## Effort

Large - this is a design discussion, not a one-PR fix. Expect:
- A short ADR / design doc covering the three options above.
- Prototype of whichever option wins, scoped to a single consumer
  (likely iron-log) before generalising.
- A migration plan for existing consumers (Pumice + iron-log) since any
  option that closes the fallback hole is a behaviour break for code that
  currently leans on `Record<string, unknown>`.

## Open questions

- Do we want ontogen to depend on `specta` directly, or stay generator-
  agnostic? (Affects option 1 vs option 2.)
- Is `bindings.ts` the only TS-side surface that needs this, or do we want
  the same treatment for any future client (Zod schemas, OpenAPI, ...)?
- Should the OF-006 warning be promoted to a hard error once OF-014 lands,
  or kept as a transitional belt-and-braces signal?

## Notes

- OF-006's warning text and `FallbackRecord` shape are deliberately
  user-facing now; whatever lands here should either remove the fallback
  entirely or repurpose the record into the new diagnostic.
- The OF-006 proposal's "documented e2e bindings path" half is *not*
  shipping under OF-006 because writing it would document the workflow
  this ticket wants to replace. If OF-014 stalls, reconsider — a stale
  guide is better than no guide.
