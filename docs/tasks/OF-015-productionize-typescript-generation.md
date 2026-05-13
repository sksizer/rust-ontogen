---
status: open
---
# OF-015 - Productionize the TypeScript generation pipeline

- **Severity:** Medium-High. Closes a real silent-untyping foot­gun in production consumers; blocks retiring the OF-006 warning.
- **Status:** Open
- **Source:** Spawned from [OF-014](./OF-014-redesign-ts-bindings-pipeline.md) on 2026-05-13. OF-014 settled the design (option 1 + option 3 hybrid) and proved it end-to-end on iron-log via the spike on `worktree-of-014-spike-option-3`. OF-015 is everything that stands between the spike and a feature consumers can rely on.

## Problem

The spike works on iron-log but carries enough spike-grade shortcuts that
shipping it as-is would create new foot­guns and stale documentation. Real
consumers (Pumice, future projects) need:

- A surface that survives a `cargo update` and module reorganization
  inside their crate (the spike hardcodes single-module assumptions and
  fragile version pins).
- A clear cargo-friendly cost profile (the spike doubles cold-build time
  by recompiling deps in an isolated target dir).
- A clean source tree (the spike writes `src/bin/__ontogen_ts_export.rs`
  into the user's crate with no cleanup story).
- A documented opt-in path so consumers know what to do, and ontogen
  knows when *not* to do it (existing `ClientGenerator::HttpTs` /
  `HttpTauriIpcSplit` users on the old workflow shouldn't have their
  `bindings.ts` silently overwritten on upgrade).
- A coherent story for the OF-006 `FallbackRecord` warning: with the
  hybrid generating types automatically, a fallback should be impossible
  in the happy path - keep the warning as belt-and-braces, promote it to
  a hard error, or remove it entirely.

## Location

Spike code that this ticket inherits and productionizes:

- `src/servers/generators/ts_bindings.rs` (schema-known emitter +
  partition helpers)
- `src/servers/generators/ts_sidecar.rs` (side-car source gen + cargo
  orchestration + bindings.ts append)
- `src/servers/mod.rs::generate_transport` (wiring + env guard)
- `src/servers/mod.rs::sidecar_lib_crate_name`,
  `sidecar_types_module_path` (helpers)

Config surface that needs decisions:

- `src/servers/config.rs::ClientGenerator` - new variant or strategy
  field?
- `src/servers/config.rs::ServersConfig` - knobs for BigInt behavior,
  side-car opt-in/out, target-dir override.

User-facing docs that need to land:

- `site/src/content/docs/guides/typescript-bindings.mdx` (new) - the
  e2e bindings guide OF-006's proposal originally asked for, now
  unblocked because the workflow no longer requires a manually-curated
  side-car.
- `site/src/content/docs/guides/client-generation.mdx` - update
  `bindings_path` section to reflect the input → output flip.
- `site/src/content/docs/reference/configuration.mdx` - document any
  new `ServersConfig` knobs.

## Scope

In:

1. **Close every shortcut in the OF-014 spike punch-list.** See
   [OF-014 § Known spike-grade shortcuts](./OF-014-redesign-ts-bindings-pipeline.md#known-spike-grade-shortcuts-productionization-punch-list)
   for the full list. The load-bearing ones:
   - Track the source module of every long-tail type so they can live
     anywhere in the user's crate, not just under
     `types_import_path`. (Today the spike hardcodes
     `strip_prefix("crate::")` over `types_import_path` and assumes all
     long-tail types live there.)
   - Detect already-optional fields in Update DTOs to avoid `T | null | null`.
   - Plumb BigInt behavior through `ServersConfig` instead of hardcoding
     `Number`.
   - Guard the entire `generate_transport` (or `gen_servers`) call when
     `ONTOGEN_TS_SIDECAR_INNER` is set, not just the side-car block.
   - Emit `cargo:rerun-if-changed` for long-tail type source files so
     edits trigger rebuilds.
   - Clean up `src/bin/__ontogen_ts_export.rs` when long-tail becomes
     empty, OR move side-car generation to `OUT_DIR` + a manifest-time
     stub so nothing lands in the user's source tree.

2. **Decide the opt-in/opt-out surface.** Existing
   `ClientGenerator::HttpTs` / `HttpTauriIpcSplit` users today treat
   `bindings_path` as an input. The spike silently makes it an *output*.
   That's a behavior break. Pick one:
   - New strategy field (`bindings_strategy: BindingsStrategy`) defaulting
     to `Manual` (current behavior) with `Specta` opt-in.
   - New `ClientGenerator` variant (`SpectaManaged { bindings_path, ... }`)
     and deprecate the bindings_path on existing variants.
   - Auto-detect: if `schema_entities` non-empty and `specta-typescript`
     available, take over. Magic; probably wrong.

3. **Decide the OF-006 `FallbackRecord` warning's fate.** Options:
   - Keep as belt-and-braces (warn if anything still slips through).
   - Promote to a hard error (per OF-006 proposal's `strict` flag).
   - Remove entirely (the new pipeline shouldn't produce fallbacks; if
     it does that's a bug, not a user error).

4. **Ship user-facing docs.** A new `guides/typescript-bindings.mdx`
   walking through "I added a type → it appears in bindings.ts," plus
   updates to `client-generation.mdx` and `configuration.mdx` reflecting
   the new flow. OF-006's original proposal asked for this; it was
   deferred under OF-014 because writing it would have ossified the
   manual workflow we replaced.

5. **Cargo dependency story.** Today consumers add `specta-typescript`
   themselves. Pick one:
   - Document it in the guide (current spike behavior).
   - Surface a clear cargo:warning if it's missing.
   - Vendor a minimal TS emitter so consumers don't need the dep.

Out (separate tickets if they arise):

- Other output targets (Zod schemas, OpenAPI, JSON Schema). OF-014 open
  question 2 raised these; they need separate design once OF-015 lands.
- Replacing specta with a hand-rolled emitter. The cost analysis in
  OF-014 made specta the right pick for the long tail; revisit only if
  specta itself becomes a problem.

## Effort

Medium-Large. Most of it is judgment calls (the three "decide" items
above), not algorithmic work. The algorithmic shortcuts in the punch-list
are individually small (each is a few-hour task); the integration tests
that show option 3 working on Pumice in addition to iron-log are
probably the highest-leverage validation step.

## Open questions

- Should consumers be able to opt *into* hybrid emission without
  changing `ClientGenerator`? E.g., a project that uses ts-rs today
  might want option 1 (schema-known emission) without option 3
  (specta side-car) - the partition between the two is a clean cut
  if we expose it.
- Should the side-car binary live in the user's `src/bin/` (today's
  spike) or somewhere ontogen owns end-to-end (e.g., a temp dir under
  `OUT_DIR`)? The latter is cleaner for git but harder for
  troubleshooting since the source disappears between builds.
- Is `cargo metadata` an acceptable build-time cost for resolving the
  lib crate name and target dir, replacing the spike's manual
  `Cargo.toml` parser?

## Notes

- The spike is on branch `worktree-of-014-spike-option-3`. Productionization
  should start from that branch (or a fresh worktree off it) rather than
  re-implementing from scratch.
- OF-014's "Trying this on your project" section will eventually become
  the user-facing guide (item 4 in Scope above), with the spike-grade
  caveats removed.
