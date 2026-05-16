---
status: closed/done
completion_note: "Shipped in 919b74a on 2026-05-12."
---
# OF-001 - Emit diagnostic when parser skips a non-matching `pub fn`

- **Severity:** High
- **Status:** Resolved (`919b74a`, 2026-05-12)
- **Source:** [feedback.md OF-001](2026-05-12-pumice.md)
- **Related:** [OF-005](./OF-005-document-state-store-shapes.md) (shipped together)

## Resolution

Shipped in `919b74a` on 2026-05-12 alongside OF-005. Took the breaking-signature
path discussed in the Open Questions: `scan_api_dir` and `parse_api_module` now
return result structs that carry skip records alongside the parsed modules.

- `parse_api_module: ... -> Option<ApiModule>` became `... -> ModuleParseResult`.
- `scan_api_dir: ... -> Vec<ApiModule>` became `... -> ScanResult`.
- New pub types in `src/servers/parse.rs`: `SkipRecord`, `SkipReason`
  (`FirstParamMismatch { first_param_ty, state_type, store_type }`,
  `SelfReceiver`, `NoParams`), `ModuleParseResult`, `ScanResult`.
- Both library call sites (`gen_api` in `src/api/mod.rs:79`, `generate_transport`
  in `src/servers/mod.rs:221`) drain `scan_result.skips` and `println!` one
  `cargo:warning=` line per record via `SkipRecord`'s `Display` impl.
- Eight `scan_api_dir` test call sites in `src/servers/tests.rs` get `.modules`
  appended to preserve the previous shape.
- 12 new tests covering each `SkipReason` variant, the four
  "out-of-scope" cases that must *not* produce a record (private fn, event fn,
  etc.), the `Display` formatting, and the OF-005 acceptance-table rows.

The change is not a public-API break: `pub(crate) mod parse;` (servers/mod.rs:13)
keeps these signatures crate-internal. Only `ApiFn`, `ApiModule`, `EventFn`, and
`Param` are re-exported. External callers see a behaviour change (warnings now
appear) but no surface change.

User-facing docs updated:
- `site/src/content/docs/guides/api-layer.mdx` — new "Service functions:
  accepted signatures" table + "Build-time skip warnings" section.
- `site/src/content/docs/cookbook/custom-api-endpoints.mdx` — added a caution
  callout explaining the cargo warning.
- `site/src/content/docs/reference/configuration.mdx` — sharpened `state_type`
  and `store_type` field descriptions; added a paragraph about skip warnings
  to "How scanning works".

---

*The remainder of this document is preserved as a record of the original analysis.*

## Problem

The API parser accepts a `pub fn` only when the rendered type string of its first parameter contains the configured `state_type` (or `store_type`) as a substring. Non-matching functions are silently dropped: no warning, no diagnostic, build still succeeds, but no IPC handler / HTTP route / TS method is emitted. Downstream consumers have nothing to call against and no signal that anything is missing.

## Location

- `src/servers/parse.rs:96-188` (`parse_api_module`) - the `if !is_accepted { continue; }` skip at line 128 has no logging.
- `src/servers/parse.rs:223-239` (`scan_api_dir`) - aggregates `parse_api_module` results across a directory; any per-file skip records have to flow back through here.
- Two `scan_api_dir` callers in the public lib surface:
  - `src/api/mod.rs:79` inside `gen_api`
  - `src/servers/mod.rs:221` inside `generate_transport` (called via `gen_servers`)
- 8 call sites in `src/servers/tests.rs` (every `let modules = scan_api_dir(...)` block).

## Current behavior

```rust
// api/v1/foo.rs - silently invisible to ontogen
pub fn x(state: tauri::State<'_, Mutex<AppState>>) -> Result<(), AppError> { ... }
```

Because `State<'_, Mutex<AppState>>` does not contain the configured `state_type` substring (e.g., `PumiceState`), the function is dropped without any output.

## Drop cases that produce a SkipRecord

The parser's `parse_api_module` loop has three silent-drop paths today. A SkipRecord is emitted for each. All three share the same shape: the user wrote `pub fn` in an `api/v1/*.rs` file (so they clearly intended an API endpoint), but the function never made it into `ApiModule.functions` and no downstream artifact (IPC handler, HTTP route, TS method) was generated.

1. **First-param substring mismatch** - `parse.rs:128 if !is_accepted { continue; }`. The fn has a typed first parameter, but its normalized type string doesn't contain `state_type` or `store_type` as a substring.
   ```rust
   // state_type = "PumiceState", store_type = Some("Store")
   pub fn rename(state: tauri::State<'_, Mutex<AppState>>, name: String) -> Result<(), AppError> { ... }
   //            ^ normalized to "tauri::State<'_,Mutex<AppState>>" - neither substring matches
   ```

2. **First param is a receiver (`self` / `&self`)** - `parse.rs:115-127` matches only `FnArg::Typed`; `FnArg::Receiver` falls into the `_ => (false, false)` arm and the fn is dropped. This shouldn't happen in a free-fn API module, but if a user accidentally writes a method-shaped fn it currently disappears.
   ```rust
   pub fn helper(&self, id: String) -> Result<(), AppError> { ... }
   ```

3. **Zero parameters** - `parse.rs:114 func.sig.inputs.first()` returns `None` and lands in the same `_ => (false, false)` arm. Covers the OF-007 footgun (stateless utility fns silently invisible).
   ```rust
   pub fn cache_clear() -> Result<(), AppError> { ... }
   ```

## Out of scope (no SkipRecord)

Cases the parser also drops, but where a warning would be noise rather than signal:

- **Private fns** (`!matches!(func.vis, Visibility::Public(_))` at `parse.rs:109`). Privacy is the user's explicit signal that the fn is internal.
- **Non-`fn` items** (`if let syn::Item::Fn(func)` at `parse.rs:108`) - `struct`, `enum`, `use`, etc.
- **`mod.rs` files** (`parse.rs:98`) and **`_impl.rs` files** (`is_scannable_rs_file` at `parse.rs:268-271`). Explicit organizational skips.
- **Files that fail `syn::parse_file`** (`parse.rs:103`). Corrupted source is a different bug class; arguably a hard error, but not OF-001's scope.
- **Accepted fns reclassified as events** (`is_receiver_return_type` at `parse.rs:148`). Those become `EventFn`s; nothing was actually skipped.

## Proposed resolution

Concrete API shape:

```rust
// src/servers/parse.rs

pub struct SkipRecord {
    pub file: PathBuf,
    pub fn_name: String,
    pub reason: SkipReason,
}

pub enum SkipReason {
    /// First-param type didn't contain state_type or store_type.
    FirstParamMismatch {
        first_param_ty: String,
        state_type: String,
        store_type: Option<String>,
    },
    /// First param was `self` or `&self`.
    SelfReceiver,
    /// Function had no parameters at all.
    NoParams,
}
```

Threading the records out of the parser:

1. **`parse_api_module` and `scan_api_dir` return shape** - decision pending (see Open questions). Either:
   - Breaking: return `ScanResult { modules, skips }` from `scan_api_dir` and a parallel struct from `parse_api_module`.
   - Non-breaking: take `&mut Vec<SkipRecord>` out-param.
2. **Emit at the `gen_api` / `generate_transport` boundary** (not inside `Pipeline::build`). Both are public entry points reachable directly from a `build.rs`. Pipeline just delegates; placing the print there would mean direct callers still see silent drops.
3. **Warning format** (one line per skip, `println!("cargo:warning=...")`):
   ```
   ontogen: skipped fn `foo::rename` in `src/api/v1/foo.rs` -
     first param `tauri::State<'_, Mutex<AppState>>` does not match
     state_type 'PumiceState' or store_type 'Store'
   ontogen: skipped fn `foo::cache_clear` in `src/api/v1/foo.rs` - fn has no parameters
   ontogen: skipped fn `foo::helper` in `src/api/v1/foo.rs` - first param is `self`/`&self`
   ```
4. **Optional (deferred)**: a `strict` flag on `ApiConfig` / `ServersConfig` that escalates skip records to a hard error. Not in the initial PR unless it falls out cheaply.

Pair this with the docs page from [OF-005](./OF-005-document-state-store-shapes.md) so the diagnostic text and the documented "accepted signatures" table reference each other - a user hitting the warning lands on the table; a user reading the table sees the warning they'll get.

## Effort

Small-medium. Roughly:
- ~30 LOC for `SkipRecord` / `SkipReason` and threading them out of the parser.
- ~20 LOC at each of the two emit sites (`gen_api`, `generate_transport`).
- ~20 LOC updating the 8 `scan_api_dir` test call sites.
- ~40 LOC of new unit tests covering each `SkipReason` variant and the no-skip cases listed in "Out of scope".

## Open questions

- **Return-shape decision** - struct (`ScanResult`) vs out-param. The codebase already broke pub API surface for OF-008/010 in the same release window, so the breaking-change cost is low. Leaning struct for extensibility (new fields can be added without further breaking).
- **Should a substring false-positive accept be warned too?** With `store_type = "Store"`, a fn taking `&StoreContext` is currently *accepted* as a store handler because `"Store"` is a substring of `"StoreContext"`. The OF-005 table will document this; whether OF-001 also emits an "INFO: matched-by-substring" warning is a scope call.

## Notes

- The parser is library code; it must not call `println!` directly. `cargo:warning=` emission lives at the `gen_api` / `generate_transport` boundaries.
- The OF-007 stateless-fn footgun is covered by the `NoParams` variant. If/when OF-007 ships (allow zero-param fns as first-class), that variant goes away or becomes opt-in.
