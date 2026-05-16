---
status: closed/done
completion_note: "Shipped in b2f882c on 2026-05-14."
---
# OF-016 - `get_*` classifier ignores param shape, forces HTTP GET + `Path<String>`

- **Severity:** Medium
- **Status:** Resolved (`b2f882c`, 2026-05-14)
- **Source:** Pumice feedback, [`docs/feedback/2026-05-14-pumice.md`](../feedback/2026-05-14-pumice.md). Filed in the consumer's log as "OF-013" (their numbering); upstream uses OF-016 to avoid collision with the already-resolved [OF-013](./OF-013-ast-param-to-owned-type.md).
- **Related:** [OF-008](./OF-008-inner-type-strip-option.md) / [OF-010](./OF-010-collect-type-import-generics.md) (AST-ified `inner_type` / `collect_type_import`), [OF-011](./OF-011-handler-arg-forwarding.md) (AST-driven `forward_arg_expr`), [OF-013](./OF-013-ast-param-to-owned-type.md) (AST-ified `param_to_owned_type`), [OF-017](./OF-017-param-import-substring-gate.md) (dropped the `Input`/`Query` param-import gate). This ticket extends the same name → AST migration to the operation classifier — the last remaining name-driven decision in the pipeline.

## Resolution

Shipped in `b2f882c` on 2026-05-14. Site docs updated in `fe237b1`.

`classify_by_name_and_params` in `src/servers/classify.rs` now consults
the AST of the first user-facing parameter for `get_*` functions:

- First param is a single-segment ident in the id-like primitive
  allowlist (`bool`, `char`, `i8`..`i128`, `isize`, `u8`..`u128`,
  `usize`, `f32`, `f64`, `String`, `str`, `Uuid`) → `CustomGet`
  (path-extractable, current behavior preserved).
- First param is `Option<…>` → `CustomGet` (query-extractable, current
  behavior preserved).
- First param is anything else (custom struct, qualified path,
  `Vec<T>`, `HashMap<K, V>`, …) → `CustomPost` (body-carrying — HTTP
  generator emits a `POST` route with JSON body extraction).
- Zero-param `get_*` → `CustomGet` (no body to carry, current behavior
  preserved).

While in the same area, the name-based `is_read_operation(name: &str)
-> bool` helper was replaced with `is_read_op(op: &OpKind) -> bool`.
Single source of truth: the classifier decides once; downstream code
checks the classified result. Updated six callers across `mod.rs`,
`generators/http.rs`, `generators/transport.rs`, and
`generators/ts_client.rs`. This closes a pre-existing latent
divergence: the old `is_read_operation` would have returned `true`
for `get_filtered_sessions` while the new classifier returns
`CustomPost` for it — keeping the two in sync would have required
updating `is_read_operation` to take params too, but it's cleaner to
delete it and centralize on the OpKind.

**Tests.**

- `test_of016_classify_get_with_first_param_ast`: unit matrix covering
  14 (name, first-param-type, expect_CustomGet) rows -- id-like
  primitives, `Option<…>`, custom structs, qualified paths, generic
  containers. Includes the symmetry assertion that
  `is_read_op(&classify_op(f))` agrees with the classifier on every
  row, so the two helpers cannot drift.
- `test_of016_get_with_body_param_routes_as_post`: end-to-end test
  parses a synthetic `api/v1/export.rs` with five functions through
  `scan_api_dir` + `http::generate`, then asserts the rendered
  output. Positive cases: `get_session(&str)` stays GET with
  `Path(id): Path<String>`; `get_filtered_sessions(&ExportFilterRequest)`
  and `get_summary(&ExportRequest)` flip to POST with
  `Json(body): Json<{Module}{Fn}Body>`; `get_recent(Option<String>)`
  stays GET with `Query(q): Query<…>` extraction; zero-param
  `get_count` stays GET. Negative cases: no `Path(filter):
  Path<String>` extraction of the body struct, no `:filter` or
  `:request` URL segments, no `get(...)` route for the body-carrying
  handlers.
- `test_is_read_op` replaces the deleted `test_is_read_operation` and
  exercises the new helper directly across the full OpKind range.

**Documentation.**

- `site/src/content/docs/cookbook/custom-api-endpoints.mdx` --
  classification rules split into a CRUD/junction vocabulary table
  and a custom-functions table keyed by first-param shape. The
  id-like primitive allowlist is documented explicitly. A new
  callout explains the body-carrying `get_*` case so consumers can
  discover the routing rule from docs rather than from a build
  failure.
- `site/src/content/docs/guides/api-layer.mdx` -- softens the
  blanket "starting with `get_` becomes GET" claim and links into
  the cookbook table for the full ruleset.

**Source-side attribute deferred.** The original ticket also proposed
`#[ontogen::post]` / `#[ontogen::get]` (or `#[ontogen(method = "post")]`)
as an explicit escape hatch. With the AST heuristic in place, no real
user repro requires the attribute — the heuristic covers the common
cases. If a downstream surfaces a shape the heuristic gets wrong, file
a new ticket with the specific signature.

**Path-extractor hardcoded `String` -- still latent.** The HTTP
generator's `path_params` slot at `http.rs:603-625` still hardcodes
`String` as the path-extractor type for any non-`i{32,64}`/`u{32,64}`
shape. After OF-016, this is much less of a problem -- the only
remaining way a non-`String` ident reaches the path slot is via
`get_session(state, id: Uuid)` (or similar), which silently extracts
as `String` and then fails to convert when the user fn expects `Uuid`.
Worth a follow-up ticket if real consumers hit it; not in scope here.

---

*The remainder of this document is preserved as a record of the original analysis.*

## Problem

The classifier maps any function whose name starts with `get_` to `OpKind::CustomGet`, regardless of what its parameters look like. Downstream this drags two consequences along that are wrong for body-carrying read endpoints:

1. The HTTP generator routes the function as a `GET`.
2. The second param is destructured as `Path(name): Path<String>` — i.e., treated as a path-segment string id.

A function like `export::get_filtered_sessions(state: &Store, filter: &ExportFilterRequest) -> Result<Vec<Session>, AppError>` is a logically-read operation that needs a body. With the current classifier it becomes `GET /api/exports/filtered-sessions/:filter` with `Path(filter): Path<String>`, then the handler tries to pass `&String` where the service fn expects `&ExportFilterRequest` — the generated code fails to compile.

The only escape today is to rename the function so it no longer starts with `get_` (the consumer renamed `get_filtered_sessions` → `filtered_sessions` and `get_summary` → `summary` to fall through to `CustomPost`). That works but loses the GET hint entirely; there is no way to say "this is a read-only operation that accepts a body."

## Location

- `src/servers/classify.rs:46` — the prefix check:
  ```rust
  if name.starts_with("get_") || params.is_empty() { OpKind::CustomGet } else { OpKind::CustomPost }
  ```
- `src/servers/classify.rs:53` — `is_read_operation` does a second name-based check (`starts_with("get_")` / `starts_with("list_")` / `starts_with("detect_")`) that the HTTP generator branches on.
- `src/servers/generators/http.rs:541` (`generate_generic_http_handler`) and `:998` (`generate_generic_http_handler_scoped`) — both call `is_read_operation` to decide method, then partition params via `ty.contains("Input")` / `ty.starts_with("Option<")` to allocate them to path/query/body slots. When `is_get` is true, anything that isn't `Option<…>` and doesn't contain `Input` in its name lands in `path_params`.
- `src/servers/generators/http.rs:603-625` — emits `Path(name): Path<String>` (or a tuple thereof) for those path params, hardcoding `String` unless the rendered type is one of `i32`/`i64`/`u32`/`u64`.

The Pumice consumer hit this on `export::get_filtered_sessions` and `export::get_summary` and worked around it by renaming. See the consumer's feedback entry "OF-013" for the verbatim repro.

## Current behavior

Reproducer:

```rust
// api/v1/export.rs
pub fn get_filtered_sessions(
    state: &Store,
    filter: &ExportFilterRequest,
) -> Result<Vec<Session>, AppError> { ... }
```

Generated route + handler (paraphrased):

```rust
.route("/api/exports/filtered-sessions/:filter", get(exports_get_filtered_sessions))

async fn exports_get_filtered_sessions(
    State(state): State<Arc<Store>>,
    Path(filter): Path<String>,  // ← classifier said GET → Path; type forced to String
) -> Result<Json<Vec<Session>>, ApiError> {
    let result = export::get_filtered_sessions(&state, &filter)?;
    //                                                ^^^^^^^^ &String, not &ExportFilterRequest
    Ok(Json(result))
}
```

Compile failure: `expected &ExportFilterRequest, found &String`.

Same shape repros for any `get_*` fn whose second param is a custom struct rather than an id-like primitive.

## Proposed resolution

Two layered fixes, smallest first.

### 1. Classifier consults the second param's AST

In `classify_by_name_and_params`, when the name starts with `get_` and there's a second param whose AST shape is a custom struct (or `&CustomStruct`), emit `CustomPost` instead of `CustomGet`. Heuristic for "custom struct": `Type::Path` (or `Type::Reference` to a `Type::Path`) where the last-segment ident is not in a small allowlist of "id-like primitives" (`String`, `str`, `i32`, `i64`, `u32`, `u64`, `Uuid`, etc.). The exact allowlist can mirror the HTTP generator's path-type list at `http.rs:605-611`.

This keeps the convenience of `get_*` → GET for the common case (id-in-path read endpoints) but doesn't force-GET when the signature obviously wants a body.

### 2. Source-side attribute to override the method

Independent of the classifier heuristic, support an explicit `#[ontogen::post]` (or symmetrical `#[ontogen::get]`) attribute on a function to override classification regardless of the name prefix. Same proc-macro plumbing OF-007 (`#[ontogen::stateless]`) and OF-003 (`#[ontogen(rename = "...")]`) already use — a no-op attr that the parser inspects to set a field on `ApiFn`.

Attribute wins over heuristic; heuristic wins over name. This mirrors how OF-003's source-attribute-vs-config-map precedence works.

### 3. Independent: hardcoded `Path<String>` extraction

Even after the classifier fix, the HTTP generator's `path_params` slot hardcodes `String` as the path extractor type for any param whose rendered ident isn't `i{32,64}` / `u{32,64}`. That's separable from this ticket but worth noting: the same AST migration that fixes (1) above would naturally let the HTTP generator inspect the param's AST and pick the right path-extractor type (or refuse to put a non-stringlike type in a path slot). Track as a follow-up if (1) alone doesn't subsume it.

## Effort

Small-to-medium. The classifier change is a few lines + a unit test matrix (similar shape to OF-013's `test_param_to_owned_type_matrix`). The proc-macro attribute is the same template as OF-007 / OF-003 — small but touches the parser, IR, and at least the HTTP + IPC generators (the IPC side doesn't care about GET/POST but should still respect the override for consistency in any future routing).

## Tests

- Unit tests on `classify_by_name_and_params` covering:
  - `get_summary(state, request: &ExportRequest)` → `CustomPost` (custom struct).
  - `get_session(state, id: &str)` → `CustomGet` (id-like primitive, current behavior preserved).
  - `get_session_by_id(state, id: String)` → `CustomGet`.
  - `get_all(state)` → `CustomGet` (no second param falls through to current rule).
  - `#[ontogen::post] fn get_x(state, id: &str)` → `CustomPost` (attribute overrides).
- End-to-end through `parse::scan_api_dir` → `generators::http::generate` asserting the route emits `post(...)` and the handler destructures `Json(...)` instead of `Path(...)` for the body-carrying case (mirrors `test_of013_unsized_dst_owned_form_in_ipc`'s end-to-end shape).

## Open questions

- **Should the attribute live on `ontogen-macros` as `#[ontogen::post]` / `#[ontogen::get]`, or fold into a single `#[ontogen(method = "post")]`?** The former matches OF-007's `#[ontogen::stateless]` shape; the latter scales better if more HTTP knobs land later (e.g., `path = "/custom-route"`). Lean toward the single-arg form for forward-compat.
- **Is the path-extractor hardcoded `String` fix in scope here or a follow-up?** Currently scoped as a note in §3 above; can split if the classifier fix alone closes the consumer's repro.
- **`Result::is_empty` second branch — `params.is_empty() → CustomGet`** — keep this rule? Probably yes: a zero-param function obviously has no body, GET is correct. Confirm there's no consumer use case where a no-param fn needs to be a POST.
