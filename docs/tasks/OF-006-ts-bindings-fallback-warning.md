---
status: draft
---
# OF-006 - Warn on TS bindings fallback to `Record<string, unknown>`

- **Severity:** Medium
- **Status:** Open
- **Source:** [feedback.md OF-006](2026-05-12-pumice.md)
- **Cross-ref:** Pumice task 0035, pumice issue ISS-002

## Problem

When an entity / DTO type cannot be resolved against the expected `bindings.ts` file, the TS bindings emitter falls back to `Record<string, unknown>` with a `TODO:` comment in `transport.ts`. The build does not fail; the TS surface is silently untyped. Result: a frontend that compiles cleanly but loses type safety on the affected calls.

## Location

- TS bindings emitter consumed by `bindings_path` (in `src/servers/generators/transport.rs` and/or `src/servers/generators/ts_client.rs` - confirm the exact fallback site during implementation).

## Current behavior

A function returning a type that's missing from `bindings.ts` emits:

```ts
// TODO: Type 'Workout' not yet exported from bindings.ts - using placeholder
type Workout = Record<string, unknown>
```

A real example exists today at `examples/iron-log/src-nuxt/app/generated/transport.ts:6-39`.

## Proposed resolution

Two parts:

1. **Build-time signal:** emit `cargo:warning=` for every fallback, e.g.:
   ```
   ontogen: type 'Workout' not found in bindings.ts - using `Record<string, unknown>` placeholder in transport.ts
   ```
   Same plumbing as [OF-001](./OF-001-parser-skip-diagnostic.md): collect fallbacks during generation, report from the pipeline.

2. **Documented e2e bindings path:** Today, getting bindings populated requires the user to set up a separate `specta::export_ts` binary. The path is doable but unobvious. Write a guide page (`site/src/content/docs/guides/typescript-bindings.mdx` or similar) walking the user from "I added an entity" to "the type appears in `bindings.ts`". Pumice task 0035 enumerates the concrete steps and can be cribbed.

## Effort

- Warning: Small (~20-40 LOC plus a generator test).
- Docs: Medium (one new guide page; needs verification against a real iron-log run).

## Notes

- Decide whether to escalate fallback to a hard error behind a config flag, similar to the OF-001 proposal. Most users will want a warning by default; CI-strict consumers may want the error toggle.
