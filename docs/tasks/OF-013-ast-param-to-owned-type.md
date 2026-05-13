---
status: in-progress
---
# OF-013 - AST-ify `param_to_owned_type` so unsized-DST inner types yield correct owned forms

- **Severity:** Medium (latent today; produces uncompileable handler param types once exercised)
- **Status:** In progress
- **Discovered:** During OF-011 implementation (commit `86045a5`); not from Pumice feedback.
- **Related:** [OF-008](./OF-008-inner-type-strip-option.md) (introduced the AST groundwork), [OF-011](./OF-011-handler-arg-forwarding.md) (forwarding fix that surfaced this gap)

## Problem

`param_to_owned_type` decides the owned form of a service-fn parameter type for use as the handler's declared parameter type. It's still string-based (unlike `collect_type_import` and `forward_arg_expr`, which now walk `syn::Type`) and only knows the `&str -> String` transformation. Other unsized DSTs that show up as `&T` in service signatures - slices, paths, C/OS strings - are stripped of the `&` but left in their unsized form, which is invalid as an owned parameter type.

OF-011's `forward_arg_expr` already identifies these cases and emits `.as_deref()` correctly. But the handler parameter declaration (still produced by `param_to_owned_type`) is malformed for the same shapes. Result: forwarding side says "this is `.as_deref()`-able", declaration side says "the param type is `[u8]`" - the latter does not compile.

## Location

- `src/servers/types.rs:104-116` (`param_to_owned_type`).

## Current behavior

| User wrote | `param_to_owned_type` produces | Compiles as a param type? |
|---|---|---|
| `&str` | `String` | yes |
| `&MyStruct` | `MyStruct` | yes |
| `Option<&str>` | `Option<String>` | yes |
| `Option<&MyStruct>` | `Option<MyStruct>` | yes |
| `Option<&[u8]>` | `Option<[u8]>` | **no** - `[u8]` is unsized |
| `Option<&Path>` | `Option<Path>` | **no** - `Path` is unsized |
| `Option<&CStr>` | `Option<CStr>` | **no** - `CStr` is unsized |
| `Option<&OsStr>` | `Option<OsStr>` | **no** - `OsStr` is unsized |
| `&[u8]` | `[u8]` | **no** |
| `&Path` | `Path` | **no** |

## Proposed resolution

AST-ify `param_to_owned_type` the same way `collect_type_import` was AST-ified in commit `9b115a4`. Take `&syn::Type`, return the rendered owned form (`String`). Mirror the Deref allowlist that `forward_arg_expr` (in `src/servers/types.rs`) already uses, mapping unsized-DST refs to their owned counterparts:

| User wrote | Owned form |
|---|---|
| `&str` | `String` |
| `&[T]` | `Vec<T>` |
| `&Path` | `PathBuf` |
| `&CStr` | `CString` |
| `&OsStr` | `OsString` |
| `&mut T` | (same as `&T`) |
| `&T` (any other) | `T` |
| `Option<U>` | `Option<owned(U)>` (recurse) |
| `Vec<U>` | `Vec<owned(U)>` (open question - see below) |
| anything else | as-is |

The change is a single Rust file. Re-use the helpers introduced for `forward_arg_expr` (`last_segment_is`, `option_inner`, the Deref-target check) - if a third call site for the same allowlist makes a shared abstraction worth extracting, do it then.

## Tests

Unit tests in `src/servers/types.rs` using `syn::parse_quote!`:

| User type | Expected owned form |
|---|---|
| `String` | `String` |
| `&str` | `String` |
| `MyStruct` | `MyStruct` |
| `&MyStruct` | `MyStruct` |
| `&[u8]` | `Vec<u8>` |
| `&[MyStruct]` | `Vec<MyStruct>` |
| `&Path` | `PathBuf` |
| `&CStr` | `CString` |
| `&OsStr` | `OsString` |
| `Option<&str>` | `Option<String>` |
| `Option<&[u8]>` | `Option<Vec<u8>>` |
| `Option<&Path>` | `Option<PathBuf>` |
| `Option<&MyStruct>` | `Option<MyStruct>` |
| `Option<String>` | `Option<String>` |
| `Option<u8>` | `Option<u8>` |

Add an end-to-end test that runs the parser through `ipc::generate` for a service fn with `Option<&[u8]>` and asserts the generated handler param renders as `Option<Vec<u8>>` (and that the forwarded call expression renders as `<name>.as_deref()` - the consistency check).

## Effort

Small. Comparable to the OF-008 fix that AST-ified `collect_type_import`. Most of the helper plumbing in `src/servers/types.rs` (`last_segment_is`, `option_inner`, the Deref-target allowlist) already exists from OF-008 and OF-011 - factor into a shared module if the third use justifies it.

## Notes

- **Public API change.** `param_to_owned_type` is currently `pub fn(&str) -> String`. AST-ifying it to `pub fn(&syn::Type) -> String` is a breaking change of the same shape as the one in `9b115a4`. Bundle this with the next release as another `BREAKING CHANGE:` footer (or fold into a wider AST-migration changelog entry if more functions get the same treatment).
- **Iron-log doesn't hit this today.** The bug surfaces the first time a downstream service signature uses `&[u8]`, `&Path`, `&CStr`, or `&OsStr` (with or without `Option<>` around them).
- **Stay consistent with `forward_arg_expr`'s allowlist.** Any `&T` that the forwarding helper handles via `.as_deref()` must have a sensible owned form here, and vice versa. If the two lists diverge, the generated handler will either fail to compile (this side incomplete) or forward incorrectly (forwarding side incomplete). Tests on both sides should cover the same matrix.

## Open questions

- **`Vec<&str>`-shaped parameters?** Does the owned form recurse into the `Vec<T>` arg? Likely yes (mapping `Vec<&str>` -> `Vec<String>`), but verify there's a real use case before adding the rule. Tauri's IPC layer serializes `Vec<String>` cleanly via serde.
- **`Cow<'_, T>`?** Out of scope until a real use surfaces.
- **Slice arrays (`&[T; N]`)?** Probably emit `Vec<T>` (the length is lost on owned conversion). Out of scope until needed.
