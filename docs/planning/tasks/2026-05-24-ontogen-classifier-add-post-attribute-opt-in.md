---
type: task
schema_version: '3'
status: in-progress
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
readiness_verified_at: '2026-05-24T15:36:32Z'
last_reviewed: '2026-05-24'
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

Add a no-op `post` proc-macro attribute to `ontogen-macros`, re-exported through the consumer-side `ontogen` crate as `pub mod http { pub use ontogen_macros::post; }` so the canonical user-facing form is `#[ontogen::http::post]`. The parser recognizes the attribute on a `pub async fn` by final-path-segment match and stamps `force_method: Option<ForcedMethod>` on the resulting `ApiFn`. `classify_op` consults that field BEFORE running the existing heuristic — if `Some(ForcedMethod::Post)`, the classifier returns `OpKind::CustomPost` unconditionally.

Two architectural choices folded into this rev (over the initial PR #77 implementation):

1. **Enum, not bool.** `force_method: Option<ForcedMethod>` instead of `force_post: bool`. The enum starts with one variant (`Post`) but accommodates the anticipated `#[ontogen::http::get]` follow-up (see the companion `[[2026-05-24-ontogen-classifier-reverse-zero-param-default-to-post]]` task — when the zero-param default flips to POST, any false-positive reads will need an explicit GET opt-in) without changing `ApiFn`'s shape. The migration shape is also future-additive: adding `Get` / `Put` / `Delete` / `Patch` variants is purely additive.
2. **Namespaced under `http::*`.** The bare `#[ontogen::post]` collapses two unrelated taxonomies: routing-shape-agnostic markers (`stateless`, `rename`, `skip` — these apply regardless of output target) and HTTP-method-shape overrides (this one). Putting HTTP overrides under `http::*` makes the scope explicit and leaves room for non-HTTP output targets (hypothetical gRPC/CLI surfaces) without retro-namespacing the existing flat attributes.

End state: Pumice annotates `pause`, `resume`, `reset`, `cancel`, `end`, `skip_break`, `stop`, `backup`, `pick_restore`, `clear_sessions`, `reset_all` with `#[ontogen::http::post]` and the next codegen emits POST routes. No change for any current consumer that doesn't add the attribute.

## Approach

1. **Add the proc-macro definition** in `crates/ontogen-macros/src/lib.rs` alongside `#[ontogen::stateless]` and the other existing no-op markers. Same shape: an `#[proc_macro_attribute] pub fn post(...)` that returns its input unchanged (the macro is a parsing hint, not a real expansion). Note: proc-macros in Rust must be defined at the crate root, so the `http::*` namespacing lives in the consumer-side crate's re-export, not inside `ontogen-macros`.
2. **Re-export under `pub mod http`** in the main `ontogen` crate (`src/lib.rs`): replace `pub use ontogen_macros::post;` at the top level with `pub mod http { pub use ontogen_macros::post; }`. Routing-shape-agnostic attributes (`OntologyEntity`, `ontogen`, `stateless`) stay at the top level.
3. **Add the `ForcedMethod` enum** to `src/servers/parse.rs` (where `ApiFn` lives). Single variant today: `Post`.
4. **Extend the AST parser** in `src/servers/parse.rs` with `parse_force_method(func: &syn::ItemFn) -> Option<ForcedMethod>` — matches the final path segment of each attribute. Today returns `Some(ForcedMethod::Post)` when the segment is `post`. Future method overrides extend by adding a match arm per ident.
5. **Add the field** `force_method: Option<ForcedMethod>` to `ApiFn` and stamp it from the parser's construction site.
6. **Update `classify_op`** in `src/servers/classify.rs` to match on `func.force_method` — `Some(ForcedMethod::Post)` returns `OpKind::CustomPost` early; `None` falls through to the existing heuristic.
7. **Tests.** Three unit tests cover the new path: parser-stamps-the-field, end-to-end HTTP-route-emission, and bare `#[post]`-form-still-accepted. Plus the orthogonal `test_force_method_post_overrides_classifier` covering the classifier short-circuit logic directly.
8. **Document** the attribute under `site/src/content/docs/cookbook/custom-api-endpoints.mdx`. Cite the canonical `#[ontogen::http::post]` form, the bare-import alternative, and explain the namespace separation rationale.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `crates/ontogen-macros/src/lib.rs` | modify | add the `#[proc_macro_attribute] pub fn post(...)` no-op macro. |
| `src/lib.rs` | modify | wrap the re-exported `post` in `pub mod http { pub use ontogen_macros::post; }`; leave routing-shape-agnostic attrs (`OntologyEntity`, `ontogen`, `stateless`) at the top level. |
| `src/servers/parse.rs` | modify | add `ForcedMethod` enum; add `force_method: Option<ForcedMethod>` field on `ApiFn`; add `parse_force_method()` that matches on final path segment and returns `Some(ForcedMethod::Post)` for `post`. |
| `src/servers/classify.rs` | modify | match on `func.force_method` at the entry point of `classify_op`; return `OpKind::CustomPost` early for `Some(ForcedMethod::Post)`. |
| `src/servers/tests.rs` | modify | new tests cover parser-stamps-field, end-to-end HTTP emission, and bare `#[post]` form. Existing fixtures updated to use `force_method: None` (default). |
| `site/src/content/docs/cookbook/custom-api-endpoints.mdx` | modify | document `#[ontogen::http::post]` (canonical form) and the bare-import variant alongside the namespace-rationale prose. |

## Acceptance criteria

- [ ] AC-1: Unit/integration test: a function annotated `#[ontogen::http::post]` emits as `post(...)` in the generated `entity_routes()`, regardless of whether the name or params would otherwise route to GET.
- [ ] AC-2: `cargo build` in `examples/iron-log/src-tauri/` succeeds with byte-identical generated TS/Rust — no behavioral regression on consumers that don't use the attribute.
- [ ] AC-3: Pumice (sksizer/pumice#225 follow-up): can annotate at least one currently-routing-as-GET mutating handler (e.g. `engine::pause`) with `#[ontogen::http::post]` and regenerated TS/Rust show the route as POST.
- [ ] AC-4: `just full-check` passes on the rust-ontogen branch.
- [ ] AC-5: The bare `#[post]` form (after `use ontogen::http::post;`) is also accepted — parser matches on final path segment.

## Out of scope

- **Changing the default classifier behavior** — that lives in `[[2026-05-24-ontogen-classifier-reverse-zero-param-default-to-post]]`. This task is the conservative, additive, never-breaks-anyone fix; the default-reversal is the principled long-term fix and can land independently.
- **A symmetric `#[ontogen::http::get]` attribute** — anticipated by the reverse-default task as a likely need (force GET on a function whose name happens to look mutating but actually reads). Trivially added by extending the `ForcedMethod` enum + the `parse_force_method` match. Deferred until a real-world repro motivates the variant.
- **Method-specific attributes** for PUT, DELETE, PATCH — same shape (`#[ontogen::http::put]` etc.) is plausible. Today named-CRUD (`update`, `delete`) covers most cases; explicit `http::*` overrides for these methods are added when their consumers surface.

## Dependencies

- none. Pure additive feature.

## Discovery context

- Surfaced by sksizer/pumice#225's inline review comment from sksizer at `src-tauri/src/api/transport/http/generated.rs:590`: "a lot of these should probably be posts since they mutate data not gets. what are the ontogen generation rules in that regard?"
- The OF-016 task spec (2026-05-14 work, merged via rust-ontogen #b2f882c) explicitly deferred this attribute: "Source-attribute escape hatch (`#[ontogen::post]`) deferred until a real-world repro motivates it." Pumice #225 is the motivating repro.
- Companion to `[[2026-05-24-ontogen-classifier-reverse-zero-param-default-to-post]]`: this task is the user-driven opt-in; that task changes the default. Either ships alone or both ship together.

## Post-mortem

_Captured by /sdlc:task-work on 2026-05-24. PR: pending._

### Acceptance criteria coverage

- AC-1: auto — new tests `test_force_method_post_overrides_classifier`, `test_post_attr_zero_param_classifies_as_custom_post`, `test_post_attr_bare_path_form_accepted`, `test_post_attr_emits_post_http_route` in `src/servers/tests.rs` all green under `cargo test`.
- AC-2: agent-manual — `cargo build` in `examples/iron-log/src-tauri/` succeeded. No consumer code uses the attribute so the generated transport is byte-identical for that example.
- AC-3: deferred-user — verification lives in the Pumice consumer repo (sksizer/pumice#225 follow-up). Once this PR lands and Pumice bumps the ontogen dependency, the user can annotate `engine::pause` (and the 10 sibling action verbs listed in the spec) with `#[ontogen::post]` and confirm the regenerated `transport/http/generated.rs` shows `post(engine_pause)` instead of `get(engine_pause)`.
- AC-4: auto — `just full-check` passes (fmt-check, clippy with `--deny warnings`, full test suite, 220 unit tests + 3 integration tests all green). Baseline-gated quality-check runner reports `OK 1/1`.

### What worked

- The existing `has_stateless_attr` helper and `is_stateless` field on `ApiFn` gave a direct template for the new `has_post_attr` / `force_post` path. Symmetric naming and structure kept the diff small.
- The `write_synthetic_api` test scaffold made end-to-end HTTP route assertion straightforward — the new test scans a synthetic module and inspects emitted route strings directly.
- Quality-check baseline subtraction worked correctly: zero pre-existing findings on the captured SHA, zero new drift on HEAD.

### Friction and automation gaps

- start_task.py rebase conflicted because the verify-stamp commit and the start-commit both modified the task frontmatter. The conflict was mechanical (just a YAML key union: keep `readiness_verified_at:` from one side plus `last_reviewed:` from the other), but start_task.py exited with code 3 and the operator had to resolve manually. A targeted resolution heuristic in start_task.py — "if the only conflict is in the task file's frontmatter, union the two sides' frontmatter keys and continue" — would close this without operator intervention. The pattern is reproducible: any task that runs ensure-ready before start-commit will hit it. (Upstream-plugin: sdlc; cross-repo dispatch unavailable in this orchestrator context — see Spawned follow-up tasks.)
- The task spec listed `src/servers/types.rs` as the home for the new `force_post` field on `ApiFn`, but `ApiFn` actually lives in `src/servers/parse.rs` (types.rs holds normalization helpers, not the IR struct). The implementer had to grep to discover this. A pre-implementation step — "for every `Files to touch` row, confirm the cited symbol actually lives in the cited file before committing to the path" — would surface this kind of spec drift during the readiness gate, not during implementation. (Upstream-plugin: sdlc; cross-repo dispatch unavailable in this orchestrator context — see Spawned follow-up tasks.)
- Adding a new field to `ApiFn` required updating 18 struct-literal call sites across `tests.rs` and `parse.rs`. The Edit tool's `replace_all` worked at the same-indent-level granularity, but three different indent levels (4/8/12-space leads) required three separate Edit calls. A `#[derive(Default)]` on `ApiFn` combined with `..Default::default()` in test literals would make field additions backward-compatible without touching every fixture. This is a Rust idiom recommendation, not a skill change. → [[2026-05-24-apifn-derive-default-for-test-ergonomics]]
- The 5a→5b conflict-on-rebase failure mode is reproducible enough that the test runner should encode it as a regression: a synthetic task whose verify-stamp lands on the branch, then a start-commit lands on main, then rebase. The skill's resume-detection branch handles the post-failure recovery path; the targeted-resolution patch above would prevent the failure in the first place. (Sub-goal of bullet 1; covered there.)

### Spawned follow-up tasks

- [[2026-05-24-apifn-derive-default-for-test-ergonomics]] — Local: derive Default on `ApiFn` for test-fixture backward compatibility (created).
- Upstream-plugin (sdlc) — `sdlc:task-work` `start_task.py` should auto-resolve task-file frontmatter conflicts on rebase — **classification-failed-dispatch**: `/sdlc:cross-repo-task-pr` skill invocation denied by the harness running this orchestrator-dispatched sub-agent. The user should open this manually against the sdlc plugin repo at `/Users/sksizer2/Developer/dev` (origin `git@github.com:sksizer/dev.git`).
- Upstream-plugin (sdlc) — `sdlc:task-ensure-ready` should verify cited symbols exist in cited files during the `Files to touch` resolution — **classification-failed-dispatch**: same reason as above. Both upstream bullets should be promoted to tasks against the sdlc plugin repo manually.

