# Ontogen task backlog

One file per discrete piece of work. Each file is self-contained: goal,
approach / plan, files to touch, acceptance criteria, and (for closed tasks)
a `completion_note` summarising what shipped.

**Schema and conventions live at [`../README.md`](../README.md).** Frontmatter
fields (`status`, `created`, `last_reviewed`, `relevance_note`,
`completion_note`, `epic`), the `stage` / `stage/reason` status form, and
the date-prefixed naming convention for new tasks (`YYYY-MM-DD-<slug>.md`)
are all documented there.

This file is the backlog index — the open / closed tables below.
Filenames in the tables stay as-they-exist on disk; the older `OF-XXX-*.md`
files are legacy and keep their names, new tasks use the date-prefixed form.

## Open backlog

Items in `draft` / `proposed` / `backlog` / `ready` / `in-progress`.

| ID | Severity | Title |
| --- | --- | --- |
| [OF-015](./OF-015-productionize-typescript-generation.md) | Medium-High | Replace the OF-014 side-car with a new `ontogen-ts` crate (build-time AST → TS emission); obsoletes the OF-019 consumer-side workarounds. **Tracked at epic level**: [`../epics/ts-pipeline.md`](../epics/ts-pipeline.md). |
| [OF-018](./OF-018-ts-fallback-mistokenizes-generics.md) | Low | TS bindings fallback emitter mis-tokenizes generic return types |
| [OF-020](./OF-020-hierarchical-ts-bindings.md) | Low | Hierarchical TS bindings output (per-module directory) for codebases that outgrow flat `bindings.ts` + `#[ontogen::ts_name]` disambiguation |
| [OF-021](./OF-021-user-defined-generics-in-ts-emitter.md) | Low | First-class support for user-defined generic types in `ontogen-ts` (phase 1 rejects them with the concrete-type-alias workaround) |
| [OF-022](./OF-022-richer-external-type-renderings.md) | Low | Richer external-type renderings in `ontogen-ts` (imported TS types like `moment.Moment` in addition to phase-1's primitive-only `"string"` / `"unknown"`) |

## Pumice feedback round 2 (2026-05-14)

Source: [`../../feedback/2026-05-14-pumice.md`](../../feedback/2026-05-14-pumice.md). Three new findings surfaced when Pumice upgraded to ontogen rev `168ff379`. Mapped to upstream IDs OF-016/17/18 to avoid collision with the existing OF-013/14/15 (the consumer's log numbers them independently as OF-013/14/15).

## Priority Planning

Narrative thread on what landed when, and what's queued. Struck-through items
are closed.

1. ~~**OF-008 + OF-010**~~ - resolved in `7c056fe` (2026-05-12).
2. ~~**OF-001 + OF-005**~~ - resolved in `919b74a` (2026-05-12).
3. ~~**OF-011**~~ - resolved in `387d460` (2026-05-12); spawned [OF-013](./OF-013-ast-param-to-owned-type.md) as a follow-up.
4. ~~**OF-013**~~ - resolved in `71d76ce` (2026-05-12); closes the OF-011 follow-up loop.
5. ~~**OF-012**~~ - resolved in `84d76dd` (2026-05-12).
6. ~~**OF-002 + OF-004**~~ - resolved in `d770838` (2026-05-12).
7. ~~**OF-006**~~ - warning shipped in `8bed7f7` (2026-05-12); the e2e bindings doc was promoted to [OF-014](./OF-014-redesign-ts-bindings-pipeline.md).
8. ~~**OF-007**~~ - resolved in `773d059` (2026-05-12); `#[ontogen::stateless]` no-op proc-macro opts a fn out of the state-first-param rule.
9. ~~**OF-003**~~ - resolved in `ef63a0d` (2026-05-12); `#[ontogen(rename = "...")]` proc-macro attribute + `NamingConfig::command_overrides` config map, source-wins.
10. ~~**OF-014**~~ - design pass + option 1 + option 3 hybrid spike landed `c87ba64` (2026-05-13) on `worktree-of-014-spike-option-3`; spawned [OF-015](./OF-015-productionize-typescript-generation.md) for productionization.
11. **OF-015** (replace the OF-014 side-car with a new `ontogen-ts` crate doing build-time AST→TS emission). Now tracked at epic level under [`../epics/ts-pipeline.md`](../epics/ts-pipeline.md); see that doc for the 8-PR phase mapping. PR 1 (scaffold + per-type emission) shipped via #55. Design decisions captured 2026-05-14: hard-cutover migration; OF-006 `FallbackRecord` removed (hard error only, no fallback placeholder, no configurable strictness); pool layer most-permissive (collects all visibilities + type aliases + generic decls; selection happens during validation); name collisions hard-error with `#[ontogen::ts_name]` as the named fix path. Phase 2 (deferred): `tag`/`content`/`untagged`/`flatten` shape attrs.
12. ~~**OF-009**~~ - resolved in `2804753` (2026-05-13); docs-only -- `NamingConfig` rustdoc and the configuration reference now carry a verified "mass nouns and Latin plural-tantums" callout.
13. ~~**OF-017**~~ - resolved in `207aa96` (2026-05-14); dropped the `Input`/`Query` substring gate at all three generator call sites and added the missing return-type walk in `mcp.rs`. The post-OF-008/10 AST walker filters primitives, qualified paths, and known containers on its own; the gate was a holdover from the pre-AST walker and no longer earned its keep.
14. ~~**OF-016**~~ - resolved in `b2f882c` (2026-05-14); classifier now consults the first-param AST so `get_*` with a body-carrying custom struct routes as `CustomPost` instead of forcing a broken `GET /api/...:filter` with `Path<String>`. Also replaces the name-based `is_read_operation` with `is_read_op(&OpKind)` so the classification result is the single source of truth across the pipeline.
15. **OF-018** (low — TS fallback emitter mis-tokenizes generics). Closes naturally under [OF-015](./OF-015-productionize-typescript-generation.md)'s 2026-05-14 decision to remove the `FallbackRecord` warning entirely and hard-error on unsupported types — the fallback emitter goes away, the mis-tokenization can't happen.
16. ~~**OF-019**~~ - resolved 2026-05-14; rewrote the `bindings_path` section in `guides/client-generation.mdx` to describe the OF-014 model (bindings.ts is an output; two emitters; specta opt-in for long-tail types) and added an "Integration gotchas" section covering `default-run`, `.taurignore`, and the CI disk-pressure env-gate. `cookbook/tauri-integration.mdx` recipe now ships `default-run` in step 1 and a dedicated `.taurignore` step. Iron-log gains `.taurignore`, `default-run = "iron-log"`, and an `IRON_LOG_SKIP_SERVER_CODEGEN` env-gate in `build.rs`. README "Known Issues" carries a short summary that links to the full gotchas section. Three upstream fixes (idempotent side-car writes, emit outside `src/bin/`, first-class `disable_codegen` knob) deferred to OF-015 productionization.
17. **OF-020** (low — hierarchical TS bindings output, per-module directory structure) — speculative future work, only earns its keep if a real consumer hits collision-fatigue with [OF-015](./OF-015-productionize-typescript-generation.md)'s phase-1 flat-bindings + `#[ontogen::ts_name]` approach. Filed 2026-05-14 alongside the OF-015 design pass; not on the OF-015 critical path.
18. **OF-021** (low — first-class user-defined generics in `ontogen-ts`) — phase 1 rejects with the concrete-type-alias workaround (`pub type PaginatedWorkouts = Paginated<Workout>`). Future work captures the two design strategies (monomorphization-as-default vs TS-generic emission via `#[ontogen::ts_generic]` opt-in) and the bound/lifetime/default open questions. Filed 2026-05-14 alongside the OF-015 design pass; priority depends on Pumice's actual generic-use count.
19. **OF-022** (low — richer external-type renderings: imported TS types in addition to primitives). Phase 1's `external_types: HashMap<TypePath, &'static str>` handles `"string"`/`"unknown"`-style primitive renderings; OF-022 generalizes the value type to an enum that also supports `Imported { module, name, local_name }` for `moment.Moment` / `luxon.DateTime` / branded-string patterns. Purely additive when it lands. Filed 2026-05-14 alongside the OF-015 design pass.
20. ~~**OF-023**~~ - relocated workspace members under `crates/`; shipped via #54 (2026-05-14). Pure repo-hygiene cleanup; no behavioural impact.

## Closed

Older legacy frontmatter (`status: closed` + `resolution: fixed` +
`resolution_commit:`) is preserved on existing files. New closes use the
`status: closed/<reason>` form with a `completion_note:` body field per
[`../README.md`](../README.md).

| ID                                                 | Resolution                                                                                                                                                                              | Commit    | Date       |
| -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- | ---------- |
| [OF-008](./OF-008-inner-type-strip-option.md)      | Fixed via syn::Type AST walker in `collect_type_import`. Breaking API change.                                                                                                           | `7c056fe` | 2026-05-12 |
| [OF-010](./OF-010-collect-type-import-generics.md) | Fixed together with OF-008.                                                                                                                                                             | `7c056fe` | 2026-05-12 |
| [OF-011](./OF-011-handler-arg-forwarding.md)       | AST-driven `forward_arg_expr` in `src/servers/types.rs` replaces type-name heuristics across IPC and HTTP handlers. Spawned OF-013.                                                     | `387d460` | 2026-05-12 |
| [OF-001](./OF-001-parser-skip-diagnostic.md)       | `SkipRecord` / `ScanResult` plumb skipped pub fns out of the parser; `gen_api` and `generate_transport` emit one `cargo:warning=` per skip. Breaking signature change (crate-internal). | `919b74a` | 2026-05-12 |
| [OF-005](./OF-005-document-state-store-shapes.md)  | New "Accepted Signatures" table + "Build-time skip warnings" section in `guides/api-layer.mdx`; each row pinned by a unit test.                                                         | `919b74a` | 2026-05-12 |
| [OF-012](./OF-012-skip-marker-helpers.md)          | `// ontogen:skip` (and `//! ontogen:skip`) file-level marker. Marker in the leading comment block drops the file from `ScanResult.modules` and silences per-fn `SkipRecord`s.           | `84d76dd` | 2026-05-12 |
| [OF-013](./OF-013-ast-param-to-owned-type.md)      | AST-driven `param_to_owned_type` mirrors `forward_arg_expr`'s DST allowlist (`&str`→`String`, `&[T]`→`Vec<T>`, `&Path`→`PathBuf`, `&CStr`→`CString`, `&OsStr`→`OsString`). New site-docs section + end-to-end symmetry test. Breaking signature change (crate-internal). | `71d76ce` | 2026-05-12 |
| [OF-002](./OF-002-singleton-url-pluralization.md)  | First-class singleton modules: `// ontogen:singleton` source marker + `NamingConfig::singleton_modules` config-side set, ORed into `ApiModule::is_singleton`. HTTP/TS gen route singletons through `url_for_module` (singular kebab). Shipped with OF-004.                | `d770838` | 2026-05-12 |
| [OF-004](./OF-004-singleton-semantic.md)           | `ApiModule::is_singleton` IR field gives downstream generators a first-class singleton signal. Shipped with OF-002.                                                                                                                                                     | `d770838` | 2026-05-12 |
| [OF-006](./OF-006-ts-bindings-fallback-warning.md) | `FallbackRecord` plumbs missing-bindings types out of `transport.rs` and `ts_client.rs`; `generate_transport` emits one `cargo:warning=` per fallback. Warning text documented in `guides/client-generation.mdx`. The e2e bindings doc was promoted to [OF-014](./OF-014-redesign-ts-bindings-pipeline.md). | `8bed7f7` | 2026-05-12 |
| [OF-007](./OF-007-support-stateless-fns.md)        | `#[ontogen::stateless]` no-op proc-macro in `ontogen-macros`; the parser bypasses the state/store first-param check when present, and IPC/HTTP/MCP generators emit handler shapes without the `State<...>` extractor or a positional state forward. OF-001 skip diagnostic now hints at the attribute. New site-docs section + recipe. | `773d059` | 2026-05-12 |
| [OF-003](./OF-003-per-function-name-override.md)   | Per-function `#[ontogen(rename = "...")]` proc-macro attribute in `ontogen-macros` plus `NamingConfig::command_overrides` config map; both feed `ApiFn::command_override`. Source attribute wins over config. IPC handler name and TS HTTP client method follow the override; HTTP route paths and the underlying Rust fn name are unaffected. Malformed values surface as `SkipReason::InvalidRenameValue` via OF-001's diagnostic plumbing. Fixed a latent bug in `ts_client::generate_generic_ts_handler` along the way. | `ef63a0d` | 2026-05-12 |
| [OF-009](./OF-009-cruet-mass-noun-pitfall.md)      | Docs-only resolution. Added a verified "mass nouns and Latin plural-tantums" callout to both the `NamingConfig` rustdoc and `site/src/content/docs/reference/configuration.mdx`, calling out the four real cruet misfires (`data`, `metadata`, `settings`, `media`) and listing the mass nouns cruet already handles correctly (`information`, `news`, `evidence`, `series`, `schema`). No built-in override constant -- per the task's own recommendation, ship docs first and only adopt curated defaults if multiple downstreams keep hitting the same landmines. | `2804753` | 2026-05-13 |
| [OF-014](./OF-014-redesign-ts-bindings-pipeline.md) | Design pass + end-to-end spike of the option 1 + option 3 hybrid: `ts_bindings.rs` emits TS straight from `EntityDef` for entities + generated DTOs; `ts_sidecar.rs` generates a `__ontogen_ts_export.rs` binary inside the user's crate that runs specta v2 + `specta-typescript` via an isolated `CARGO_TARGET_DIR` and appends to `bindings.ts`. Env-guarded against build-script recursion. Iron-log builds with zero fallback warnings. Productionization deferred to [OF-015](./OF-015-productionize-typescript-generation.md). | `c87ba64` | 2026-05-13 |
| [OF-017](./OF-017-param-import-substring-gate.md)  | Dropped the `Input`/`Query` substring filter wrapping `collect_type_import` at all three generator call sites (`ipc.rs`, `http.rs`, `mcp.rs`); the post-OF-008/10 AST walker filters primitives, qualified paths, and known containers on its own. Also added the missing return-type walk in `mcp.rs`. New regression test parses the rendered `use crate::schema::{ ... }` block to assert every custom param/return type is imported. | `207aa96` | 2026-05-14 |
| [OF-016](./OF-016-classify-get-by-param-shape.md)  | AST-aware classifier: `get_*` with a body-carrying first param (custom struct / qualified path / `Vec<T>` / `HashMap<K, V>`) reclassifies as `CustomPost` so HTTP routes as POST with JSON body extraction instead of forcing GET with `Path<String>` stuffing. Id-like primitives, `Option<…>`, and zero-param `get_*` keep their old behaviour. Also replaces the name-based `is_read_operation` with `is_read_op(&OpKind)` -- single source of truth derived from the classifier. New unit matrix + end-to-end test parses synthetic api/v1/export.rs through `http::generate` and asserts the rendered routes + handler signatures. Site docs updated to reflect the new rules. Source-attribute escape hatch (`#[ontogen::post]`) deferred until a real-world repro motivates it. | `b2f882c` | 2026-05-14 |
| [OF-019](./OF-019-document-side-car-tauri-watcher.md) | Documentation lift of the OF-014 side-car's three consumer-side gotchas (`default-run`, `.taurignore`, CI disk pressure) from the spike ticket into user-facing pages. Rewrote the `bindings_path` section in `guides/client-generation.mdx` to describe the OF-014 model and added an "Integration gotchas" section. `cookbook/tauri-integration.mdx` recipe now sets `default-run` in step 1 and adds a `.taurignore` step. Iron-log gains `.taurignore`, `default-run`, and an `IRON_LOG_SKIP_SERVER_CODEGEN` env-gate. README "Known Issues" carries a short summary. Upstream fixes that would obviate the gotchas (idempotent side-car writes, emit outside `src/bin/`, `disable_codegen` knob) deferred to OF-015. | `d0a9c5e` | 2026-05-14 |
| [OF-023](./OF-023-relocate-workspace-members-under-crates.md) | Relocated `ontogen-core/` and `ontogen-macros/` under `crates/`. Pure repo-hygiene cleanup; no behavioural impact. Shipped via #54. | `5944307` | 2026-05-14 |
