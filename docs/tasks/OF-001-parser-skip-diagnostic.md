---
status: draft
---
# OF-001 - Emit diagnostic when parser skips a non-matching `pub fn`

- **Severity:** High
- **Status:** Open
- **Source:** [feedback.md OF-001](2026-05-12-pumice.md)
- **Related:** [OF-005](./OF-005-document-state-store-shapes.md)

## Problem

The API parser accepts a `pub fn` only when the rendered type string of its first parameter contains the configured `state_type` (or `store_type`) as a substring. Non-matching functions are silently dropped: no warning, no diagnostic, build still succeeds, but no IPC handler / HTTP route / TS method is emitted. Downstream consumers have nothing to call against and no signal that anything is missing.

## Location

- `src/servers/parse.rs:85-120` (`parse_api_module`) - the `if !is_accepted { continue; }` skip path has no logging.

## Current behavior

```rust
// api/v1/foo.rs - silently invisible to ontogen
pub fn x(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), AppError> { ... }
```

Because `State<'_, Mutex<AppState>>` does not contain the configured `state_type` substring (e.g., `PumiceState`), the function is dropped without any output.

## Proposed resolution

Surface skipped functions during build:

1. Return a list of `SkipRecord { file, fn_name, first_param_ty, reason }` from `parse_api_module` / `scan_api_dir` alongside the parsed modules.
2. In the pipeline (caller in `src/pipeline.rs`), emit one `cargo:warning=` per skipped function. Suggested wording:
   ```
   ontogen: skipped fn `foo::bar` - first param type `State<'_, Mutex<AppState>>` does not match state_type 'PumiceState' or store_type 'Store'
   ```
3. (Optional) Behind a config flag, escalate to a hard error.

Pair this with the docs page from [OF-005](./OF-005-document-state-store-shapes.md) so the diagnostic and the documented contract reference each other.

## Effort

Small. Roughly 30-50 LOC plus a unit test that asserts a skipped fn is reported.

## Notes

- The parser is library code; it must not call `println!` directly. The pipeline (build-time caller) is the right place to emit `cargo:warning=`.
- Functions with zero parameters are currently also silently dropped (see [OF-007](./OF-007-support-stateless-fns.md)) - the same diagnostic path covers that case.
