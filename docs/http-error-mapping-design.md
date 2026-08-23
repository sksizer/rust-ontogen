# Design — consumer-controlled HTTP error responses

Status: design source for epic
[E0003](planning/epics/http-error-mapping.md), which carries the problem
statement, the options considered, and the phase/PR breakdown. This
document is the engineering treatment: exact surfaces, generated-output
specification, scanning semantics, compatibility analysis, and test plan.
No implementation has started; line references are against `main` at the
time of writing (2026-08-16).

## Scope

Designs the error pathway of the generated **Axum HTTP transport** only.
Tauri IPC and MCP transports (which flatten errors to `Result<T, String>`)
and the generated TS client's error body are explicitly out of scope except
where noted in [Wire contract](#wire-contract); they are phase-4 follow-ups
in the epic.

## Current state (the "before")

The emitter writes one error pathway into every generated HTTP file
(`src/servers/generators/http.rs:122-139`), and every fallible call site
uses it identically. Real output from the CI-covered pilot
(`crates/markdown-pilot/src/api/transport/http/generated.rs`):

```rust
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

type ApiError = (StatusCode, Json<ErrorResponse>);

fn err(msg: String) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: msg }))
}

async fn note_get_by_id(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Result<Json<Note>, ApiError> {
    let store = state.store().await.map_err(|e| err(e.to_string()))?;
    note::get_by_id(&store, &id).await.map(Json).map_err(|e| err(e.to_string()))
}
```

Everything is 500. The full call-site inventory is in
[Appendix A](#appendix-a--call-site-inventory).

Two structural facts drive the design:

- **The handler parser discards the error type.** `extract_result_ok_type`
  (`src/servers/parse.rs:664-677`) keeps only `T` from `Result<T, E>`;
  `ApiFn` (`parse.rs:59-116`) has no error field. The only demand on a
  hand-written handler's `E` today is `Display`.
- **`AppError` is consumer-authored.** Generated store/API code imports it
  from `{schema_module_path}` and constructs variants by convention
  (`{Entity}NotFound`, `DbError`, `From`-converted backend errors). Ontogen
  never parses its definition today.

## Consumer-facing surface

Three layers; each is opt-in on top of the previous.

### Layer 0 — nothing (correct defaults)

```rust
// schema/mod.rs — unchanged from today
pub enum AppError {
    NoteNotFound(String),   // → 404 (naming convention)
    Md(String),             // → 500
}
```

Rebuild; `GET /api/notes/absent` is a 404. No consumer edit of any kind.

### Layer 1 — per-variant annotation

```rust
use ontogen::HttpError;

#[derive(Debug, HttpError)]
pub enum AppError {
    NoteNotFound(String),        // → 404, convention, no annotation needed
    #[http(status = 409)]
    DuplicateSlug(String),       // → 409
    #[http(status = 422)]
    InvalidBody(String),         // → 422
    Md(String),                  // → 500 default
}
```

`HttpError` is a **no-op derive**; it generates no code and exists solely to
register `http` as a legal inert helper attribute on the variants. The
build-time scan — not the macro — reads the annotations.

### Layer 2 — full override

```rust
// build.rs
ServerGeneratorConfig::HttpAxum {
    output: "src/api/transport/http/generated.rs".into(),
    error_handler: Some("crate::api::http_error::to_response".into()),
}

// src/api/http_error.rs — consumer-owned, ordinary Rust, never generated over
pub fn to_response(e: crate::schema::AppError) -> axum::response::Response {
    // problem+json, custom bodies, logging — the consumer owns the wire shape
}
```

When `error_handler` is set, layers 0/1 mapping is bypassed for
`AppError`-typed sites: one owner of the wire shape at a time.

## Detailed design

### 1. The error-enum scan

**New module** `src/servers/error_map.rs`, invoked from
`servers::generate()` (`src/servers/mod.rs:38`) before
`generate_transport`, producing:

```rust
pub(crate) struct ErrorMap {
    /// (variant_name, status) for variants whose status != 500,
    /// in source declaration order.
    pub arms: Vec<(String, u16)>,
}
```

threaded to the generators via a new `error_map: Option<ErrorMap>` field on
the internal `Config` (`src/servers/config.rs:13`).

**Algorithm.**

1. Input: `error_source_dir: Option<PathBuf>` (see [Config plumbing](#4-config-plumbing)).
   `None` → return `None`; generated output is byte-identical to today.
2. Walk `*.rs` files in the directory, **sorted by path** — same
   determinism rule the schema scan adopted in cbb89bd; the generated file
   is written via `write_if_changed`, and nondeterministic output causes
   watcher rebuild loops (documented at
   `src/servers/generators/http.rs:163-173`).
3. `syn::parse_file` each; collect top-level `ItemEnum`s named `AppError`.
   Enums nested in inline `mod` blocks are not searched (matching the
   schema scanner's top-level convention).
4. Zero found → `Ok(None)` (silent; the scan-dirs-only and
   servers-without-store use cases must keep working unchanged).
   More than one found → `CodegenError::Server` listing both paths —
   never guess.
5. Per variant, resolve a status with this precedence:
   1. `#[http(status = N)]` attribute (grammar below);
   2. variant name ends with `NotFound` → `404` — the symmetric read of
      ontogen's own store-side convention (`not_found_variant()`,
      `src/store/backends/seaorm/gen_crud.rs:386-389` and
      `src/store/backends/markdown/gen_crud.rs`);
   3. default `500`.
6. Keep only non-500 entries, in declaration order. If that set is empty,
   return `Ok(None)` — an all-500 map is indistinguishable from today, so
   emit today's exact shape rather than a degenerate match.

**Attribute grammar.** `#[http(status = <int-literal>)]` on a variant.
Validation is loud, not lenient — silent drift is the failure mode this
design exists to remove:

- `status` outside `100..=599` → `CodegenError::Server` naming the variant.
- Non-integer value, unknown key inside `#[http(...)]`, or an empty list →
  `CodegenError::Server`. (Future keys — e.g. `code = "conflict"` for the
  phase-4 typed wire codes — are added to the grammar, never skipped over.)
- The scan reads attributes syntactically, so it works whether or not the
  derive is present — but without `#[derive(HttpError)]` **rustc** rejects
  the inert attribute, so the consumer cannot end up with annotations that
  compile yet aren't honored.
- Coexists with `thiserror`/`serde` derives; helper-attribute namespaces
  (`error`, `serde`, `http`) don't collide.

### 2. The `HttpError` derive

`crates/ontogen-macros/src/lib.rs`, precedent directly above it at line 29
(`OntologyEntity` with `attributes(ontology)`):

```rust
#[proc_macro_derive(HttpError, attributes(http))]
pub fn derive_http_error(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
```

Re-exported from the facade root next to `OntologyEntity`
(`src/lib.rs:53`). Deliberately **not** under `ontogen::http::` — that
module namespaces *fn-classification* markers; this is a derive and follows
derive conventions (`use ontogen::HttpError`).

Release note: the root crate pins `ontogen-macros = "=0.1.1"`
(`Cargo.toml`). Adding a derive is additive → `0.2.0` (pre-1.0), pin bump
in the same PR, released via release-plz as usual.

### 3. Parser change — capture `E`

`extract_result_ok_type` (`src/servers/parse.rs:664-677`) is extended to
also return the second generic argument when present:

```rust
/// The inner `E` from `Result<T, E>`, normalized; `None` when the return
/// type isn't a two-argument `Result`.
pub error_type: Option<String>,
```

added to `ApiFn` (after `return_type_ast`, `parse.rs:75`). No AST is
carried — unlike `return_type_ast` (needed for import recursion), the error
type is only ever compared by last path segment.

**Routing predicate.** A call site maps errors with `app_error` iff:

- the internal config's `error_map` is `Some`, and
- the function's `error_type`'s **last path segment** equals `AppError`
  (`AppError`, `crate::schema::AppError`, `schema::AppError` all match —
  same last-segment convention as `parse_force_method`,
  `parse.rs:391-407`).

Everything else keeps `.map_err(|e| err(e.to_string()))` — behavior
identical to today, so hand-written handlers with ad-hoc `Display` error
types are untouched. Known accepted gap: a consumer alias
(`type Error = AppError;`) fails the segment match and falls back to 500 —
safe, documented, and fixable later without design change.

Generated CRUD forwarders need no special-casing: `gen_api` hard-codes
`Result<T, AppError>` (`src/api/gen_crud.rs:26-67`) and the servers stage
re-parses those files like any other module, so the predicate matches them
naturally.

### 4. Config plumbing

New public field, defaulting off:

```rust
pub struct ServersConfig {
    // ...
    /// Directory scanned for the consumer's `enum AppError` definition to
    /// derive HTTP status mappings. `None` disables the scan (all errors
    /// map to 500, as before). `Pipeline::build` fills this with its
    /// `schema_dir` automatically.
    pub error_source_dir: Option<PathBuf>,
}
```

Three-place plumbing (the known cost, epic constraint 5): `ServersConfig`
(`src/lib.rs:553-584`), internal `Config` (`src/servers/config.rs:13-70` +
`Default` at 81-99), the conversion literal (`src/servers/mod.rs:44-58`).

**Pipeline threading** — stage 5 (`src/pipeline.rs:499-502`) adopts the
same fill-if-unset pattern stage 6 already uses for
`ClientsConfig::schema_entities` (`pipeline.rs:509-512`):

```rust
if let Some(stage) = self.servers {
    let mut servers_config = stage.config;
    if servers_config.error_source_dir.is_none() {
        servers_config.error_source_dir = Some(self.schema_dir.clone());
    }
    gen_servers(api_out.as_ref(), &stage.scan_dirs, &servers_config)?;
}
```

Pipeline consumers therefore get layer 0 with zero edits. Direct
`gen_servers` callers opt in by setting the field. An explicit
`Some(dir)` from the consumer is never overwritten.

### 5. Generated-output specification

With an `ErrorMap` present, the shared-types block
(`http.rs:122-139`) becomes:

```rust
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

type ApiError = (StatusCode, Json<ErrorResponse>);

/// Fallback for error types ontogen doesn't recognize. 500, as before.
fn err(msg: String) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: msg }))
}

/// Ontogen-authored request validation failures.
fn bad_request(msg: String) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg }))
}

/// Domain errors, mapped per `#[http(status)]` annotations and the
/// `*NotFound → 404` convention.
fn app_error(e: AppError) -> ApiError {
    let status = match &e {
        AppError::NoteNotFound(..) => StatusCode::NOT_FOUND,
        AppError::DuplicateSlug(..) => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(ErrorResponse { error: e.to_string() }))
}
```

Emission rules:

| Item | Emitted when | Notes |
|---|---|---|
| `err()` | always | fallback + store-construction preamble |
| `bad_request()` | ≥1 `JunctionAdd` op present | avoids dead code churn in junction-free consumers |
| `app_error()` | `error_map` is `Some` | adds `AppError` to the `types_import_path` import group (it is absent today) |
| match arms | non-500 variants only, **source declaration order** | deterministic; `_ =>` closes the match |
| variant patterns | `Name(..)` for tuple/struct variants, bare `Name` for unit variants | read from the scanned `ItemEnum`, so rustc verifies every arm against the real enum |

Statuses are emitted as `StatusCode::NOT_FOUND`-style named constants where
axum defines one, `StatusCode::from_u16(NNN).unwrap()` otherwise (the
100..=599 validation makes the unwrap infallible; emit the named constant
for every status axum names, which covers all realistic annotations).

Handler bodies change only in which mapper they name:

```rust
async fn note_get_by_id(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Result<Json<Note>, ApiError> {
    let store = state.store().await.map_err(|e| err(e.to_string()))?;
    note::get_by_id(&store, &id).await.map(Json).map_err(app_error)
}
```

### 6. Call-site routing

Applying the predicate from §3 to the inventory in Appendix A:

| Site class | Today | After |
|---|---|---|
| CRUD arms, unscoped + scoped (`E` = `AppError` via gen_api) | `err(e.to_string())` | `app_error` |
| Generic custom handlers (`http.rs:720`, `:1168-1173`), `E` matches `AppError` | `err(e.to_string())` | `app_error` |
| Generic custom handlers, `E` ad-hoc / non-`Result` return | `err(e.to_string())` | unchanged |
| `JunctionAdd` missing-param check (`http.rs:417-420`) | `err(...)` → 500 | `bad_request(...)` → 400 |
| Store construction `state.store().await` (`http.rs:206`) | `err(e.to_string())` | unchanged (phase 1) — contract doesn't pin `E`; formalization is phase 4 |
| Scoped accessor `state.{accessor}(&scope_id)` (`http.rs:1130-1133`) | `err(e.to_string())` | unchanged (phase 1) — same reason; natural end state is 404 via `app_error` once the accessor contract pins `AppError` |

Phase 0 (pure refactor, zero output diff) collapses the ~20 duplicated
`.map_err(|e| err(e.to_string()))` string literals and per-arm
`let err_map = ...` rebindings into one generator-side helper, so every
row above is subsequently a one-line change. The scoped/unscoped emitter
duplication (`http.rs:222-457` vs `733-1003`; `550-725` vs `1006-1178`)
is not otherwise unified here — that's a larger refactor this epic
shouldn't absorb.

### 7. `error_handler` full override (phase 3)

```rust
pub enum ServerGenerator {
    HttpAxum {
        output: PathBuf,
        /// Module path to `fn(AppError) -> axum::response::Response`.
        /// When set, replaces the built-in mapping for `AppError`-typed
        /// sites; annotations and conventions are NOT consulted.
        error_handler: Option<String>,
    },
    // ...
}
```

Generated deltas when set:

- `type ApiError = axum::response::Response;`
- `err()` / `bad_request()` keep their bodies but end with
  `.into_response()` (they cover non-`AppError` sites and ontogen-authored
  validation, which the consumer fn — typed on `AppError` — cannot).
- `AppError`-typed sites emit `.map_err({path})` with the configured path
  verbatim; `app_error()` is not emitted at all.
- Handler signatures stay `Result<Json<T>, ApiError>`; `Response`
  implements `IntoResponse`, so axum is satisfied without further change.

Adding a field to an existing enum variant **is** a breaking change for
consumers constructing `HttpAxum { output }` literally. Accepted: the crate
is pre-1.0, the change rides a minor bump with a one-line migration
(`error_handler: None`), and the alternative — a parallel config channel —
is exactly the config-at-a-distance this design rejects. An optional
scaffold-once starter file (store-hooks pattern, `src/store/gen_hooks.rs`)
seeds adopters with the phase-1 mapping as plain Rust.

## Wire contract

- Body shape `{"error": string}` is unchanged in every layer of this
  design. The emitted TS transport's `body.error ?? res.statusText`
  (`src/clients/generators/transport.rs:332-375`), the IPC and MCP string
  flattening, and any consumer curl scripts keep working.
- `ErrorResponse` and `err` are file-local today (`struct`/`fn`, no `pub`);
  `ErrorResponse` becomes `pub` only so a phase-3 `error_handler` can reuse
  it. Neither is a semver surface — the generated file is consumer-side.
- Status changes (500 → 404/400/annotated) are **observable behavior
  changes**, shipped as `feat(servers)` with an explicit changelog entry.
  Anything depending on 500-for-not-found was depending on a bug; the TS
  client treats all non-2xx identically, so generated clients are
  indifferent.
- Layer-2 consumers own the body shape entirely; if they diverge from
  `{"error": string}` they own the client story too (the epic's phase 4
  covers typed codes as the supported path).

## Testing

**Phase 0** — existing insta snapshots (`src/servers/tests.rs`) must be
byte-identical. That *is* the test; no new coverage.

**Scan unit tests** (new, `src/servers/error_map.rs` `#[cfg(test)]`):
annotated/unannotated variants, precedence (annotation beats `NotFound`
convention), unit vs tuple variants, out-of-range and malformed attributes
→ error, duplicate enums across files → error, empty/no enum → `None`,
all-500 map → `None`, deterministic arm order.

**Emission snapshots**: new fixtures pairing an api dir with an error enum
source; snapshot the generated file with (a) no scan dir, (b) enum with
convention hits only, (c) enum with annotations, (d) junction ops present
(`bad_request` emission), (e) ad-hoc error-type handler (fallback
preserved). Case (a) must equal today's snapshot exactly.

**Live wire tests** — `crates/markdown-pilot/tests/http_router.rs` is the
only place CI executes generated handlers, so the observable behavior lands
there: `GET /api/notes/<absent>` → 404 with `{"error": ...}`; a 200 happy
path already exists. Phase 2 adds an annotated variant to the pilot's
`AppError` and asserts its status end-to-end. Phase 3 exercises
`error_handler` in the pilot with a custom body shape.

**Determinism**: build the pilot twice; the generated file's mtime must not
change on the second build (`write_if_changed` + the rebuild-loop hazard at
`http.rs:163-173` make this the invariant that actually hurts when broken).

**Parity**: `tests/backend_parity.rs` (gen_api byte-identical across
backends) is unaffected — this design changes the servers stage only, and
identically for both backends. It runs anyway; green is required.

**Macros**: a compile-use of `#[derive(HttpError)]` + `#[http(status)]`
lands in the pilot's schema (real-world compile coverage beats a trybuild
harness for a no-op derive).

**Examples**: regenerate iron-log, iron-log-md, notes-kb, tasks-tracker;
notes-kb (or the pilot) gains one annotated variant so a real consumer
exercises layer 1.

## Implementation plan (file-level)

| Phase | Files touched |
|---|---|
| 0 — consolidate | `src/servers/generators/http.rs` |
| 1 — scan + defaults | `src/servers/error_map.rs` (new), `src/servers/mod.rs`, `src/servers/config.rs`, `src/servers/parse.rs` (`ApiFn.error_type`, `extract_result_ok_type`), `src/servers/generators/http.rs`, `src/lib.rs` (`ServersConfig`), `src/pipeline.rs` (stage 5), `src/servers/tests.rs`, `crates/markdown-pilot/tests/http_router.rs`, examples regen |
| 2 — derive + attrs | `crates/ontogen-macros/src/lib.rs`, `Cargo.toml` (pin), `src/lib.rs` (re-export + fix the stale `http` module doc at `src/lib.rs:55-61`), `src/servers/error_map.rs` (attribute grammar), snapshots, pilot schema, site docs + walkthrough |
| 3 — override hook | `src/servers/config.rs` (`ServerGenerator::HttpAxum`), `src/lib.rs`, `src/servers/mod.rs` (match site `:219-231`), `src/servers/generators/http.rs`, optional scaffold module, pilot test, docs |
| 4 — follow-ups | filed as tasks, not designed here |

## Rejected alternatives & open questions

Both live in the epic to avoid drift:
[options considered](planning/epics/http-error-mapping.md#options-considered)
(`IntoResponse`-on-`AppError`, config status maps, scaffold-as-default) and
[open questions](planning/epics/http-error-mapping.md#open-questions)
(store-accessor contract, helper-attribute name, `AppError` name
configurability). One design-level addition to the helper-name question:
if `#[http(...)]` ever collides with another derive's helper in a real
consumer, the fallback is `attributes(http, ontogen)` on our derive —
accepting both spellings is additive and non-breaking.

## Appendix A — call-site inventory

Every `.map_err(|e| err(e.to_string()))` emission site in
`src/servers/generators/http.rs` at time of writing; "routing" per §6.

| Line(s) | Context | Routing after phase 1 |
|---|---|---|
| 206 | unscoped store construction | fallback (`err`) |
| 225, 268-279 | `OpKind::List` (+ paginated 251-266) | `app_error` |
| 285-295 | `OpKind::GetById` | `app_error` |
| 304-314 | `OpKind::Create` | `app_error` |
| 323-334 | `OpKind::Update` | `app_error` |
| 342-352 | `OpKind::Delete` | `app_error` |
| 360, 375-395 | `OpKind::JunctionList` | `app_error` |
| 417-420 | `OpKind::JunctionAdd` — missing-param check + call | `bad_request` + `app_error` |
| 439-440 | `OpKind::JunctionRemove` | `app_error` |
| 678 | generic handler, store construction | fallback (`err`) |
| 715-721 | generic handler result mapping | predicate: `app_error` or fallback |
| 772 | scoped accessor | fallback (`err`) |
| 785, 831, 842, 852, 873, 894, 913 | scoped CRUD arms | `app_error` |
| 1130-1133 | scoped generic handler, accessor | fallback (`err`) |
| 1168-1173 | scoped generic handler result mapping | predicate: `app_error` or fallback |
