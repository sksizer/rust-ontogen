# Ontogen task backlog

One file per discrete piece of work. Each file is self-contained: severity, location in code, current behaviour, proposed resolution, effort estimate, and open questions.

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

## Suggested priority

1. ~~**OF-008 + OF-010**~~ - resolved in `7c056fe` (2026-05-12).
2. **OF-001 + OF-005** (diagnostic + docs page documents the contract).
3. **OF-011** (groundwork now in place via `7c056fe`; effort dropped to Medium).
4. **OF-012** (small, isolated).
5. **OF-002 + OF-004** (singleton marker; design discussion).
6. **OF-003** (override mechanism; design discussion).
7. **OF-006** (warning is easy; e2e bindings doc is its own task).
8. **OF-009** (lowest-value; documentation only).

## When an entry is resolved

Set frontmatter `status: closed` and `resolution: fixed` (or `wontfix`), update the
inline status line to `Resolved (<commit>, <date>)`, add a Resolution section near the
top, and append a row to the Resolved table below. Do not delete the file - retain it
for context.

## Resolved

| ID | Resolution | Commit | Date |
| --- | --- | --- | --- |
| [OF-008](./OF-008-inner-type-strip-option.md) | Fixed via syn::Type AST walker in `collect_type_import`. Breaking API change. | `7c056fe` | 2026-05-12 |
| [OF-010](./OF-010-collect-type-import-generics.md) | Fixed together with OF-008. | `7c056fe` | 2026-05-12 |
