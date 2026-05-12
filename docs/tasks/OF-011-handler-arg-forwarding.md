# OF-011 - Make handler argument forwarding consistent and fix `.as_deref()` for non-Deref `Option<T>`

- **Severity:** High
- **Status:** Open (design decision needed)
- **Source:** [feedback.md OF-011](../feedback.md)

## Problem

The IPC generator's decision between by-value and by-ref forwarding for handler arguments depends on type-name shape ("does the type name contain `Input`?") rather than the user's actual parameter signature. Six observed cases:

| User-function param                                  | Handler passes      | Note               |
| ---------------------------------------------------- | ------------------- | ------------------ |
| `text: &str`                                         | `&text`             | OK (deref-coerces) |
| `enabled: bool`                                      | `enabled` (by value)| OK                 |
| `id: String`                                         | `&id`               | OK                 |
| `input: ProfileInput` (custom, short name)           | `input` (by value)  | Triggered by `Input` substring |
| `prefs: crate::schema::NotificationPrefs` (qualified path) | `&prefs`        | OK                 |
| `profile_id: SelectedProfileId` (custom, short name) | `&profile_id`       | OK by accident     |
| `rating: Option<u8>`                                 | `rating.as_deref()` | **Broken** - `u8: !Deref` |

The substring-on-type-name heuristic is fragile, and `.as_deref()` is emitted for *every* `Option<T>` regardless of whether `T: Deref`.

## Location

- `src/servers/generators/ipc.rs:472-485` (`generate_generic_ipc_handler`)
- `src/servers/generators/ipc.rs:530-543` (`generate_paginated_ipc_handler`)
- The HTTP generator likely has parallel logic - audit `src/servers/generators/http.rs` while fixing.

## Current behavior

```rust
// User wrote:
pub async fn rate(state: &PumiceState, rating: Option<u8>) -> Result<(), AppError> { ... }

// Generator emits:
service::rate(&state, rating.as_deref())
//                          ^^^^^^^^^ fails to compile - u8 has no Deref
```

## Proposed resolution

**Design call required.** The current code is heuristic-based (a) and broken; choose one of:

A. **Mirror the user's signature literally.** During generation, read the `Param.ty` string verbatim. If it starts with `&`, pass `&name`; otherwise pass `name`. Drop the `Input` substring check entirely. For `Option<T>`: only emit `.as_deref()` when `T` is in a known-Deref allowlist (`String`, `Vec<_>`, `Box<_>`, `PathBuf`, `Rc<_>`, `Arc<_>`); otherwise pass `name` by value.

B. **Standardize on owned-only.** Require all API function parameters to be owned (`T`, not `&T`). Generator always passes by value. Strictest, simplest, but breaks every existing site that uses `&str` / `&Type`.

C. **Standardize on ref-only.** Require all parameters to be `&T`. Generator always passes by ref. Equally strict in the other direction.

Recommendation: **A**. The user's signature is already the source of truth for the parsed types; the generator should respect it instead of overriding with type-name heuristics. The `Option<T>` Deref allowlist is small enough to maintain in one place.

## Effort

Medium-plus. The forwarding logic is duplicated across IPC handler shapes (generic, paginated, possibly HTTP). Need a single helper function that returns the forwarded expression for a given `Param`, used everywhere. Snapshot coverage for each combination (`T`, `&T`, `Option<T>`, `Option<&T>`, and the seven cases above).

## Notes

- The `.as_deref()` bug is a strict regression. Even if (A) is not adopted, the Deref allowlist should land as a hotfix.
- Tests should include:
  - `rating: Option<u8>` → `rating` (by value)
  - `name: Option<String>` → `name.as_deref()`
  - `path: Option<PathBuf>` → `path.as_deref()`
  - `tags: Option<Vec<String>>` → `tags.as_deref()`
- Worth checking whether the same heuristics drive HTTP body / query parameter binding - if so, fold the audit into one PR.
