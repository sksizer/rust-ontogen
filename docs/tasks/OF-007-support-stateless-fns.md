---
status: closed
resolution: fixed
resolution_date: 2026-05-12
resolution_commit: 773d059
---
# OF-007 - Support pure utility functions without a no-op state parameter

- **Severity:** Medium
- **Status:** Resolved (`773d059`, 2026-05-12)
- **Source:** [feedback.md OF-007](2026-05-12-pumice.md)

## Resolution

Shipped in `773d059` on 2026-05-12, with site docs in the follow-up commit.

Implemented option C from the original analysis: a per-function opt-in
attribute `#[ontogen::stateless]` (a no-op proc-macro in
`ontogen-macros`, re-exported from the top-level `ontogen` crate). The
attribute expands to a pass-through of the annotated item; the parser
reads it via `syn` during build-time scanning.

Behaviour when present on a `pub fn`:

- The first-param state/store substring check is bypassed.
- The zero-param guard (`NoParams`) is bypassed.
- The `&self` / `self` guard is **still enforced** — method signatures
  don't fit free-function API modules regardless of statelessness.
- `ApiFn::is_stateless` is set, and `params` keeps every declared input
  (no state slot to skip).

Generators emit a second handler shape for stateless fns:

- **IPC:** no `tauri::State<'_, Arc<AppState>>` argument, no
  store-construction or prefix-validation body, no positional state
  forward when calling the service fn.
- **HTTP:** no `State(state): State<Arc<AppState>>` extractor, no
  prefix-accessor validation, route still nested under the module URL
  (`/api/<module-url>/<fn>`).
- **MCP:** tool handler closure body skips the store/state prefix and
  forwards only the user-declared parameters.

Route nesting is unchanged: stateless fns sit under their module URL
the same as state-bearing fns. `pub fn copy(...)` in `clipboard.rs`
becomes IPC command `clipboard_copy` and HTTP route
`/api/clipboards/copy` (with the module URL transformed per the usual
rules — flip the module to a singleton via `// ontogen:singleton` if
you want `/api/clipboard/copy`).

**Attribute path matching.** The parser matches the attribute on the
final path segment, so both `#[stateless]` (after
`use ontogen::stateless`) and `#[ontogen::stateless]` (fully qualified)
are accepted. A foreign `stateless` attribute from an unrelated crate
would also match; users hitting that collision should rename or omit
in API modules.

**OF-001 skip diagnostic.** Unmarked stateless-shaped fns continue to
emit `cargo:warning=` lines, with `add #[ontogen::stateless] if this
fn intentionally takes no state` appended to the `FirstParamMismatch`
and `NoParams` messages so users find the opt-in from the build output.

**Tests:**

- `test_of007_stateless_zero_params_accepted` — `#[ontogen::stateless]
  fn now() -> Result<i64, _>` parses, produces no `SkipRecord`, and
  carries `is_stateless = true`.
- `test_of007_stateless_non_state_first_param_kept` — `fn copy(text:
  &str)` keeps `text` as a regular `Param` instead of treating it as
  a state slot.
- `test_of007_bare_stateless_path_form_accepted` — `#[stateless]`
  after `use ontogen::stateless` is matched by the final path segment.
- `test_of007_stateless_self_receiver_still_rejected` — `&self` is
  still excluded even with the attribute.
- `test_of007_unmarked_stateless_fn_emits_skip_with_hint` — the
  `FirstParamMismatch` / `NoParams` diagnostics include the new hint.
- `test_of007_stateless_ipc_handler_shape`,
  `test_of007_stateless_http_handler_shape`,
  `test_of007_stateless_mcp_tool_shape` — end-to-end rendering checks
  on each generator (no `State<...>` extractor, call site forwards no
  state argument).

**Documentation:**

- `site/src/content/docs/guides/api-layer.mdx` gained a "Stateless
  utility functions: `#[ontogen::stateless]`" subsection with both
  attribute path forms, the rendered handler shapes, and the routing
  rule.
- `site/src/content/docs/cookbook/custom-api-endpoints.mdx` gained a
  recipe block under "Writing scannable functions" with zero-param and
  non-state-first-param examples plus a cross-reference to the
  api-layer.mdx subsection.

---

*The remainder of this document is preserved as a record of the original analysis.*

## Problem

The parser requires every API function's first parameter to be `&PumiceState` or `&Store` (or whatever's configured). Pure utility functions - data transformations, OS-level side effects - cannot satisfy this. The workaround is to add a `_state: &PumiceState` parameter that is never used, just to placate the parser. Function signatures end up shaped by the parser's constraints rather than by what the function actually needs.

## Location

- `src/servers/parse.rs:103-119` (`parse_api_module`) - the first-param check.

## Current behavior

Pumice workaround:

```rust
// api/v1/clipboard.rs - _state is unused, exists only to be parsed
pub fn copy(_state: &PumiceState, text: &str) -> Result<(), AppError> {
    pumice_desktop::clipboard::copy_text(text.to_string()).map_err(AppError::DbError)
}
```

Functions with zero parameters are also silently dropped (see [OF-001](./OF-001-parser-skip-diagnostic.md)).

## Proposed resolution

**Design call required.** Three options:

A. **Allow stateless fns.** Permit `pub fn` with no parameters, or with a first parameter that isn't state-shaped, to still be emitted. IPC/HTTP wrappers do not thread any state through. Removes the lying-signature workaround at the cost of a second handler shape in the generators.

B. **Keep current rule, document explicitly.** Document the constraint, recommend the `_state` workaround as the idiomatic pattern. Cheaper, but workaround stays ugly.

C. **Per-function opt-in marker.** Require an explicit `/// @ontogen(stateless)` directive on functions that opt out of the state-param rule. Cleaner than A (only the marked functions get the second handler shape) and avoids accidentally emitting handlers for unrelated `pub fn`s in the same file. Composes with the directive grammar in [OF-003](./OF-003-per-function-name-override.md) and [OF-012](./OF-012-skip-marker-helpers.md).

Recommendation: **C**. Stateless emission is real value; an opt-in marker keeps the default behaviour conservative.

## Effort

Medium. The generator divergence (a second IPC handler shape with no state arg, second HTTP handler with no `State<...>` extractor) is mechanical but needs careful testing.

## Notes

- This is the only OF-### item that materially changes the generator's output shape. Worth discussing before committing.
- Whichever option is chosen, [OF-001](./OF-001-parser-skip-diagnostic.md)'s diagnostic should mention "stateless functions are also skipped by default" so users discover this case from the build output.
