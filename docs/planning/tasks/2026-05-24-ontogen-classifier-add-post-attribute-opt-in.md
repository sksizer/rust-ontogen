---
type: task
schema_version: '3'
status: open/ready
created: '2026-05-24'
impact: medium
complexity: small
tags:
- attribute-opt-in
- http-method
- ontogen-classifier
- pumice-follow-up
related: []
autonomy: supervised
---
# ontogen-macros: add #[ontogen::post] to force POST classification on zero-user-param mutating handlers

## Goal

Add a `#[ontogen::post]` proc-macro attribute that consumers can place on a server-side handler function to force `CustomPost` classification regardless of the auto-classifier's verdict. This is the cheapest, most conservative cover for the "zero-user-param mutating verb routes as GET" bug — it adds an opt-in escape hatch without changing the default for any existing consumer.

## Today

`src/servers/classify.rs::classify_by_name_and_params` (line 32) walks function metadata to decide the HTTP method:

```rust
// Zero-param custom fns are always read-shaped (no body to carry).
if params.is_empty() {
    return OpKind::CustomGet;
}
```

After ontogen strips `state: &PumiceState`, action-verb functions like `engine::pause(state)`, `engine::resume(state)`, `data::backup(state)`, `data::reset_all(state)` have zero user-input params — so they fall into this branch and emit as `get(...)` routes. Pumice has 11+ such routes today (PR `sksizer/pumice#225`'s generated `transport/http/generated.rs:586-606`). The original OF-016 task spec explicitly mentioned `#[ontogen::post]` as a deferred follow-up: "Source-attribute escape hatch (`#[ontogen::post]`) deferred until a real-world repro motivates it."

| Location | Role today |
|---|---|
| `src/servers/classify.rs` | `classify_by_name_and_params()` runs the heuristic; reads no source-level attributes other than what the parser already gave it. |
| `src/servers/parse.rs` | Parses each `pub async fn` into an `ApiFn`; this is where attribute parsing lives for the existing `#[ontogen::stateless]`, `#[ontogen::rename = ...]`, `#[ontogen::skip]`, etc. |
| `crates/ontogen-macros/src/lib.rs` | Re-exports the proc-macro attributes (no-op proc-macros that the parser inspects). |

The repro: Pumice's `pub async fn pause(state: &PumiceState) -> Result<(), AppError>` emits `route("/api/engines/pause", get(engine_pause))` when it should be POST. The user has to choose between accepting GET semantics on a mutating call (incorrect per RFC 7231), renaming the function to fit one of the auto-classifier patterns (`create_pause`? unnatural), or adding bodies to function signatures purely to trigger the existing OF-016 reclassification (gross).

## Proposed

Add a no-op `#[ontogen::post]` proc-macro attribute to `ontogen-macros`. The parser recognizes the attribute on a `pub async fn` and sets a flag on the resulting `ApiFn`. `classify_by_name_and_params` (or its caller) consults the flag BEFORE running the existing heuristic — if present, the classifier returns `OpKind::CustomPost` unconditionally.

End state: Pumice annotates `pause`, `resume`, `reset`, `cancel`, `end`, `skip_break`, `stop`, `backup`, `pick_restore`, `clear_sessions`, `reset_all` with `#[ontogen::post]` and the next codegen emits POST routes. No change for any current consumer that doesn't add the attribute.

## Approach

1. **Add the proc-macro definition** in `crates/ontogen-macros/src/lib.rs` alongside `#[ontogen::stateless]` and the other existing no-op markers. Same shape: an `#[proc_macro_attribute]` that returns its input unchanged (the macro is a parsing hint, not a real expansion).
2. **Extend the AST parser** in `src/servers/parse.rs` to recognize the attribute and stamp a per-fn boolean (`force_post: bool` or similar) on the resulting `ApiFn`.
3. **Thread the flag into the classifier.** In `classify_op` (or wherever the flag is most naturally consulted — call-site at `classify_by_name_and_params` may be cleanest), return `OpKind::CustomPost` early when the flag is true.
4. **Test fixture.** Add a fixture with a function annotated `#[ontogen::post]` and zero user params; assert the emitted HTTP route uses `post(...)` not `get(...)`.
5. **Document** the attribute alongside `#[ontogen::stateless]` and the other markers in the parser's attribute table / site docs (typescript-bindings.mdx or the appropriate page).

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `crates/ontogen-macros/src/lib.rs` | modify | add the `#[proc_macro_attribute] pub fn post(...)` no-op macro. |
| `src/servers/parse.rs` | modify | recognize the attribute, stamp a `force_post` flag on `ApiFn`. |
| `src/servers/classify.rs` | modify | consult the flag at the entry point of the classifier and return `OpKind::CustomPost` early when set. |
| `src/servers/types.rs` | modify | add the `force_post: bool` (or equivalent) field to `ApiFn`. |
| `tests/` | modify | fixture function annotated `#[ontogen::post]` with zero user params; assert POST emission. |
| `site/src/content/docs/reference/` | modify | document the attribute alongside the existing `#[ontogen::stateless]` reference. |

## Acceptance criteria

- [ ] AC-1: Unit/integration test: a function annotated `#[ontogen::post]` emits as `post(...)` in the generated `entity_routes()`, regardless of whether the name or params would otherwise route to GET.
- [ ] AC-2: `cargo build` in `examples/iron-log/src-tauri/` succeeds with byte-identical generated TS/Rust — no behavioral regression on consumers that don't use the attribute.
- [ ] AC-3: Pumice (sksizer/pumice#225 follow-up): can annotate at least one currently-routing-as-GET mutating handler (e.g. `engine::pause`) with `#[ontogen::post]` and regenerated TS/Rust show the route as POST.
- [ ] AC-4: `just full-check` passes on the rust-ontogen branch.

## Out of scope

- **Changing the default classifier behavior** — that lives in `[[2026-05-24-ontogen-classifier-reverse-zero-param-default-to-post]]`. This task is the conservative, additive, never-breaks-anyone fix; the default-reversal is the principled long-term fix and can land independently.
- **A symmetric `#[ontogen::get]` attribute** — possibly useful but no real-world repro today. File a follow-up if a consumer surfaces a need.
- **Method-specific attributes** for PUT, DELETE, PATCH — same shape (`#[ontogen::put]` etc.) is plausible but again deferred until motivated. Today named-CRUD (`update`, `delete`) covers most cases.

## Dependencies

- none. Pure additive feature.

## Discovery context

- Surfaced by sksizer/pumice#225's inline review comment from sksizer at `src-tauri/src/api/transport/http/generated.rs:590`: "a lot of these should probably be posts since they mutate data not gets. what are the ontogen generation rules in that regard?"
- The OF-016 task spec (2026-05-14 work, merged via rust-ontogen #b2f882c) explicitly deferred this attribute: "Source-attribute escape hatch (`#[ontogen::post]`) deferred until a real-world repro motivates it." Pumice #225 is the motivating repro.
- Companion to `[[2026-05-24-ontogen-classifier-reverse-zero-param-default-to-post]]`: this task is the user-driven opt-in; that task changes the default. Either ships alone or both ship together.
