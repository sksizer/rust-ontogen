# OF-012 - File-level skip marker for helper modules in `api/v1/`

- **Severity:** Low
- **Status:** Open
- **Source:** [feedback.md OF-012](../feedback.md)

## Problem

The parser scans every `.rs` file under `api_dir` (and one subdirectory level). Any `pub fn` whose first parameter matches `state_type` / `store_type` is considered for emission. There is no opt-out marker, so shared helper modules placed in `api/v1/` get scanned and the generator tries to emit transport commands for them.

Pumice's workaround is to move helper files out of `api/v1/` entirely (one directory up). This works but fragments the layout.

## Location

- `src/servers/parse.rs:85-120` (`parse_api_module`) - no skip mechanism.
- `src/servers/parse.rs:207-223` (`scan_api_dir`) - walks the directory unconditionally.

## Proposed resolution

Add a skip marker. Three candidate shapes:

1. **File-level magic comment:** `// ontogen:skip` (or `//! @ontogen(skip)`) at the top of the file. `parse_api_module` returns `None` when present.
2. **Per-function attribute:** `#[ontogen::skip]` on individual functions.
3. **Visibility-based:** Only scan `pub` functions (already done). Honour `pub(crate)` as opt-out - functions visible only within the crate are presumed internal helpers.

Recommendation: ship (1) first. It is the least surprising mechanism for the common case (whole-file helper module), composes with the directive grammar in [OF-003](./OF-003-per-function-name-override.md) and [OF-007](./OF-007-support-stateless-fns.md), and avoids overloading visibility semantics. (2) can follow if function-level granularity becomes needed.

Note: option (3) might break legitimate use cases - a `pub(crate)` API fn intended for codegen would silently disappear - so it should not be the default.

## Effort

Small. ~20-40 LOC plus tests covering a helper file with and without the marker.

## Notes

- Generic functions are not currently emittable regardless (the type-import logic can't handle them), so today they fail to compile if scanned. After this fix, marking them skip makes the error go away cleanly.
- Pair with [OF-001](./OF-001-parser-skip-diagnostic.md)'s diagnostic so users who *forget* the skip marker get a warning explaining why nothing was emitted.
