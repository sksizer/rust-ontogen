---
status: closed
resolution: fixed
---
# OF-008 - `inner_type` should recursively strip `Option<T>` and other wrappers

- **Severity:** High (produces broken output)
- **Status:** Resolved (`7c056fe`, 2026-05-12)
- **Source:** [feedback.md OF-008](2026-05-12-pumice.md)
- **Related:** [OF-010](./OF-010-collect-type-import-generics.md) (same root cause, fixed together)

## Resolution

Shipped in `7c056fe` on 2026-05-12. Took the AST-based approach (Option B from the
discussion) rather than the string-walker proposed below, after noticing the whole bug
class exists because the parser flattens `syn::Type` to a string and downstream code
re-parses it via substring checks.

- `ApiFn` and `Param` now carry the parsed `syn::Type` alongside the rendered string
  (`return_type_ast`, `ty_ast`). Parser populates both.
- `collect_type_import` was rewritten as `fn(&syn::Type, &mut Vec<String>)` and walks the
  AST recursively. Single-arg containers (`Option`, `Vec`, `Box`, `Arc`, `Rc`, `Cow`) are
  peeled; multi-arg containers (`HashMap`, `BTreeMap`, `HashSet`, `BTreeSet`, `IndexMap`,
  `IndexSet`, `Result`) recurse into each arg.
- `inner_type` left untouched (callers in `ipc.rs:218,501` still need one-layer behaviour
  for `PaginatedResult<{item_type}>`).
- Call sites in `src/servers/generators/{ipc,http,mcp}.rs` updated to pass the AST.
- 29-case unit-test matrix + end-to-end regression test added in `src/servers/tests.rs`.
- Iron-log build and `just full-check` pass clean; no snapshot diffs.

This is a breaking API change (new pub fields on `ApiFn`/`Param`, `collect_type_import`
signature change, `syn::Type` now in the public surface). Flag in next release notes.

Bonus: the AST is now available for [OF-011](./OF-011-handler-arg-forwarding.md), making
that fix substantially simpler than originally scoped.

---

*The remainder of this document is preserved as a record of the original analysis and
the proposed-but-not-shipped string-walker approach.*

## Problem

`inner_type` unwraps `Vec<T>` but nothing else. Every other wrapper - `Option`, `HashMap`, `Box`, etc. - falls through to `collect_type_import`, which then adds the fully-rendered string to the import list. The emitted `use crate::schema::{ ... };` block ends up containing things like `Option<RestoreCandidate>`, which is not a valid Rust path.

## Location

- `src/servers/types.rs:52-54` (`inner_type`) - only unwraps `Vec<T>`.
- `src/servers/types.rs:58-92` (`collect_type_import`) - calls `inner_type` once, checks for `::`, checks against a primitive allowlist, then pushes the result.
- Call sites that feed return types in: `src/servers/generators/ipc.rs:109`, `src/servers/generators/http.rs:52`, `src/servers/generators/mcp.rs:117`.

The string the call sites pass is the value of `ApiFn::return_type` - already the `T` from `Result<T, E>` (parser strips `Result` in `parse.rs:189-202`), so the bug surface is everything *inside* the `Ok` arm.

## Current behavior

Each row below is a real service-function return type and what ontogen emits today. The "Generated import line" column shows what ends up in the `use crate::schema::{ ... };` block.

| Service return type                              | `inner_type` returns                          | Generated import line                            | Compiles? |
| ------------------------------------------------ | --------------------------------------------- | ------------------------------------------------ | --------- |
| `RestoreCandidate`                               | `RestoreCandidate`                            | `RestoreCandidate,`                              | ✅        |
| `Vec<RestoreCandidate>`                          | `RestoreCandidate`                            | `RestoreCandidate,`                              | ✅        |
| `Option<String>`                                 | `Option<String>`                              | `Option<String>,`                                | ❌        |
| `Option<RestoreCandidate>`                       | `Option<RestoreCandidate>`                    | `Option<RestoreCandidate>,`                      | ❌        |
| `Vec<Option<RestoreCandidate>>`                  | `Option<RestoreCandidate>` (one peel)         | `Option<RestoreCandidate>,`                      | ❌        |
| `Option<Vec<RestoreCandidate>>`                  | `Option<Vec<RestoreCandidate>>`               | `Option<Vec<RestoreCandidate>>,`                 | ❌        |
| `HashMap<String, NotificationPrefs>`             | `HashMap<String, NotificationPrefs>`          | `HashMap<String , NotificationPrefs>,`           | ❌        |
| `Option<HashMap<String, Vec<NotificationPrefs>>>`| same string                                    | same string                                      | ❌        |
| `Box<RestoreCandidate>`                          | `Box<RestoreCandidate>`                       | `Box<RestoreCandidate>,`                         | ❌        |
| `crate::schema::Foo`                             | (skipped by `contains("::")`)                 | (no entry)                                       | ✅        |
| `Vec<crate::schema::Foo>`                        | `crate::schema::Foo` (Vec stripped)           | (skipped by `contains("::")`)                    | ✅        |

Real example - `backup_data` returns `Result<Option<String>, AppError>` and `restore_from_backup` returns `Result<Option<RestoreCandidate>, AppError>`. Today's generated `transport/ipc/generated.rs`:

```rust
use crate::schema::{
    Option<RestoreCandidate>,   // ← invalid; both Option and the angle brackets break the use
    Option<String>,             // ← also invalid; String is prelude anyway
    NotificationPrefsMap,       // (this one only exists because of the OF-008/OF-010 workaround)
};
```

## After fix

Same inputs after the recursive strip:

| Service return type                              | Imports added                                  |
| ------------------------------------------------ | ---------------------------------------------- |
| `RestoreCandidate`                               | `RestoreCandidate`                             |
| `Vec<RestoreCandidate>`                          | `RestoreCandidate`                             |
| `Option<String>`                                 | (none - `String` is prelude)                   |
| `Option<RestoreCandidate>`                       | `RestoreCandidate`                             |
| `Vec<Option<RestoreCandidate>>`                  | `RestoreCandidate`                             |
| `Option<Vec<RestoreCandidate>>`                  | `RestoreCandidate`                             |
| `HashMap<String, NotificationPrefs>`             | `NotificationPrefs`                            |
| `Option<HashMap<String, Vec<NotificationPrefs>>>`| `NotificationPrefs`                            |
| `Box<RestoreCandidate>`                          | `RestoreCandidate`                             |
| `HashMap<MyKey, MyValue>`                        | `MyKey`, `MyValue`                             |
| `(MyType, OtherType)`                            | `MyType`, `OtherType` (open Q - tuples used?) |

## Algorithm sketch

A recursive walker, working on the string form (cheap and self-contained):

```rust
/// Walk a Rust return-type string and append any identifier names that
/// need an explicit `use` to `imports`. Skips prelude/std, qualified
/// paths (`a::B`), primitives, and the type wrappers themselves.
pub fn collect_type_import(ty: &str, imports: &mut Vec<String>) {
    let ty = ty.trim();
    if ty.is_empty() || ty == "()" { return; }

    // Peel references: &T, &mut T
    if let Some(rest) = ty.strip_prefix('&') {
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        return collect_type_import(rest.trim(), imports);
    }

    // Single-arg wrappers we want to peel:
    //   Option, Vec, Box, Arc, Rc, Cow (and dyn-ignored)
    // Multi-arg wrappers we want to recurse into each arg:
    //   HashMap, BTreeMap, HashSet, BTreeSet, IndexMap, Result (defensive)
    if let Some((head, args)) = split_generic(ty) {
        if KNOWN_CONTAINERS.contains(&head) {
            for arg in split_generic_args(args) {
                collect_type_import(arg, imports);
            }
            return;
        }
        // Unknown generic head: walk the args defensively, but do not
        // try to import `head` itself - it's almost always a std/prelude
        // container we don't know about yet.
        for arg in split_generic_args(args) {
            collect_type_import(arg, imports);
        }
        return;
    }

    // Qualified path - handled by entity-import path elsewhere.
    if ty.contains("::") { return; }

    // Primitives & prelude scalars - already covered by current matches!.
    if is_prelude_scalar(ty) { return; }

    if !imports.contains(&ty.to_string()) {
        imports.push(ty.to_string());
    }
}

/// Split `Outer<Inner1, Inner2>` into `("Outer", "Inner1, Inner2")`.
/// Returns None if not a generic.
fn split_generic(ty: &str) -> Option<(&str, &str)> { ... }

/// Comma-split a generic-arg list at depth 0 only:
///   `String, Vec<T>` → ["String", "Vec<T>"]
///   `HashMap<A, B>, C` → ["HashMap<A, B>", "C"]
fn split_generic_args(args: &str) -> Vec<&str> { ... }
```

The current `inner_type` function can either:
- become a thin wrapper that strips a single layer (kept for callers that genuinely want "the inner type for codegen" - see `ipc.rs:218` and `:501`, where `item_type = inner_type(ret_type)` is used for `PaginatedResult<{item_type}>`), or
- be split into `inner_type_for_codegen` (one-layer peel, current behaviour) and `inner_type_for_import` (recursive, used only by `collect_type_import`).

Recommendation: keep `inner_type` shaped as it is today, and make `collect_type_import` do its own recursive walk. The pagination call site genuinely wants the surface-level item type (`Vec<Foo>` → `Foo`); it does not want `Option<Vec<Foo>>` → `Foo`.

## Test matrix

Add unit tests for `collect_type_import` covering:

- `()` → no imports
- `String`, `&str`, `bool`, `i64`, `f32`, `i128` → no imports
- `MyType` → `["MyType"]`
- `&MyType`, `&mut MyType` → `["MyType"]`
- `Vec<MyType>` → `["MyType"]`
- `Option<MyType>` → `["MyType"]`
- `Option<String>` → no imports
- `Vec<Option<MyType>>` → `["MyType"]`
- `Option<Vec<MyType>>` → `["MyType"]`
- `Box<MyType>`, `Arc<MyType>`, `Rc<MyType>` → `["MyType"]`
- `HashMap<String, MyType>` → `["MyType"]`
- `HashMap<MyKey, MyValue>` → `["MyKey", "MyValue"]`
- `HashMap<String, Vec<MyType>>` → `["MyType"]`
- `BTreeMap<String, MyType>` → `["MyType"]`
- `HashSet<MyType>`, `BTreeSet<MyType>` → `["MyType"]`
- `Option<HashMap<String, Vec<MyType>>>` → `["MyType"]`
- `crate::schema::Foo` → no imports (qualified path)
- `Vec<crate::schema::Foo>` → no imports
- `Option<crate::schema::Foo>` → no imports

Add snapshot coverage on the IPC/HTTP/MCP generators with a service module that returns each of: `Option<String>`, `Option<Vec<T>>`, `HashMap<String, T>`, and a nested `Option<HashMap<String, Vec<T>>>`. Existing snapshot infrastructure lives in `src/snapshots/`.

## Proposed resolution

1. Add `split_generic` / `split_generic_args` helpers in `src/servers/types.rs` (depth-aware so nested commas don't break).
2. Rewrite `collect_type_import` as the recursive walker above. Leave `inner_type` alone (the codegen call sites depend on its current one-layer behaviour).
3. Maintain a `KNOWN_CONTAINERS` set: `{Option, Vec, Box, Arc, Rc, Cow, Result, HashMap, BTreeMap, HashSet, BTreeSet, IndexMap, IndexSet}`. Unknown generics still recurse into their args (defensive) but never add the head itself.
4. Extend the prelude/skip set to include `str`, `PathBuf`, `Path` (return-by-value of `Path` is rare but harmless to skip).

## Effort

Small. ~80-120 LOC including helpers and unit tests; ~30 LOC for the snapshot test fixture.

## Open questions

- **Tuples in return types?** `(MyType, OtherType)` - audit existing service functions before deciding. If never used, skip; if used, parse `(...)` similarly to a generic.
- **Trait objects?** `Box<dyn Trait>` - probably not in DTO returns. If hit, the walker would currently try to import `dyn Trait`, which is wrong. Cheap to guard against: if an arg starts with `dyn ` or `impl `, skip it.
- **String-level walker vs syn AST?** The string-level approach above is self-contained and matches the existing `String` shape of `ApiFn::return_type`. Switching to carrying a `syn::Type` (or both) would be cleaner long-term but is a bigger IR change. Stick with strings for this fix; revisit if a third bug-class shows up in the same area.

## Notes

- The TS emitter (`rust_type_to_ts` at `src/servers/types.rs:136-170`) already handles `Option<T>` and `Vec<T>` recursively. Its structure is a good reference for the new walker.
- Current Pumice workaround (wrap maps/options in named structs - `BackupOutcome { saved_path: Option<String> }`, `NotificationPrefsMap { entries: HashMap<…> }`) is documentable but should be retired from any guide after the fix lands.
