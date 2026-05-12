# OF-007 - Support pure utility functions without a no-op state parameter

- **Severity:** Medium
- **Status:** Open (design decision needed)
- **Source:** [feedback.md OF-007](../feedback.md)

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
