---
status: open
---
# OF-017 - Param type-import collector is gated by `Input`/`Query` substring filter

- **Severity:** High (produces uncompileable output for any param struct whose name doesn't match the substring filter)
- **Source:** Pumice feedback, [`docs/feedback/2026-05-14-pumice.md`](../feedback/2026-05-14-pumice.md). Filed in the consumer's log as "OF-014" (their numbering); upstream uses OF-017 to avoid collision with the (resolved) [OF-014](./OF-014-redesign-ts-bindings-pipeline.md) TS-bindings redesign.
- **Related:** [OF-008](./OF-008-inner-type-strip-option.md) / [OF-010](./OF-010-collect-type-import-generics.md) — AST-ified the *walker* but left the substring *gate* in place. This ticket closes the gap.

## Problem

After [OF-008](./OF-008-inner-type-strip-option.md) and [OF-010](./OF-010-collect-type-import-generics.md), `collect_type_import` walks `syn::Type` recursively and handles `Option<T>`, `Vec<T>`, `HashMap<K, V>`, and friends correctly. Return positions get the walker called unconditionally, so they pick up arbitrary user types fine.

Parameter positions are different. The IPC and HTTP generators wrap the walker in a substring filter on the rendered type name:

```rust
// src/servers/generators/ipc.rs:117-127
for p in &f.params {
    // The substring filter on the rendered name is intentional:
    // only param types named like `*Input` or `*Query` get pulled
    // into the import list. We still walk the AST for the actual
    // collection so generic wrappers (Option, Vec, …) get peeled
    // properly.
    let ty = extract_input_type(&p.ty);
    if ty.contains("Input") || ty.contains("Query") {
        collect_type_import(&p.ty_ast, &mut type_imports);
    }
}
```

Same pattern in `src/servers/generators/http.rs:54-61`.

The comment is misleading — the AST walker *would* handle the collection, but the substring gate prevents it from being called for any param type whose ident doesn't contain `Input` or `Query`. Wrapper structs like `ExportRequest`, `ExportFilterRequest`, `FilenameTemplateRequest` (the consumer's verbatim repro) silently miss the import emission.

The downstream symptom is identical to OF-008/OF-010 pre-fix: the generated transport file references the type without a corresponding `use crate::schema::T;`, and compilation fails with "cannot find type `ExportRequest` in this scope."

## Location

- `src/servers/generators/ipc.rs:117-127` (`generate`, the parameter loop inside the imports collection block).
- `src/servers/generators/http.rs:54-61` (`generate`, same loop).
- The walker itself (`src/servers/types.rs:111` `collect_type_import`) is correct post-OF-008/10 and needs no changes — the fix is to stop gating its invocation.

## Current behavior

Repro:

```rust
// src-tauri/src/schema/export.rs
pub struct ExportRequest { /* fields */ }
pub struct ExportFilterRequest { /* fields */ }

// src-tauri/src/api/v1/export.rs
pub fn run(state: &Store, request: &ExportRequest) -> Result<ExportSummary, AppError> { ... }
pub fn filtered_sessions(
    state: &Store,
    filter: &ExportFilterRequest,
) -> Result<Vec<Session>, AppError> { ... }
```

`ExportSummary` (return position) gets imported correctly via `collect_type_import(&f.return_type_ast, ...)`. `ExportRequest` and `ExportFilterRequest` (param position) do *not* — `extract_input_type(&p.ty)` returns `"ExportRequest"` / `"ExportFilterRequest"`, neither contains `"Input"` or `"Query"`, so the AST walker is never called for them. The emitted `transport.rs` references both types without importing them, and compilation fails.

The Pumice consumer's verified workaround is the fully-qualified path: `&crate::schema::ExportRequest` in the service-fn signature. [OF-011](./OF-011-handler-arg-forwarding.md)'s AST-driven forwarding preserves the qualified path through to the generated handler, so the type resolves at compile time without participating in the import collector at all. Cosmetic noise in user code; not a correctness issue.

## Proposed resolution

Drop the substring filter and let the AST walker run unconditionally on every param's AST. The walker already:

- skips prelude scalars (`String`, `i64`, `bool`, …) at `src/servers/types.rs:71-95`,
- skips qualified paths (`crate::schema::Foo`) at `:128-136` (they're handled by the entity-import path),
- skips known containers (`Option`, `Vec`, `HashMap`, …) at `:141-144`,
- skips `dyn Trait` / `impl Trait` and tuples-of-uninteresting-types.

So unconditional invocation collects exactly the simple type idents that genuinely need a `use crate::schema::T;`, no more. The substring gate was a holdover from before OF-008/10 AST-ified the walker; it no longer earns its keep.

Three-line diff per generator: remove the `if ty.contains(...)` wrapper, drop the unused `let ty = extract_input_type(&p.ty);` if nothing else needs it, leave the walker call.

## Effort

Small. One-line change in two files plus the test matrix expansion. Comparable to the OF-008 finish-up that converted the call sites once the walker was AST-ified.

## Tests

- Add cases to `test_collect_type_import_matrix` (in `src/servers/tests.rs:338`) covering:
  - param type `ExportRequest` (bare custom struct) → imports `ExportRequest`.
  - param type `&ExportRequest` → imports `ExportRequest`.
  - param type `Option<&ExportRequest>` → imports `ExportRequest`.
  - param type `Vec<ExportRequest>` → imports `ExportRequest`.
  - param type `String` → imports nothing (prelude).
  - param type `&str` → imports nothing.
- End-to-end test mirroring `test_of013_unsized_dst_owned_form_in_ipc`: parse a service fn through `parse::scan_api_dir`, run through `generators::ipc::generate` *and* `generators::http::generate`, assert the rendered `use crate::schema::{...}` block contains every custom-struct param type referenced by the function (not just `*Input` / `*Query`-suffixed ones).

## Open questions

- **Does dropping the gate break any existing generated output?** The walker already filters everything that *shouldn't* be imported (primitives, qualified paths, containers). The only difference vs. today is that param structs without `Input`/`Query` in their name now get imported instead of being silently skipped. That's a strict improvement — there's no path where an imported type causes a build failure that an unimported type doesn't. Run the full generator test suite + iron-log build to confirm.
- **Should the same fix apply to `mcp.rs`?** `src/servers/generators/mcp.rs:119` has an identical-looking call but inside a different control flow — verify whether it has the same substring gate before claiming the fix is generator-wide.
- **Why was the gate originally there?** Comment says "intentional," but no rationale recorded. Likely a defense-in-depth holdover from when the walker was string-based and could spuriously import primitive names. Post-OF-008/10 the walker can no longer do that, so the defense is redundant.

## Notes

- The consumer's filed-as-OF-014 entry includes the empirical verification that this is *not* a stale-cache issue: switching from `&crate::schema::ExportRequest` to a bare `&ExportRequest` (with a local `use crate::schema::ExportRequest;` block in the user fn) still fails compilation in the generated transport files because no `use` is emitted for the param-position type. Reproduces fresh.
- Cross-references the same area as OF-011's "fully-qualified path is preserved verbatim by the forwarding emitter" observation, which is the load-bearing piece of the current workaround.
