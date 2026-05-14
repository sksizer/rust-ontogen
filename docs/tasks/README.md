# Ontogen task backlog

One file per discrete piece of work. Each file is self-contained: severity, location in code, current behaviour, proposed resolution, effort estimate, and open questions.

## When an entry is resolved
- set frontmatter
	- status: closed
	- resolution: fixed (or wontfix)
	- resolution_date: ISO Timestamp
	- resolution_commit: commit-hash
- document resolution changes

# Planning Scratch

## Pumice feedback (OF-###)

Items surfaced while integrating ontogen into Pumice. Source: [`docs/feedback.md`](2026-05-12-pumice.md).

| ID | Severity | Title |
| --- | --- | --- |
| [OF-001](./OF-001-parser-skip-diagnostic.md) | High | Emit diagnostic when parser skips a non-matching `pub fn` |
| [OF-002](./OF-002-singleton-url-pluralization.md) | Medium | Singleton module URL pluralization |
| [OF-003](./OF-003-per-function-name-override.md) | Medium | Per-function command-name override |
| [OF-004](./OF-004-singleton-semantic.md) | Low/Med | First-class singleton-module semantic for downstream generators |
| [OF-005](./OF-005-document-state-store-shapes.md) | Medium | Document accepted `state_type` / `store_type` first-param shapes |
| [OF-006](./OF-006-ts-bindings-fallback-warning.md) | Medium | Warn on TS bindings fallback to `Record<string, unknown>` |
| [OF-007](./OF-007-support-stateless-fns.md) | Medium | Support pure utility functions without a no-op state parameter |
| [OF-008](./OF-008-inner-type-strip-option.md) | High | `inner_type` should recursively strip `Option<T>` and other wrappers |
| [OF-009](./OF-009-cruet-mass-noun-pitfall.md) | Low | Document or default-override cruet mass-noun singularization |
| [OF-010](./OF-010-collect-type-import-generics.md) | High | `collect_type_import` should recurse into multi-arg generics |
| [OF-011](./OF-011-handler-arg-forwarding.md) | High | Consistent handler argument forwarding; fix `.as_deref()` on non-Deref `Option<T>` |
| [OF-012](./OF-012-skip-marker-helpers.md) | Low | File-level skip marker for helper modules in `api/v1/` |
| [OF-013](./OF-013-ast-param-to-owned-type.md) | Medium | AST-ify `param_to_owned_type` for unsized-DST inner types (follow-up from OF-011) |
| [OF-014](./OF-014-redesign-ts-bindings-pipeline.md) | Medium | Redesign the TypeScript bindings / type-generation pipeline (spawned from OF-006) |
| [OF-015](./OF-015-productionize-typescript-generation.md) | Medium-High | Productionize the TypeScript generation pipeline (OF-014 follow-up) |
| [OF-018](./OF-018-ts-fallback-mistokenizes-generics.md) | Low | TS bindings fallback emitter mis-tokenizes generic return types |

## Pumice feedback round 2 (2026-05-14)

Source: [`docs/feedback/2026-05-14-pumice.md`](../feedback/2026-05-14-pumice.md). Three new findings surfaced when Pumice upgraded to ontogen rev `168ff379`. Mapped to upstream IDs OF-016/17/18 to avoid collision with the existing OF-013/14/15 (the consumer's log numbers them independently as OF-013/14/15).

## Priority Planning

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
11. **OF-015** (productionize the TS generation pipeline; closes spike-grade shortcuts, ships user-facing guide, decides OF-006 warning fate).
12. ~~**OF-009**~~ - resolved in `2804753` (2026-05-13); docs-only -- `NamingConfig` rustdoc and the configuration reference now carry a verified "mass nouns and Latin plural-tantums" callout.
13. ~~**OF-017**~~ - resolved in `207aa96` (2026-05-14); dropped the `Input`/`Query` substring gate at all three generator call sites and added the missing return-type walk in `mcp.rs`. The post-OF-008/10 AST walker filters primitives, qualified paths, and known containers on its own; the gate was a holdover from the pre-AST walker and no longer earned its keep.
14. ~~**OF-016**~~ - resolved in `b2f882c` (2026-05-14); classifier now consults the first-param AST so `get_*` with a body-carrying custom struct routes as `CustomPost` instead of forcing a broken `GET /api/...:filter` with `Path<String>`. Also replaces the name-based `is_read_operation` with `is_read_op(&OpKind)` so the classification result is the single source of truth across the pipeline.
15. **OF-018** (low — TS fallback emitter mis-tokenizes generics). Hold until [OF-015](./OF-015-productionize-typescript-generation.md) decides the fallback path's fate; closes naturally if OF-015 hard-errors or removes the fallback.

## Resolved

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
