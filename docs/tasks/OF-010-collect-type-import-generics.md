# OF-010 - `collect_type_import` should recurse into multi-arg generics

- **Severity:** High (produces broken output; sibling of OF-008)
- **Status:** In progress
- **Source:** [feedback.md OF-010](../feedback.md)
- **Related:** [OF-008](./OF-008-inner-type-strip-option.md) (same fix)

## Problem

`inner_type` strips `Result` and `Vec` but doesn't handle parameterized generics like `HashMap<K, V>`. The fully rendered type string ends up in the import list.

## Location

- `src/servers/types.rs:52` (`inner_type`) - same function that misses `Option<T>` per [OF-008](./OF-008-inner-type-strip-option.md).

## Current behavior

```rust
pub async fn get_prefs(state: &PumiceState)
    -> Result<HashMap<String, NotificationPrefs>, AppError> { ... }
```

Generated:

```rust
use crate::schema::{
    HashMap<String , NotificationPrefs>,  // ← invalid
};
```

(Pumice reported 54 resulting compile errors when this surfaced in the notification module.)

## Proposed resolution

Same fix as [OF-008](./OF-008-inner-type-strip-option.md): change `inner_type` (or split into `inner_type_for_import` / `inner_type_for_ts`) to walk the type recursively, collecting *all* base type identifiers found in generic arguments rather than emitting the rendered text. For `HashMap<String, NotificationPrefs>`, the collector should yield `NotificationPrefs` (and skip `String` and `HashMap` as prelude / std types).

One PR can close both OF-008 and OF-010.

## Effort

Folded into [OF-008](./OF-008-inner-type-strip-option.md).

## Notes

- Current workaround (wrap the map in a struct, e.g., `NotificationPrefsMap { entries: HashMap<…> }`) mirrors the OF-008 workaround. Both go away once `inner_type` is recursive.
- The `BTreeMap`, `IndexMap`, `HashSet`, `BTreeSet` variants likely have the same issue. Cover them in the test matrix.
