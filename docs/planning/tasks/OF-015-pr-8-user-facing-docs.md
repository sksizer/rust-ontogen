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
# OF-015 PR 8 — User-facing docs (typescript-bindings guide + cleanups)

## Goal

Write the end-to-end TypeScript-bindings guide OF-006 originally asked for, now unblocked by ontogen-ts. Rewrite `client-generation.mdx`'s `bindings_path` section a third time to describe the ontogen-ts model (replacing the OF-019 spike-grade prose). Strip the now-obsolete "Integration gotchas" (`default-run`, `.taurignore`, CI env-gate idiom) from the docs site, cookbook, and README. Document the supported subset, the external-types table, and the `#[ontogen::ts_opaque]` / `#[ontogen::ts_name]` escape hatches. Satisfies AC-16 of [OF-015](./OF-015-productionize-typescript-generation.md).

## Today

The site currently documents the side-car model:
- `site/src/content/docs/guides/client-generation.mdx` carries an OF-019-grade `bindings_path` section describing the side-car mechanism (specta side-car + side-car write), plus an "Integration gotchas" section with three subsections: `default-run`, `.taurignore`, and the CI env-gate idiom.
- `site/src/content/docs/cookbook/tauri-integration.mdx` carries a `.taurignore` step and a recipe `Cargo.toml` with `default-run = "iron-log"`.
- `README.md`'s "Known Issues" list has a third bullet summarizing the side-car (added by OF-019).
- There is no end-to-end TypeScript-bindings guide. OF-006 originally requested one but it was blocked behind the side-car model.

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

Each commit builds clean (`just full-check` covers Rust; verify any docs-site build command — e.g., `just site-build` or `npm run build` inside `site/` — still succeeds).

## Files to touch

- `site/src/content/docs/guides/typescript-bindings.mdx` (new) — end-to-end TS-bindings guide.
- `site/src/content/docs/guides/client-generation.mdx` (modify) — rewrite `bindings_path`, drop "Integration gotchas".
- `site/src/content/docs/cookbook/tauri-integration.mdx` (modify) — drop `.taurignore` + `default-run`; renumber steps.
- `README.md` (modify) — drop the OF-019 "Known Issues" bullet.

## Acceptance criteria

These are AC-16 from OF-015 — restated here for per-PR scope:

- [ ] AC-16.1: `site/src/content/docs/guides/typescript-bindings.mdx` exists; describes the ontogen-ts model end-to-end; covers supported subset, serde renames, external-types table, escape hatches, and the error model.
- [ ] AC-16.2: `site/src/content/docs/guides/client-generation.mdx`'s `bindings_path` section rewritten to reflect ontogen-ts; "Integration gotchas" section removed.
- [ ] AC-16.3: `site/src/content/docs/cookbook/tauri-integration.mdx`: `.taurignore` step removed; `default-run` removed from recipe `Cargo.toml`; subsequent steps renumbered.
- [ ] AC-16.4: `README.md`: third "Known Issues" bullet (OF-019 side-car summary) removed.
- [ ] AC-16.5: Supported subset documented (struct shapes, enum shapes, container handling, smart-pointer transparency, external-types table).
- [ ] AC-16.6: `#[ontogen::ts_opaque]` and `#[ontogen::ts_name]` attrs documented with code examples.
- [ ] AC-16.7 (umbrella per OF-015 AC-17): After this PR lands, `just full-check` passes on `main`; `cargo build` in `examples/iron-log/src-tauri/` succeeds; CI workflows pass.

## Out of scope

- **Phase-2 documentation** (shape-changing serde attrs, alternative output targets) — deferred to OF-015 phase 2 / spawned-as-needed.

## Dependencies

- [[OF-015-pr-7-pumice-validation]] ideally lands first so the docs can confidently describe "the supported subset covers iron-log and Pumice's full long-tail." If PR 7 is delayed by Pumice access (the human-only validation), PR 8 can ship on the strength of PR 5/6 alone and PR 7's reconciliation lands as a docs-update PR later.
