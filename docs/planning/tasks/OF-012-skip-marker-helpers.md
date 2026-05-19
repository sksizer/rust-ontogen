---
schema_version: '0'
status: closed/done
completion_note: "Shipped in 84d76dd on 2026-05-12."
---
# OF-012 - File-level skip marker for helper modules in `api/v1/`

- **Severity:** Low
- **Status:** Resolved (`84d76dd`, 2026-05-12)
- **Source:** [feedback.md OF-012](2026-05-12-pumice.md)

## Resolution

Shipped in `84d76dd` on 2026-05-12 (option 1 from the proposed-resolution list). `parse_api_module` now checks the source for a file-level skip marker in its leading comment-and-attribute block before invoking syn. Two grammars are honoured, both requiring exact trimmed equality:

- `// ontogen:skip` (plain line comment)
- `//! ontogen:skip` (inner doc comment, including when embedded inside a multi-line `//!` block)

When the marker is present, `parse_api_module` returns `ModuleParseResult::default()` immediately: the file is dropped from `ScanResult.modules` AND no [`SkipRecord`](./OF-001-parser-skip-diagnostic.md) is emitted for any `pub fn` inside it. Silencing the per-fn `cargo:warning=` lines is intentional — opt-out is a deliberate file-level decision, so the diagnostics that exist precisely to surface unintentional drops would be noise here.

Placement rule: the marker must appear in the run of blank lines / `//` / `///` / `//!` line comments / `#![...]` inner attributes that precedes the first real item. Markers buried after a `use` or `pub fn` are ignored so the directive can't be smuggled in mid-file.

Pumice's prior workaround (moving helper files one directory above `api/v1/`) is no longer necessary — helpers can sit alongside transport-bearing modules with a single comment line.

Test coverage in `src/servers/tests.rs` (5 cases):

- `test_parse_skip_marker_suppresses_module` - marker drops an otherwise-accepted module from the scan.
- `test_parse_skip_marker_suppresses_skip_records` - marker silences the per-fn warnings that OF-001 would otherwise emit.
- `test_parse_doc_comment_skip_marker` - `//! ontogen:skip` is honoured the same as `// ontogen:skip`.
- `test_parse_skip_marker_after_real_items_not_honored` - marker buried after a `pub fn` does NOT take effect.
- `test_parse_skip_marker_inside_doc_comment_block` - marker embedded inside a multi-line `//!` doc block is honoured.

Site docs: added an "Opting a file out: `// ontogen:skip`" subsection in `guides/api-layer.mdx` directly under the existing "Build-time skip warnings" section, so readers reaching for an escape hatch find it adjacent to the diagnostics it silences.

Per-function attribute (option 2 from the original sketch) and visibility-based opt-out (option 3) remain unimplemented; the whole-file mechanism is sufficient for the helper-module-in-`api/v1/` case.

---

*The remainder of this document is preserved as a record of the original analysis.*

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
