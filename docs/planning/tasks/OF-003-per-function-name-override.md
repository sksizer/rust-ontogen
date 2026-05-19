---
schema_version: '0'
status: closed/done
completion_note: "Shipped in ef63a0d on 2026-05-12."
---
# OF-003 - Per-function command-name override

- **Severity:** Medium
- **Status:** Resolved (`ef63a0d`, 2026-05-12)
- **Source:** [feedback.md OF-003](2026-05-12-pumice.md)

## Resolution

Shipped in `ef63a0d` (feat) + `3943b17` (site docs) on 2026-05-12. Both declaration mechanisms in the original plan landed:

- **Source-side:** new `#[ontogen(rename = "...")]` proc-macro attribute. `ontogen-macros` gained a no-op `#[proc_macro_attribute] pub fn ontogen` (sibling of the existing OF-007 `stateless`); the main `ontogen` crate re-exports it (`pub use ontogen_macros::{OntologyEntity, ontogen, stateless};`) so users write `use ontogen::ontogen;` then `#[ontogen(rename = "tag_get_history")]` on a function.
- **Config-side:** new `NamingConfig::command_overrides: HashMap<String, String>` keyed by `"module::fn_name"`, ORed onto the parsed IR by a post-parse `parse::apply_command_overrides(...)` overlay (parallel to OF-004's `apply_singleton_overlay`).

Both feed the same `ApiFn::command_override: Option<String>` IR field. The IPC generator's `command_name()` returns the override verbatim when present, else falls back to the default `{entity}_{fn_name}` scheme. The TS HTTP client picks up the rename automatically through the shared `command_name` import (camelCased).

**Precedence: source wins.** If both an attribute and a matching config entry exist for the same function, the attribute is kept and the config entry is silently ignored. The config map is treated as an escape hatch for cases where the source can't be modified.

**Malformed values are surfaced through OF-001's `SkipRecord` plumbing.** A new `SkipReason::InvalidRenameValue` variant covers `#[ontogen(rename = 42)]` and similar — the function is dropped and a `cargo:warning=...` is emitted via the existing diagnostic path.

**Out of scope (intentional):**
- HTTP route paths stay module-driven (`derive_action` unchanged). If you want a different URL prefix, move the function to the right module.
- The underlying Rust fn name (`journal::get_tag_history`) is untouched.
- Query/body struct names in `http.rs` still derive from `f.name`.
- Event names (`EventFn`) are a separate IR with their own naming function; out of scope for v1.

**Latent bug fixed in passing:** `ts_client::generate_generic_ts_handler` previously derived its TS method name as `snake_to_camel(&f.name)` instead of routing through `command_name(...)`. Custom GET/POST handlers therefore bypassed any rename. Fixed alongside.

Test coverage in `src/servers/tests.rs` (11 cases): attribute parsing (valid, absent, unknown directive, malformed value → drop), config overlay (populate + source-wins precedence), `command_name` resolution (override path + fallback path), and end-to-end IPC handler name, IPC handler body still calls the original Rust path, and TS client method camel-cases the override.

Site docs: new "Renaming a command" subsection in `guides/api-layer.mdx`; new `command_overrides` row in the `NamingConfig` table in `reference/configuration.mdx`; new `command_override` row in the `ApiFnMeta` table in `reference/intermediate-representations.mdx`.

**IR fidelity (follow-up, post-merge):** `ontogen_core::ir::ApiFnMeta` now carries `command_override: Option<String>`, populated from `parse::ApiFn::command_override` in `convert_scanned_fn`. The five generated CRUD construction sites and `convert_scanned_event` set it to `None`. This closes the gap that was originally noted as out-of-scope and matches the pattern OF-007 established in `6940a72` for `is_stateless`.

---

*The remainder of this document is preserved as a record of the original analysis.*

## Problem

Every emitted IPC command / TS method name is prefixed with `{url_singular(module)}_`. This is correct when the module name is the noun the operations are *about*, but when the function name already encodes the noun, the result is redundant:

```
journal::get_tag_history   →   journalGetTagHistory()   // worse
                           ←   tagGetHistory()           // better
```

The only escape hatch today is to rename either the function (`get_history` instead of `get_tag_history`) or fragment the module structure (`tag.rs` purely so its prefix reads cleanly).

## Location

- `src/servers/generators/ipc.rs:76` (`command_name`) - hard-codes `format!("{entity}_{fn_name}")`.
- TS client generator consumes the same name (snake-to-camel converted) via `command_name` import at `src/servers/generators/ts_client.rs:13`, so a single fix at the IPC layer propagates to TS automatically.

## Proposed resolution

Two declaration mechanisms feed a new `ApiFn::command_override: Option<String>` IR field. Downstream generators that build IPC command / TS method names read the override directly; everything else (HTTP route paths, Rust fn names, struct names for query/body types) is left untouched.

### Declaration mechanisms

**1. Source-side: `#[ontogen(rename = "...")]` proc-macro attribute.**

A new no-op `#[proc_macro_attribute]` lives in the existing `ontogen-macros` crate (alongside the current `derive(OntologyEntity)`) and is re-exported from the main `ontogen` crate. Users write:

```rust
use ontogen::ontogen;

#[ontogen(rename = "tag_get_history")]
pub fn get_tag_history(store: &Store, tag: &str) -> Result<Vec<HistoryEntry>, Error> {
    // ... unchanged
}
```

The attribute is a pass-through at compile time — it exists only so the source is legal Rust and rust-analyzer / rustdoc see it. The build script parses the attribute via `syn` (walking `func.attrs`) and stores the value on `ApiFn::command_override`. This mirrors the `#[serde(rename = "...")]` pattern exactly.

The same `#[ontogen(...)]` umbrella will host future per-fn directives (candidates: OF-007 stateless markers). Today only `rename` is recognized.

**2. Config-side: `NamingConfig::command_overrides`.**

For cases where the source can't be modified, or for batch overrides across many functions:

```rust
let config = Config {
    naming: NamingConfig {
        command_overrides: HashMap::from([
            ("journal::get_tag_history".to_string(), "tag_get_history".to_string()),
        ]),
        ..Default::default()
    },
    ..Default::default()
};
```

Key format: `"module::fn_name"`. An `apply_command_overrides()` step (parallel to OF-004's `apply_singleton_overlay`) runs post-parse and writes the value onto `ApiFn::command_override` for any matching entry.

**Precedence: source wins.** If both an attribute and a config entry are set for the same function, the attribute takes effect and the config entry is silently ignored. Reasoning: the attribute is co-located with the function, so a reader looking at the source has the full naming story. The config map is an escape hatch.

### Generated code: before and after

For a function defined as:

```rust
// api/v1/journal.rs
#[ontogen(rename = "tag_get_history")]
pub fn get_tag_history(store: &Store, tag: &str) -> Result<Vec<HistoryEntry>, Error> { ... }
```

**Rust IPC handler** (in `target/.../ipc_generated.rs`):

```rust
// BEFORE
#[tauri::command]
pub async fn journal_get_tag_history(
    tag: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<HistoryEntry>, String> {
    let store = JournalStore::new(&state).await;
    journal::get_tag_history(&store, &tag).map_err(|e| e.to_string())
}

// AFTER
#[tauri::command]
pub async fn tag_get_history(                    // ← override replaces command name
    tag: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<HistoryEntry>, String> {
    let store = JournalStore::new(&state).await;
    journal::get_tag_history(&store, &tag).map_err(|e| e.to_string())
    //       ^^^^^^^^^^^^^^^ underlying Rust path unchanged
}
```

**Tauri `invoke()` call site** (user-written TS):

```typescript
// BEFORE
await invoke('journal_get_tag_history', { tag: 'rust' });

// AFTER
await invoke('tag_get_history', { tag: 'rust' });
```

**TS HTTP client method** (in `frontend/src/http-client.ts`):

```typescript
// BEFORE
export const httpCommands = {
    async journalGetTagHistory(tag: string): Promise<HistoryEntry[]> {
        return httpGet(`/journals/tag-history?tag=${encodeURIComponent(tag)}`);
    },
    // ...
};

// AFTER
export const httpCommands = {
    async tagGetHistory(tag: string): Promise<HistoryEntry[]> {     // ← camelCased from override
        return httpGet(`/journals/tag-history?tag=${encodeURIComponent(tag)}`);
        //              ^^^^^^^^^^^^^^^^^^^^ URL unchanged (see "Out of scope" below)
    },
    // ...
};
```

The TS HTTP method name flows automatically because `ts_client.rs:153` does `let camel = snake_to_camel(&cmd_name)` and `cmd_name` already reflects the override via the shared `command_name(...)` helper.

### Out of scope (intentional)

- **HTTP route paths** are unaffected. `derive_action(module, fn_name)` at `types.rs:529` operates on the raw function name and the module prefix follows the file location. The override is a *naming* fix for IPC/TS surfaces. If the URL is also wrong, the function probably wants to live in a different module — that's a structural change, not a directive.
- **Underlying Rust function name** (`journal::get_tag_history`) is untouched. Generated handlers still call the original symbol.
- **Query / body struct names** (`JournalGetTagHistoryQuery`, etc.) at `http.rs:577,587` still derive from `f.name`. They're internal to the generated file and never user-facing.
- **Event names** (functions returning `broadcast::Receiver<T>`, emitted via `app_handle.emit(event_name, ...)`) are emitted from a separate `EventFn` IR and don't flow through `command_name`. Out of scope for v1; can mirror this design later if it becomes a need.

### Files touched

| File | Change |
|---|---|
| `ontogen-macros/src/lib.rs` | Add `#[proc_macro_attribute] pub fn ontogen(args, input) -> TokenStream { input }` |
| `Cargo.toml` (root) | Ensure `ontogen-macros` is a dep of `ontogen` (currently used via the existing derive — re-confirm wiring) |
| `src/lib.rs` | `pub use ontogen_macros::ontogen;` |
| `src/servers/parse.rs` | Add `command_override: Option<String>` to `ApiFn`; in `parse_api_module`, walk `func.attrs` for `#[ontogen(rename = "...")]`, parse via `syn::parse2::<MetaList>`, populate the field |
| `src/servers/types.rs` | Add `command_overrides: HashMap<String, String>` to `NamingConfig` |
| `src/servers/mod.rs` | Add `apply_command_overrides(&mut [ApiModule], &NamingConfig)`; call after `apply_singleton_overlay` |
| `src/servers/generators/ipc.rs:76` | `command_name` returns `f.command_override.clone().unwrap_or_else(...)` (else branch is the current `format!("{entity}_{fn_name}")`) |
| `src/servers/tests.rs` | New section: parser cases (attr present / absent / multiple args / config overlay / source-wins-precedence) + end-to-end IPC handler test + end-to-end TS client method test |
| `site/src/content/docs/guides/api-layer.mdx` | New "Renaming a command: `#[ontogen(rename = ...)]`" section near the existing "Singleton modules" section |
| `site/src/content/docs/reference/configuration.mdx` | New `command_overrides` row in the `NamingConfig` table |

### Test coverage plan (target: ~10 tests)

- **Parser, source-side:**
  - `test_parse_rename_attribute` — `#[ontogen(rename = "X")]` populates `command_override`.
  - `test_parse_no_rename_attribute_leaves_override_none` — absence of the attr leaves the field `None`.
  - `test_parse_ontogen_attribute_without_rename_is_ignored` — `#[ontogen(other = "...")]` doesn't crash; field stays `None`.
  - `test_parse_rename_attribute_with_invalid_value_emits_skip` — non-string literal (e.g., `rename = 42`) emits a `SkipRecord` (parallel to OF-001 diagnostics) and the fn is dropped.
- **Config overlay:**
  - `test_apply_command_overrides_populates_field` — config map → `ApiFn::command_override` set.
  - `test_command_overrides_source_wins_over_config` — both set, attr value is preserved, config value is ignored.
- **End-to-end:**
  - `test_command_name_uses_override_when_set` — `command_name(...)` returns the override verbatim.
  - `test_ipc_handler_uses_override_function_name` — generated handler signature is `pub async fn tag_get_history(...)`.
  - `test_ts_client_uses_override_camelcased` — generated TS method is `async tagGetHistory()`.
  - `test_ipc_handler_calls_unchanged_rust_fn` — body still invokes the original `journal::get_tag_history`.

## Effort

Medium. New no-op attribute macro (~10 LOC), attribute parsing via `syn` (~30 LOC), IR field + config overlay (~40 LOC), single-line change in `command_name`, ~10 tests, ~80 lines of site docs.

## Notes

- The grammar for the attribute (`rename = "..."`) follows `serde`'s precedent. Future per-fn directives slot into the same `#[ontogen(...)]` umbrella.
- The file-level marker grammar (`// ontogen:skip`, `// ontogen:singleton`) stays as comments — Rust does not allow custom inner-attribute macros without unstable features, so the comment form is the only practical option for file-level flags. The two-tier story is: file-level flags via comments, item-level directives via `#[ontogen(...)]`.
- Pair with [OF-001](./OF-001-parser-skip-diagnostic.md): if the parser sees `#[ontogen(rename = ...)]` with a malformed value, the function should be dropped with a `SkipRecord` so the user gets a build-time warning rather than silent fallback to the default name.
