---
type: epic
schema_version: "1"
id: E0003
status: proposed
title: Consumer-controlled HTTP error responses
created: 2026-08-16
last_reviewed: 2026-08-16
tags: [servers, http, errors, dx]
---
# Epic — Consumer-controlled HTTP error responses

**Milestone:** M3 — Observability & extensibility ("First-class error-type
specification … the wire error shape is consumer-controlled rather than
ontogen-imposed", [roadmap](../../roadmap.md))
**Status:** proposed — design pass complete, no implementation started
**Design source:** [design document](../../http-error-mapping-design.md) —
exact surfaces, generated-output spec, scanning semantics, call-site
routing, test plan

## Problem

Every error a generated HTTP server produces is `500 Internal Server Error`.

The Axum emitter writes exactly one error pathway
(`src/servers/generators/http.rs:122-139`):

```rust
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

type ApiError = (StatusCode, Json<ErrorResponse>);

fn err(msg: String) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: msg }),
    )
}
```

and ~20 call sites that all read `.map_err(|e| err(e.to_string()))` — the
CRUD arms (`http.rs:222-457`), the generic custom-op handlers
(`http.rs:550-725`), and their route-prefix-scoped near-duplicates
(`http.rs:733-1178`). The only status codes a generated server can emit are
`500`, `201` (create), and `204` (delete / unit return). There is no 400,
404, 409, or 422 path anywhere.

What makes this worse than a missing feature: **ontogen itself produces the
semantic information and then destroys it.** The store generator emits
not-found detection for every entity —
`src/store/backends/seaorm/gen_crud.rs:387-389` and
`src/store/backends/markdown/gen_crud.rs:114` both generate

```rust
.ok_or_else(|| AppError::NoteNotFound(id.to_string()))?;
```

— so the consumer's `AppError` reliably distinguishes "row absent" from "db
exploded". That distinction survives the store layer and the API layer
(`src/api/gen_crud.rs` forwards `Result<T, AppError>` verbatim) and dies at
exactly one place: `err(e.to_string())`. The end-to-end path today:

```
sea_orm::DbErr / markdown_store::Error
  → AppError::DbError / AppError::Md / AppError::{Entity}NotFound   (store, generated)
  → passed through verbatim                                         (api, generated)
  → e.to_string() → err() → (500, {"error": msg})                   (server, generated)
  → throw new Error(body.error)                                     (TS client, generated)
```

Concretely, in the CI-covered pilot
(`crates/markdown-pilot/src/api/transport/http/generated.rs`), `GET
/api/notes/nonexistent` returns `500 {"error":"Note not found:
nonexistent"}`. A REST client, an HTTP cache, a load balancer health check,
and the generated TS client all see an internal server error for a routine
lookup miss.

There is also no escape hatch. A consumer who needs a 409 for a slug
collision has two options today: stop using `gen_servers` for that entity
and hand-write the router, or grep-and-patch the generated file after every
build. Both defeat the pipeline.

### Two related defects in the same pathway

- `JunctionAdd` parameter validation
  (`src/servers/generators/http.rs:417-420`) — a missing required body field
  returns 500. This is an ontogen-authored request check and should be
  400/422 regardless of any consumer-facing error API.
- Scoped store accessor failures (`http.rs:1130-1133` and the scoped CRUD
  arms) — with `route_prefix` set, `state.{accessor}(&scope_id)` failing
  (typically: unknown scope id, e.g. `/projects/{project_id}/…` with a bad
  id) returns 500. The natural status is 404, but the accessor's error type
  is consumer-owned, so this rides on the same mapping machinery as
  handler errors rather than a hard-coded fix.

## Constraints the current architecture imposes

Facts that shape the design (verified against the code, 2026-08-16):

1. **`AppError` is consumer-authored, ontogen-consumed.** The store
   generator does `use {schema_module_path}::AppError`
   (`src/store/mod.rs:128-129`) and constructs variants by convention
   (`DbError`, `Md` via `From`, `{Entity}NotFound`). Ontogen never sees the
   enum's definition today — it just emits references and lets rustc check
   them. So any status mapping keyed on variants must either scan the
   consumer's enum or reference only variants ontogen provably generates
   uses of.

2. **The handler parser throws the error type away.**
   `extract_result_ok_type` (`src/servers/parse.rs:664-677`) keeps only the
   `Ok` type of `Result<T, E>`; `ApiFn` has no `error_type` field. The only
   constraint on a hand-written handler's `E` today is `Display`, because
   every generated call site does `e.to_string()`. A design that changes
   what generated code calls on `E` must know when `E` is `AppError` and
   when it is some ad-hoc type, or it breaks existing handlers.

3. **The mapping point is duplicated ~20×** across unscoped CRUD arms,
   scoped CRUD arms, and two near-identical generic-handler emitters. Any
   error-strategy change must first consolidate these or it lands in twenty
   string literals.

4. **The wire shape is a cross-crate contract.** The emitted TS transport
   (`src/clients/generators/transport.rs:332-375`) reads `body.error ??
   res.statusText`. IPC and MCP transports flatten to `Result<T, String>`.
   `ErrorResponse { error: String }` can gain fields but must not lose
   `error` without a lockstep client change.

5. **Config fields cost three edits each** — `ServersConfig`
   (`src/lib.rs:553-584`), the internal `Config`
   (`src/servers/config.rs:13-70`), and the hand-written conversion
   (`src/servers/mod.rs:44-58`). Per-generator knobs are cheaper:
   `ServerGenerator::HttpAxum` is matched in exactly one place
   (`src/servers/mod.rs:219-231`).

6. **Pipeline knows where `AppError` lives; standalone callers don't.**
   `Pipeline::new(schema_dir)` (`src/pipeline.rs:150-158`) can hand the
   servers stage the schema directory for free. Direct `gen_servers`
   callers would need to opt in via config.

## Design goals

Ranked; when they conflict, the earlier one wins.

1. **The zero-config default is correct.** Not-found is 404 out of the box.
   A consumer who writes no error code at all gets a REST API that behaves
   like one.
2. **Customization is declared where the error lives.** Adding a status for
   a variant should be an annotation on that variant — not a config map in
   `build.rs` keyed by strings, not a parallel file that drifts.
3. **Plain Rust escape hatch for full control.** When annotations aren't
   enough (custom body shape, problem+json, i18n), the consumer supplies an
   ordinary function and ontogen calls it.
4. **Existing consumers rebuild without edits.** Hand-written handlers with
   ad-hoc `Display` error types keep compiling. The wire body shape
   (`{"error": string}`) is unchanged by default.
5. **One mental model with the rest of the pipeline.** Build-time `syn`
   scanning of consumer source + inert marker attributes is how ontogen
   already does extension (`#[ontogen::stateless]`, `#[ontogen::http::get]`,
   `// ontogen:singleton`); the store's scaffold-once hooks
   (`src/store/gen_hooks.rs`) are the pattern for consumer-owned files.
   Reuse those, don't invent a third mechanism.

## Options considered

### A. Require `AppError: axum::response::IntoResponse`

Generated handlers return `Result<Json<T>, AppError>` and axum does the
rest. Idiomatic axum, maximal power — and wrong for this pipeline:

- Breaks every existing consumer at once (none implement it).
- Forces an `axum` dependency into the consumer's schema module, which the
  Tauri IPC and MCP transports also consume. The schema module is the one
  place that must stay transport-neutral.
- No default: every consumer must write the impl before anything works.

Rejected as the primary mechanism; the full-override hook (phase 3) gives
the same power without contaminating the schema module.

### B. Status map in `ServersConfig`

`error_statuses: HashMap<String, u16>` keyed by variant name, following the
`command_overrides` overlay precedent (`src/servers/types.rs:660`).
Cheap to build, but it's configuration-at-a-distance: the mapping lives in
`build.rs`, drifts from the enum silently (a typo'd variant name is ignored
rather than compile-erroring), and stringly-typed keys scale badly. The
overlay precedent exists for *naming*, where the data genuinely is config;
error semantics belong with the error type.

Rejected as the consumer-facing API. (An overlay may still fall out of
phase 2 internals for free, but it is not the documented surface.)

### C. Scaffold-once mapper file (store-hooks pattern)

`gen_servers` scaffolds an `error.rs` next to the generated transport
containing `pub fn error_response(e: AppError) -> ApiError`, seeded with the
404 mapping, never overwritten — exactly like `src/store/gen_hooks.rs`.

Attractive, and it's the right *shape* for the full-override hatch. But as
the default mechanism it has two problems: the scaffold goes stale as
entities/variants are added (the seeded match doesn't grow), and it makes
the *common* case (one status for one variant) cost a whole consumer-owned
file plus module wiring. Store hooks scaffold no-ops that are correct
forever; an error mapper scaffold freezes a snapshot of the enum.

Kept as the phase-3 escape hatch, rejected as the default.

### D. Variant annotations scanned at build time  ← chosen

The consumer annotates their own enum; ontogen's existing `syn` scanning
reads the annotations during `gen_servers` and emits the status match into
the generated file. No new file, no config, nothing to drift — the
declaration lives on the variant it describes, and the generated match
references real variants, so rustc still checks everything.

Requires one new no-op macro (a derive, so the helper attribute is legal on
variants) and a schema-source scan. Both have direct precedent:
`#[proc_macro_derive(OntologyEntity, attributes(ontology))]` is already a
no-op derive with a helper attribute (`crates/ontogen-macros/src/lib.rs:29`),
and `parse_force_method` (`src/servers/parse.rs:391-407`) is already an
inert-attribute scan.

## Proposed design

### What the consumer sees

Zero config — rebuild and not-found becomes 404:

```rust
// schema/mod.rs — unchanged from today
pub enum AppError {
    NoteNotFound(String),   // → 404 by naming convention
    Md(String),             // → 500
}
```

Custom statuses — annotate the enum you already own:

```rust
#[derive(Debug, ontogen::HttpError)]
pub enum AppError {
    NoteNotFound(String),        // → 404 (convention; no annotation needed)
    #[http(status = 409)]
    DuplicateSlug(String),       // → 409
    #[http(status = 422)]
    InvalidBody(String),         // → 422
    Md(String),                  // → 500 (default)
}
```

`ontogen::HttpError` is a no-op derive (nothing generated at macro time);
it exists so `#[http(...)]` is a legal inert helper attribute. The build
script's `syn` scan — not the macro — reads the annotations, mirroring how
`#[ontogen::http::get]` works today.

Full control — supply a function, get the whole response:

```rust
// build.rs
ServerGeneratorConfig::HttpAxum {
    output: "src/api/transport/http/generated.rs".into(),
    error_handler: Some("crate::api::http_error::to_response".into()),
}

// src/api/http_error.rs — consumer-owned, ordinary Rust
pub fn to_response(e: AppError) -> axum::response::Response {
    // problem+json, custom bodies, logging, whatever
}
```

### Generated output (phases 1–2)

The single `err()` chokepoint becomes three small pieces in the generated
file:

```rust
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

type ApiError = (StatusCode, Json<ErrorResponse>);

/// Fallback for handler error types ontogen doesn't recognize. 500, as today.
fn err(msg: String) -> ApiError { (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: msg })) }

/// Ontogen-authored request validation failures (missing junction param, bad scope).
fn bad_request(msg: String) -> ApiError { (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })) }

/// The consumer's domain errors, mapped per scanned annotations + conventions.
fn app_error(e: AppError) -> ApiError {
    let status = match &e {
        AppError::NoteNotFound(..) => StatusCode::NOT_FOUND,
        AppError::DuplicateSlug(..) => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(ErrorResponse { error: e.to_string() }))
}
```

Call-site routing:

- Generated CRUD handlers (`E` is `AppError` by construction —
  `src/api/gen_crud.rs` hard-codes it): `.map_err(app_error)`.
- Hand-written handlers whose parsed `E`'s last path segment is `AppError`:
  `.map_err(app_error)`.
- Hand-written handlers with any other / unparseable `E`: today's
  `.map_err(|e| err(e.to_string()))` — behavior identical, nothing breaks.
- The `JunctionAdd` missing-param check: `bad_request(...)`.
- `state.store().await` construction failures and the scoped
  `state.{accessor}(&scope_id)` calls: keep the `err()` fallback in phase 1
  (neither contract formally pins the error type; see open questions).

### How the mapping is derived

Precedence, highest first:

1. `#[http(status = N)]` on the variant (scanned).
2. Naming convention: variant name ends in `NotFound` → 404. This is
   ontogen's own convention — the store generator manufactures these
   variants (`not_found_variant()`, seaorm `gen_crud.rs:387`) — so the
   server generator honoring it is symmetry, not magic.
3. Default: 500.

The scan parses `.rs` files in the configured schema source for
`enum AppError` (name fixed for now; the enum ontogen's own generated code
already imports). `Pipeline::build` passes its `schema_dir` to the servers
stage automatically; standalone `gen_servers` callers opt in via a new
optional `ServersConfig.error_source_dir`. **When no enum is found, emit
exactly today's output** (single `err()`, all-500) — never guess at
variants the compiler might reject, and never break the
scan-dirs-only use case.

### Wire compatibility

- Body shape `{"error": string}` is byte-identical; the TS transport
  (`src/clients/generators/transport.rs:332-375`), IPC, and MCP need no
  changes. `throw new Error(body.error ?? res.statusText)` already ignores
  status specifics.
- Status change (500 → 404/400 on the affected paths) is a behavior fix,
  released as `feat(servers)` with a changelog note. Anything matching on
  500-for-not-found was matching on a bug.
- `ErrorResponse`/`err()` naming inside the generated file is private
  (`type ApiError` and both fns are file-local); renames there are not a
  public API change. `ErrorResponse` becomes `pub` only so a future phase-3
  handler can reuse it if it wants.

## Phases

Each phase is independently shippable and leaves the tree green.

### Phase 0 — consolidate the chokepoint (pure refactor)

Collapse the ~20 `.map_err(|e| err(e.to_string()))` string literals and the
per-arm `let err_map = ...` rebindings (`http.rs:225, 285, 304, 323, 342,
360, 785, 842, …`) into one shared emission helper on the generator side.
Snapshot tests must show **zero** output diff. This is the enabling move —
after it, every later phase edits one function.

*Touches:* `src/servers/generators/http.rs` only.

### Phase 1 — correct defaults, zero config

- New `ApiFn.error_type` (+ AST) captured in `extract_result_ok_type`'s
  replacement; `servers/parse.rs` unit tests.
- Schema-source error-enum scan (`enum AppError`, variant names only in this
  phase); `Pipeline` threads `schema_dir`; `ServersConfig.error_source_dir`
  for standalone callers (3-place config plumbing).
- Emit `app_error()` with the `*NotFound → 404` convention; route call
  sites per the table above; `bad_request()` for the junction param check.
- Tests: insta snapshots (`src/servers/tests.rs`); a live 404 assertion in
  `crates/markdown-pilot/tests/http_router.rs` (`GET /api/notes/absent` →
  404, body `{"error": ...}`) — markdown-pilot is the only place CI
  actually executes generated handlers, so the wire behavior lands there;
  `tests/backend_parity.rs` stays green.
- Regenerate `examples/` (iron-log, iron-log-md, notes-kb, tasks-tracker).

### Phase 2 — `#[derive(ontogen::HttpError)]` + `#[http(status = N)]`

- `ontogen-macros`: no-op `HttpError` derive with `attributes(http)`
  (precedent: `OntologyEntity`/`ontology` at
  `crates/ontogen-macros/src/lib.rs:29`); re-export from `ontogen` root.
  Version note: `ontogen-macros` is pinned `=0.1.1` from the root crate —
  additive bump + pin update via release-plz.
- Extend the enum scan to read `#[http(status = N)]` on variants; attribute
  beats convention beats default. Malformed attributes (`status = "x"`,
  out-of-range) are build errors via `CodegenError`, not warnings —
  config-at-a-distance silently drifting is exactly what this design
  rejects.
- Docs: site guide section + walkthrough update; fix the stale
  `src/lib.rs:55-61` doc comment ("Today this is just
  `#[ontogen::http::post]`" — `get` already shipped) while in that file.
- One example adopts an annotated variant (notes-kb `DuplicateSlug`-style)
  so the feature is exercised end-to-end in a real consumer.

### Phase 3 — full-override hook

- `ServerGenerator::HttpAxum` gains `error_handler: Option<String>` (module
  path to `fn(AppError) -> axum::response::Response`). Per-generator, so
  the config cost is the single match site (`src/servers/mod.rs:219-231`)
  — though note the enum is consumer-constructed, so adding a field to the
  variant **is** a breaking config change; ship it in a minor release with
  a `..Default`-friendly shape or a builder, decided at implementation
  time.
- When set: generated handlers whose `E` is `AppError` return
  `axum::response::Response` from the consumer fn; annotated/convention
  mapping is bypassed entirely (one owner of the wire shape at a time).
  Unknown-`E` handlers keep the fallback.
- Optional scaffold-once starter file (store-hooks pattern,
  `src/store/gen_hooks.rs` precedent) so `error_handler` adopters start
  from the phase-1 mapping instead of a blank page.

### Phase 4 — follow-ups (filed, not scheduled)

- Typed error codes on the wire (`{"error", "code"}`) + generated TS
  `ApiError` class; needs the lockstep client change and is where
  IPC/MCP error fidelity (currently `Result<T, String>`) joins.
- Per-handler status overrides for *success* codes and non-`AppError`
  custom error types routed through consumer-declared conversions.
- `AppState::store()` error contract formalization (open question below).

## Open questions

1. **Should `state.store().await` failures route through `app_error`?**
   The reference impl returns `AppError` already
   (`crates/markdown-pilot/src/lib.rs:23-28`), but the contract is
   informal. Phase 1 keeps the `Display` fallback (500 — construction
   failure genuinely is a 500 in almost every case); formalizing the
   contract is phase 4.
2. **Helper attribute name:** `#[http(...)]` (proposed — groups future keys
   like `code = "conflict"`) vs `#[ontogen(...)]` (avoids any conceivable
   helper-name collision with other derives). Decide in phase-2 review;
   pure surface choice, zero structural impact.
3. **Is `enum AppError` name-configurable?** Not in this epic — the store
   generator hard-codes the name too. If a consumer ever needs to rename
   it, that's a pipeline-wide `error_type_name` config, out of scope here.

## Incidental findings (fix opportunistically, not gating)

Surfaced during the analysis; none block this epic:

- `src/lib.rs:55-61` doc comment is stale (claims only `http::post`
  exists; `http::get` shipped in 4a2704c). Scheduled into phase 2.
- `crates/ontogen-macros/src/lib.rs:106-108` says zero-user-param fns
  route as `CustomGet`; since the classifier reversal
  (`src/servers/classify.rs:147-148`) they default to `CustomPost` unless
  the name implies a read. The macro doc predates the reversal.
- `README.md:113-116` "Known Issues" claims `gen_clients` is a no-op;
  `src/clients/` contradicts.

## Tasks

To be split into `tasks/` files when the epic is accepted; the intended
PR-sized cuts are the phases above:

- [ ] Phase 0 — consolidate error-map emission (refactor, zero snapshot drift)
- [ ] Phase 1 — error-enum scan + 404/400 defaults + pilot wire tests + example regen
- [ ] Phase 2 — `HttpError` derive + `#[http(status)]` + docs pass
- [ ] Phase 3 — `error_handler` full-override hook (+ optional scaffold)
- [ ] Phase 4 — follow-up tickets filed (typed codes / TS client, IPC & MCP parity, store-contract formalization)
