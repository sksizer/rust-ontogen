# Architecture Follow-ups — 2026-05-03

Optional refinements identified while closing out the 2026-04-25 architecture assessment. None are blocking; each is a code-organization or ergonomics cleanup that can ship independently when there is time or when consumer pressure surfaces.

---

## 1. Move the operation classifier to a neutral home

- **Origin:** Finding #9 close-out. The substance of the duplication finding is resolved (one classifier remains), but the canonical implementation lives at `src/servers/classify.rs` and is consumed sideways by the api layer (`src/api/mod.rs:96` → `crate::servers::classify::classify_by_name_and_params`).
- **Why it's a smell:** The api layer architecturally precedes the servers layer in the pipeline, but reaches into a sibling module's internals for a primitive. After PR #47 the `servers::classify` module is `pub(crate)` — fine for the workspace, but the directional arrow is inverted.
- **Suggested move:** Relocate the classifier to one of:
  - `crate::classify` at the top level of the main crate (lowest-effort, no new public API).
  - `ontogen_core::classify` — most "right" architecturally, since classification is IR-shaped and has no I/O. Requires moving (or accepting an unstructured form of) `Param` into `ontogen-core`, since the classifier currently takes `&[Param]` from `servers::parse`. A pragmatic shape: take `params_len: usize` and `name: &str` only — that is all the current implementation actually uses for the junction-shape heuristics.
- **Risk:** Negligible. Pure refactor of a small (~50 LOC) function with two callers.
- **When to do this:** When touching either classifier or the api↔servers seam for another reason; not worth a dedicated PR otherwise.

---

## 2. Structured `CodegenError` variants (Finding #7)

- **Origin:** Architecture assessment Finding #7 — left open. All `CodegenError` variants except `ExternalTool` wrap a bare `String`.
- **Status:** The assessment itself recommended deferring: *"acceptable for a build-time library where errors are primarily for humans. Low priority until a consumer needs it."*
- **Trigger to revisit:** A consumer (e.g., the planned CLI generator, an IDE plugin, or a tool that wants to retry on transient failures vs. surface user errors) needs to programmatically distinguish failure modes within a layer.
- **Suggested direction when triggered:** Replace `Schema(String)` / `Persistence(String)` / `Store(String)` / etc. with per-layer enums (e.g., `SchemaError::Parse { path, source } | SchemaError::Io(io::Error) | SchemaError::Validation { ... }`). Keep the outer `CodegenError::Schema(SchemaError)` shape so existing matches still compile.

---

## 3. `schema_module_path` deduplication (Finding #12)

- **Origin:** Architecture assessment Finding #12 — resolved by PR #49.
- **What landed:** Promoted the canonical default to `pub const ontogen::DEFAULT_SCHEMA_MODULE_PATH = "crate::schema"`, cross-referenced from both `StoreConfig` and `ApiConfig` rustdocs and the doctest examples. `Pipeline` already eliminates the duplication for the recommended ergonomic path.
- **Open option:** The fully-shared `PipelineConfig` extraction the assessment also suggested was not pursued — it would be a wider breaking change for a low-severity ergonomics issue that is now adequately addressed by the named constant. Revisit only if more shared fields appear across the configs and the duplication grows.

---

## Closed as won't-do (with rationale)

The following items appeared in `ARCHITECTURE_REVIEW.md` (the older, broader review) but are now formally closed as **not worth doing** for this codebase. Each was re-evaluated against the current code on 2026-05-03 and found to have negative ROI; revisit only if a real consumer surfaces the underlying need.

### T12 — Batch `populate_relations` queries (N+1 fix)

- **What it is:** `src/store/gen_crud.rs:292` (`generate_populate_relations`) emits per-entity relation-loading code. Listing N parents results in N×R queries, where R is the number of relations on the entity.
- **Real concern:** Yes. For an entity with junction relations, a `list()` of 100 items can fan out to 100–500 SeaORM round-trips.
- **Why we're not doing it:**
  1. The fix requires emitting a parallel `populate_X_relations_batch(items: &mut [X])` codepath alongside the existing single-entity version, doubling the generated code volume in this section.
  2. Store hooks (`after_load_X`) currently take a single entity. A batch path needs a batch-hook contract too — that's a public API design decision (single hook called per item? a separate `after_load_X_batch`? both?) that should not be made in the abstract.
  3. Consumers calling `list()` would need to be migrated to the new path or it stays unused. Without a real consumer hitting this in production, "fixing" it is speculation.
- **Revisit when:** A consuming project measures real latency from this fan-out and is willing to co-design the batch-hook API contract.

### T14 — Batch rustfmt subprocess calls

- **What it is:** `ontogen_core::utils::write_and_format` (`ontogen-core/src/utils.rs:30`) spawns one `rustfmt` subprocess per generated file, currently called ~4–8 times per `build.rs` invocation across all generators.
- **Why we're not doing it:**
  1. Spawn overhead is ~50 ms/process on macOS. Total: 200–400 ms per build, well under the noise floor of any non-trivial Rust build (which already takes seconds for compile-on-change).
  2. PR #28 already cached `detect_edition()` in a `OnceLock`, eliminating the largest cost inside each spawn.
  3. The viable batching strategies all add fragility:
     - Concatenate-then-split with markers breaks if rustfmt's output ever changes structure across the marker.
     - Format-after-write-then-re-read defeats the in-memory `write_if_changed` optimization that prevents file-watcher rebuild loops.
     - A persistent rustfmt-as-server doesn't exist; rustfmt is a one-shot tool.
- **Revisit when:** Build-script timing becomes a measurable user complaint AND `rustfmt` ships a streaming/server mode.

### T22 — Feature flags for optional generators

- **What it is:** Add Cargo feature flags (e.g., `mcp`, `tauri-ipc`, `axum-http`) so consumers can compile out generators they don't use.
- **Why we're not doing it:**
  - The premise of T22 is "gate heavy framework dependencies." Looking at `Cargo.toml`, the direct deps are: `ontogen-core`, `ontogen-macros`, `syn`, `quote`, `cruet` — and dev-only `insta` and `tempfile`. There are **no** axum/tauri/mcp framework crates pulled in. Generators are pure-Rust modules that emit text.
  - Adding feature flags now would gate code that has no compile-cost or supply-chain reason to be gated. It would add `#[cfg(feature = "...")]` noise across the crate for zero observable benefit.
- **Revisit when:** A future generator pulls in a heavy runtime dep (e.g., a generator that wants to interact with a real schema registry would pull in `reqwest` / `serde_json` at runtime). At that point gating that specific dep behind a feature is well-motivated.
