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

## Priority Planning

1. ~~**OF-008 + OF-010**~~ - resolved in `7c056fe` (2026-05-12).
2. ~~**OF-001 + OF-005**~~ - resolved in `919b74a` (2026-05-12).
3. ~~**OF-011**~~ - resolved in `387d460` (2026-05-12); spawned [OF-013](./OF-013-ast-param-to-owned-type.md) as a follow-up.
4. ~~**OF-013**~~ - resolved in `71d76ce` (2026-05-12); closes the OF-011 follow-up loop.
5. ~~**OF-012**~~ - resolved in `84d76dd` (2026-05-12).
6. ~~**OF-002 + OF-004**~~ - resolved in `d770838` (2026-05-12).
7. ~~**OF-006**~~ - warning shipped in `8bed7f7` (2026-05-12); the e2e bindings doc was promoted to [OF-014](./OF-014-redesign-ts-bindings-pipeline.md).
8. **OF-003** (override mechanism; design discussion).
9. **OF-014** (TS bindings pipeline redesign; design discussion).
10. **OF-009** (lowest-value; documentation only).

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
