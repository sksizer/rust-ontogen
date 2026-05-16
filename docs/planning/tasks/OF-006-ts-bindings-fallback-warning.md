---
status: closed/done
completion_note: "Shipped in 8bed7f7 on 2026-05-12."
---
# OF-006 - Warn on TS bindings fallback to `Record<string, unknown>`

- **Severity:** Medium
- **Status:** Resolved (`8bed7f7`, 2026-05-12). Build-time warning shipped; e2e bindings docs deferred to [OF-014](./OF-014-redesign-ts-bindings-pipeline.md).
- **Source:** [feedback.md OF-006](2026-05-12-pumice.md)
- **Cross-ref:** Pumice task 0035, pumice issue ISS-002

## Resolution

Shipped on 2026-05-12. The build-time signal is implemented; the documented
e2e bindings path was promoted to a follow-up ticket
([OF-014](./OF-014-redesign-ts-bindings-pipeline.md)) since the existing
`specta::export_ts` flow is the wider design problem the reporter actually
flagged.

What landed:

- New `pub struct FallbackRecord { output, bindings_path, type_name }` in
  `src/servers/generators/mod.rs`. The `Display` impl is the
  `cargo:warning=` body, e.g.
  `ontogen: type 'Workout' not found in `.../bindings.ts` - using `Record<string, unknown>` placeholder in `.../transport.ts``.
- `src/servers/generators/transport.rs:74` (`generate`) and
  `src/servers/generators/ts_client.rs:22` (`generate`) now return
  `Vec<FallbackRecord>` - one record per type that wasn't found in
  `bindings.ts` and got stubbed as `Record<string, unknown>`. The TS
  placeholder is still emitted so the file compiles; the record just makes
  the silent untyping observable.
- `src/servers/mod.rs:243-256` (`generate_transport`) drains both generators'
  fallback lists and `println!("cargo:warning={record}")` per occurrence -
  same pattern as OF-001's `SkipRecord` plumbing.
- 4 new unit tests in `src/servers/tests.rs` cover: (a) every CRUD reference
  type produces a `FallbackRecord` when bindings is empty, (b) no records
  when bindings exports every referenced type, (c) the equivalent for
  `ts_client::generate`, (d) the `Display` shape that becomes the
  user-visible warning text.
- `site/src/content/docs/guides/client-generation.mdx` "The bindings_path
  option" section now documents the warning text and tells users the fix
  is to export the missing type from `bindings.ts`.

Deferred to [OF-014](./OF-014-redesign-ts-bindings-pipeline.md):

- The new "I added an entity → the type appears in `bindings.ts`" guide
  page from the original proposal. The honest cause is that the current
  `specta::export_ts` setup is a separate-binary side car the user has to
  wire up themselves; documenting it without redesigning it would
  ossify a workflow we want to replace.
- The optional `strict` flag that escalates fallbacks to a hard error.

The change is not a public-API break: both `generate` functions are
crate-internal (`pub(crate) mod generators;` in `src/servers/mod.rs:12`).
External callers see a behaviour change (warnings now appear) but no
surface change.

---

*The remainder of this document is preserved as a record of the original analysis.*

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
