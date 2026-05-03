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

- **Origin:** Architecture assessment Finding #12 — being addressed in a separate PR alongside this doc.
- **What's being done now:** Documented to make the cross-reference explicit; the substantive change lives in the PR that closes Finding #12.

---

## Notes on items intentionally **not** captured here

The following appear in `ARCHITECTURE_REVIEW.md` (the older, broader review) and remain deferred for reasons documented in commit history:

- **T12 — Batch `populate_relations` queries (N+1 fix).** Real performance concern, but the fix needs a cross-example consumer-helper contract design before code changes. Revisit when a consuming project hits the N+1 in practice.
- **T14 — Batch rustfmt subprocess calls.** PR #28 cached the edition-detection lookup, removing the worst hotspot. Remaining gain is modest, regression risk on output formatting is real.
- **T22 — Feature flags for optional generators.** Premature for 0.1.0; only useful once consumers exist who want to compile out a specific transport (e.g., MCP or Tauri IPC).

These three are not "follow-ups" in the same sense — they are *intentional non-goals for the current release*. Listing them here for traceability only.
