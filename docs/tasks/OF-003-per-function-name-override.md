# OF-003 - Per-function command-name override

- **Severity:** Medium
- **Status:** Open
- **Source:** [feedback.md OF-003](../feedback.md)

## Problem

Every emitted IPC command / TS method name is prefixed with `{url_singular(module)}_`. This is correct when the module name is the noun the operations are *about*, but when the function name already encodes the noun, the result is redundant:

```
journal::get_tag_history   →   journalGetTagHistory()   // worse
                           ←   tagGetHistory()           // better
```

The only escape hatch today is to rename either the function (`get_history` instead of `get_tag_history`) or fragment the module structure (`tag.rs` purely so its prefix reads cleanly).

## Location

- `src/servers/generators/ipc.rs:76-79` (`command_name`) - hard-codes `format!("{entity}_{fn_name}")`.
- TS client generator consumes the same name (snake-to-camel converted), so a single fix at the IPC layer propagates correctly.

## Proposed resolution

Allow a per-function name override. Two candidate shapes:

1. **Source-side:** doc-comment directive on the function, e.g.:
   ```rust
   /// @ontogen(name = "tag_get_history")
   pub fn get_tag_history(store: &Store) -> ... { ... }
   ```
   Parser extracts the directive into `ApiFn::name_override: Option<String>`. `command_name` returns the override when set.

2. **Config-side:** `Config::ts_command_overrides: HashMap<String, String>` keyed by `module::fn_name`.

Recommendation: source-side. The override lives next to the function it modifies and survives moves between modules. The doc-comment shape also lays the groundwork for related markers in [OF-007](./OF-007-support-stateless-fns.md) and [OF-012](./OF-012-skip-marker-helpers.md).

## Effort

Medium. Directive parsing + plumbing through `ApiFn` + tests.

## Notes

- Pick a single doc-comment grammar and reuse it across OF-003 / OF-007 / OF-012. Bikeshed candidates: `@ontogen(...)`, `#[ontogen(...)]` (real attribute), or `// ontogen: ...` magic comments. A real attribute is the most idiomatic Rust but requires the user to import / declare it.
- The HTTP route segment also derives from the function name (via `derive_action` in `types.rs:251`). Decide whether the override should also influence the HTTP path or only IPC/TS naming.
