---
type: task
schema_version: '1'
status: ready
created: 2026-05-19
last_reviewed: 2026-05-19
impact: medium
complexity: medium
tags: [ontogen-ts, ts-pipeline, docs]
related: [OF-015, OF-015-pr-7]
---
# OF-015 PR 8 — User-facing docs + residual sidecar cleanup

## Goal

Write the end-to-end TypeScript-bindings guide OF-006 originally asked for, now unblocked by ontogen-ts. Rewrite `client-generation.mdx`'s `bindings_path` section a third time to describe the ontogen-ts model (replacing the OF-019 spike-grade prose). Strip the now-obsolete "Integration gotchas" (`default-run`, `.taurignore`, CI env-gate idiom) from the docs site, cookbook, and README. Document the supported subset, the external-types table, and the `#[ontogen::ts_opaque]` / `#[ontogen::ts_name]` escape hatches. Also absorb the residual sidecar-cleanup items PR 6's `completion_note` flagged as partial — stale module comments and the `FallbackRecord` decision (delete vs. retain as a defensive backstop). Satisfies AC-16 of [OF-015](./OF-015-productionize-typescript-generation.md).

## Today

The site currently documents the side-car model:
- `site/src/content/docs/guides/client-generation.mdx` carries an OF-019-grade `bindings_path` section describing the side-car mechanism (specta side-car + side-car write), plus an "Integration gotchas" section with three subsections: `default-run`, `.taurignore`, and the CI env-gate idiom.
- `site/src/content/docs/cookbook/tauri-integration.mdx` carries a `.taurignore` step and a recipe `Cargo.toml` with `default-run = "iron-log"`.
- `README.md`'s "Known Issues" list has a third bullet summarizing the side-car (added by OF-019).
- There is no end-to-end TypeScript-bindings guide. OF-006 originally requested one but it was blocked behind the side-car model.

The codebase also carries residual sidecar references PR 6 didn't reach (PR 6's `completion_note` records the cleanup as partial):
- `src/servers/generators/ts_bindings.rs` lines 5 and 58 still describe the long-tail path as "handled separately by `ts_sidecar` via a generated specta binary" and "the specta side-car (option 3 half of the OF-014 hybrid)". The code no longer matches the prose — long-tail emission has moved to `ontogen-ts`.
- `src/servers/generators/mod.rs::FallbackRecord` is still defined and is still actively used by `src/servers/generators/ts_client.rs` and `src/servers/generators/transport.rs` (search for `FallbackRecord` to enumerate). `src/servers/mod.rs:332-340` drains the records and emits `cargo:warning` per occurrence. Per OF-015's hard-error decision the fallback path should be unreachable in the happy case (ontogen-ts is supposed to populate every type the transport references), but it is *not* obviously dead — the transport's "type not in `bindings.ts`" check is a defensive backstop against root-set / use-resolution misconfiguration. This needs a verify-before-delete pass, not a blind rip-out.
- `docs/walkthrough.md:555` has a `// Type = specta` comment on a code example that no longer reflects the derive macro the project uses post-cutover.

## Approach

Four commits inside the worktree (or one focused PR if grouping reads better in review):

1. **New `site/src/content/docs/guides/typescript-bindings.mdx`** — the end-to-end TS-bindings guide.
   - How ontogen-ts works (AST-based, build-time; not runtime; no side-car).
   - The supported subset:
     - Named structs.
     - C-style enums and tagged enums where the tag is implicit from variant idents.
     - Containers: `Vec<T>`, `Option<T>`, `HashMap<K, V>`, `BTreeMap<K, V>` (K must be `String` or id-like primitive).
     - Primitives: `bool`, all integer types, `f32`/`f64`, `String`, `&str`.
     - Smart-pointer transparency: `Box`, `Rc`, `Arc`, `Cow`, `Pin` peeled silently.
     - External-types table (defaults documented) + user override semantics.
   - Serde rename family (`rename`, `rename_all`, `skip`); split-rename rejected.
   - Escape hatches:
     - `#[ontogen::ts_opaque(target = "...")]` for opting a type out (user supplies the TS rendering).
     - `#[ontogen::ts_name = "..."]` for breaking name collisions without changing the JSON wire.
   - Error model: hard error on unsupported shapes; no fallback `Record<string, unknown>`.

2. **Rewrite `site/src/content/docs/guides/client-generation.mdx`**:
   - `bindings_path` section rewritten to reflect ontogen-ts (replaces the OF-019 prose).
   - "Integration gotchas" section removed entirely (`default-run`, `.taurignore`, CI env-gate are all side-car-only and no longer apply).
   - Cross-links added to the new typescript-bindings guide.

3. **Strip side-car-only content from `cookbook/tauri-integration.mdx`**:
   - Remove the `.taurignore` step (and the explanation paragraph).
   - Remove `default-run = "iron-log"` from the recipe Cargo.toml block.
   - Renumber subsequent steps if needed.

4. **Strip the README "Known Issues" bullet** added by OF-019:
   - `README.md` — remove the third bullet (the side-car summary).

5. **Residual sidecar cleanup** (carried over from PR 6 partial completion):
   - **Stale module comments in `src/servers/generators/ts_bindings.rs`** (lines 5 and 58). Rewrite the module header to describe what the head-of-stream emitter does today (schema-known types looked up against `bindings.ts`) without referencing `ts_sidecar` or "specta side-car (option 3 half of the OF-014 hybrid)". Pure prose; no behavior change.
   - **`docs/walkthrough.md:555`** — `// Type = specta` comment on the `CreateTaskInput` example. Update to reflect the derive the project actually uses now (verify by reading a current generated DTO).
   - **`FallbackRecord` decision** — choose one of two paths and execute it:
     - **(a) Delete.** If a pass over `src/servers/generators/ts_client.rs` + `transport.rs` + the `gen_servers` root-set logic confirms the fallback path is unreachable post-cutover (every type a transport references is always in `bindings.ts`), delete `FallbackRecord` from `src/servers/generators/mod.rs`, the `use` and emission paths in `ts_client.rs` and `transport.rs`, and the drain loops in `src/servers/mod.rs:332-340`. Update PR 6's AC-12/13 ticks.
     - **(b) Retain as defensive backstop.** If the verify pass surfaces a real "type not in `bindings.ts`" path (e.g., transport references types outside the configured root set; consumer hand-edits `bindings.ts`; stale-build races), keep `FallbackRecord` but rewrite its doc comment in `mod.rs` to describe its post-cutover role as a backstop, not the primary fallback emitter. Note the decision in OF-015's discovery log.
   - **Verify-before-delete commit ordering**: the verification pass commit comes first (a small commit that adds an `EmitError`-tightening test or a doc-only commit asserting unreachability); the delete commit follows. This keeps the "is it dead?" decision in the git history independent of the delete itself.

Each commit builds clean (`just full-check` covers Rust; verify any docs-site build command — e.g., `just site-build` or `npm run build` inside `site/` — still succeeds).

## Files to touch

- `site/src/content/docs/guides/typescript-bindings.mdx` (new) — end-to-end TS-bindings guide.
- `site/src/content/docs/guides/client-generation.mdx` (modify) — rewrite `bindings_path`, drop "Integration gotchas".
- `site/src/content/docs/cookbook/tauri-integration.mdx` (modify) — drop `.taurignore` + `default-run`; renumber steps.
- `README.md` (modify) — drop the OF-019 "Known Issues" bullet.
- `src/servers/generators/ts_bindings.rs` (modify) — rewrite stale module/fn header comments referencing `ts_sidecar` / "specta side-car".
- `docs/walkthrough.md` (modify) — fix `// Type = specta` comment on the `CreateTaskInput` example.
- `src/servers/generators/mod.rs` (modify or unchanged depending on FallbackRecord decision) — either delete `FallbackRecord` or rewrite its doc comment.
- `src/servers/generators/ts_client.rs` (modify or unchanged) — same conditional.
- `src/servers/generators/transport.rs` (modify or unchanged) — same conditional.
- `src/servers/mod.rs` (modify or unchanged) — drain-loop removal if FallbackRecord is deleted.

## Acceptance criteria

These are AC-16 from OF-015 — restated here for per-PR scope:

- [ ] AC-16.1: `site/src/content/docs/guides/typescript-bindings.mdx` exists; describes the ontogen-ts model end-to-end; covers supported subset, serde renames, external-types table, escape hatches, and the error model.
- [ ] AC-16.2: `site/src/content/docs/guides/client-generation.mdx`'s `bindings_path` section rewritten to reflect ontogen-ts; "Integration gotchas" section removed.
- [ ] AC-16.3: `site/src/content/docs/cookbook/tauri-integration.mdx`: `.taurignore` step removed; `default-run` removed from recipe `Cargo.toml`; subsequent steps renumbered.
- [ ] AC-16.4: `README.md`: third "Known Issues" bullet (OF-019 side-car summary) removed.
- [ ] AC-16.5: Supported subset documented (struct shapes, enum shapes, container handling, smart-pointer transparency, external-types table).
- [ ] AC-16.6: `#[ontogen::ts_opaque]` and `#[ontogen::ts_name]` attrs documented with code examples.
- [ ] AC-16.7 (umbrella per OF-015 AC-17): After this PR lands, `just full-check` passes on `main`; `cargo build` in `examples/iron-log/src-tauri/` succeeds; CI workflows pass.
- [ ] AC-16.8: `src/servers/generators/ts_bindings.rs` module comments no longer reference `ts_sidecar` or "specta side-car (option 3 half of the OF-014 hybrid)" — replaced with prose describing the post-cutover head-of-stream emitter.
- [ ] AC-16.9: `docs/walkthrough.md`'s `// Type = specta` comment is replaced with whichever derive the example actually uses today.
- [ ] AC-16.10: `FallbackRecord` decision recorded — either deleted (with PR 6's AC-12/13 ticks updated) or doc-comment-rewritten to describe its backstop role. The decision and its evidence (which call sites can/can't reach it) are committed; the verification commit precedes any delete commit.

## Out of scope

- **Phase-2 documentation** (shape-changing serde attrs, alternative output targets) — deferred to OF-015 phase 2 / spawned-as-needed.
- **Reworking the root-set derivation in `gen_servers`** to make the FallbackRecord path provably unreachable — if path (b) is taken in AC-16.10 (FallbackRecord retained as backstop), a follow-up ticket should capture any structural fix that would let it be deleted later.

## Dependencies

- [[OF-015-pr-7-pumice-validation]] ideally lands first so the docs can confidently describe "the supported subset covers iron-log and Pumice's full long-tail." If PR 7 is delayed by Pumice access (the human-only validation), PR 8 can ship on the strength of PR 5/6 alone and PR 7's reconciliation lands as a docs-update PR later.
