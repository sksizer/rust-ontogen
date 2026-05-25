---
type: task
schema_version: '3'
status: closed/done
created: '2026-05-24'
impact: medium
complexity: medium
tags:
- breaking-change
- http-method
- ontogen-classifier
- pumice-follow-up
related: []
autonomy: supervised
last_reviewed: '2026-05-25'
completion_note: |
  Shipped via #80 (merge 614e226, 2026-05-25). Breaking-change flip of
  the zero-user-param classifier default from CustomGet to CustomPost,
  with opt-back-into-GET via the new KNOWN_READ_PREFIXES allowlist
  (`get_`, `list_`, `count_`, `exists_`, `find_`, `is_`, `has_`).
  Iron-log's `stats::workout` was renamed to `stats::get_workout` to
  keep GET classification (URL `/api/stats/workout` preserved by the
  naming convention's get_-prefix strip). CHANGELOG flagged the
  breaking change under [Unreleased]; alpha tag bump deferred to next
  release cut. AC-3 (Pumice rebump dropping #[ontogen::http::post]
  annotations) tracked in the companion Pumice PR. Implementation
  started by a stalled task-work sub-agent (resume-detection
  AskUserQuestion hang); operator carried forward directly — friction
  captured in post-mortem and tracked in sksizer/dev#119's tickets.
---
# ontogen: reverse the zero-user-param classifier default from CustomGet to CustomPost, opt-in to GET via known-read prefixes

## Goal

Make ontogen's HTTP-method classifier RFC-7231-correct by default: zero-user-param custom functions classify as `CustomPost` (route as POST) unless the function name matches a known-read prefix (`get_`, `list_`, `count_`, `exists_`, `find_`, `is_`, `has_`, etc.). Today's default is the opposite — zero-param fns are always `CustomGet` — which silently misclassifies action verbs like `pause`, `resume`, `backup`, `reset_all` as safe/idempotent reads when they actually mutate state.

## Today

`src/servers/classify.rs::classify_by_name_and_params` (line 32 onward) runs this hierarchy:

1. Named constants (`list` / `get_by_id` / `create` / `update` / `delete`) → exact mapping.
2. `add_*` / `remove_*` / `list_*` with the right param count → junction kinds.
3. **Zero user params → `CustomGet`** (line 62–64) — the bug.
4. `get_*` with body-carrying first param → `CustomPost` (OF-016).
5. `get_*` with id-like first param → `CustomGet`.
6. Default fallback (non-`get_*` with any user param) → `CustomPost`.

Step 3 is the failure mode. Pumice's `engine::pause(state: &PumiceState)`, `engine::resume(state: &PumiceState)`, `data::backup(state: &PumiceState)`, `data::reset_all(state: &PumiceState)` and ~7 similar functions all hit it and emit as GET (see sksizer/pumice#225 generated `transport/http/generated.rs:586–606`).

| Location | Role today |
|---|---|
| `src/servers/classify.rs` | `classify_by_name_and_params()` — line 62–64 is the offending branch. |
| `src/servers/classify.rs::is_read_op` (line 84-85) | The single-source-of-truth predicate that maps `OpKind` to HTTP semantics. Used by the HTTP generator to decide between `get(...)` and `post(...)`. |
| `tests/` | Existing tests pin the current zero-param-defaults-to-GET behavior (probably). Reversing the default WILL flip those tests. |

## Proposed

Reverse step 3: zero-user-param custom fns default to `CustomPost`. Opt-in to `CustomGet` via a known-read name-prefix allowlist:

- `get_*`, `list_*`, `count_*`, `exists_*`, `find_*`, `is_*`, `has_*` → `CustomGet` (any param count).
- Anything else with zero user params → `CustomPost`.
- Anything else with non-zero user params → `CustomPost` (unchanged from current default).

This aligns with RFC 7231 §4.2.1 ("safe methods" — GET is for retrieval only). It also matches the existing `is_read_op` taxonomy at line 84-85 which already names `CustomGet` (alongside `List`, `GetById`, `JunctionList`) as the read class — the classifier just hasn't been derivin' read-ness from naming as carefully.

This is a **breaking change** for any consumer relying on the current default. The recommended migration is the companion task `[[2026-05-24-ontogen-classifier-add-post-attribute-opt-in]]`: ship `#[ontogen::post]` first as an opt-in, let consumers annotate their mutating handlers on the old behavior, then flip the default in a later alpha tag.

## Approach

1. **Inventory the known-read prefixes.** Codify the allowlist in `classify.rs` as a `const` slice. Start conservative: `get_`, `list_`, `count_`, `exists_`, `find_`, `is_`, `has_`. Note that named-CRUD (`list`, `get_by_id`) is already handled in the earlier match arms, so the prefix check is a fallback for non-named-CRUD reads.
2. **Add a prefix-match helper.** `fn name_implies_read(name: &str) -> bool` that returns true if the name starts with any known-read prefix. Place it alongside `is_read_op`.
3. **Flip the classifier branch.** In `classify_by_name_and_params`, replace the existing line 62–64 with:
   ```rust
   if params.is_empty() {
       return if name_implies_read(name) { OpKind::CustomGet } else { OpKind::CustomPost };
   }
   ```
4. **Update the existing `get_*`-with-body-carrying-param branch.** Currently at line 69–71, this branch only fires when name starts with `get_`. With the new prefix-match helper, the broader `name_implies_read` check could be reused — though the body-carrying-param reclassification has slightly different semantics (about URL-vs-body extraction, not just HTTP method). Decide during implementation whether to keep them separate or unify.
5. **Update fixtures.** Existing tests that pin zero-param-defaults-to-GET will fail; either update them (if the test was validating the old behavior as correct) or migrate them to `#[ontogen::post]` (if the test was just asserting the emitted shape and didn't care about method).
6. **Re-verify against iron-log.** Run `cargo build` in `examples/iron-log/src-tauri/`; capture the diff in generated TS/Rust route declarations. If routes flip, evaluate each — most should genuinely be reads (iron-log has few mutating zero-param handlers); for anything that flips and IS actually a read, decide whether to add a prefix or rename the handler.
7. **Re-verify against Pumice.** sksizer/pumice#225 follow-up: drop any `#[ontogen::post]` annotations that became redundant; the action-verb routes should now emit as POST by default.
8. **Document the change** as breaking in the changelog and in the next alpha tag's release notes.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `src/servers/classify.rs` | modify | reverse the zero-param default; add `name_implies_read` helper with the known-read prefix allowlist. |
| `tests/` | modify | update or migrate existing tests that pinned zero-param-defaults-to-GET. |
| `CHANGELOG.md` | modify | note the breaking-change classifier flip under the next alpha tag's section. |
| `site/src/content/docs/reference/` | modify | document the known-read prefix allowlist in the classifier reference. |

## Acceptance criteria

- [ ] AC-1: Unit test matrix: for each combination of (name prefix in allowlist vs not, param shape: zero / id-like / body-carrying), assert the expected classification. Specifically: `fn pause(state)` → POST; `fn get_state(state)` → GET; `fn list_items()` → GET; `fn backup(state)` → POST.
- [ ] AC-2: `cargo build` in `examples/iron-log/src-tauri/` succeeds; any route diff is reviewed and either accepted (genuinely mutating handlers correctly flipped to POST) or addressed (false-positive read handlers renamed or annotated `#[ontogen::get]` — separate task to introduce that attribute if needed).
- [ ] AC-3: Pumice (sksizer/pumice#225 follow-up): the action-verb routes (`/api/engines/pause`, `/api/data/backup`, etc.) emit as POST by default WITHOUT explicit `#[ontogen::post]` annotations.
- [ ] AC-4: `just full-check` passes on the rust-ontogen branch.
- [ ] AC-5: `CHANGELOG.md` flags this as a breaking change; the next alpha tag is bumped to reflect the API surface change.

## Out of scope

- **Adding `#[ontogen::get]`** as a symmetric opt-in to force GET classification on functions whose names happen to look mutating but actually are reads. Likely needed if AC-2 surfaces any false-positives in iron-log; file as a sibling task if so.
- **Method semantics for PUT / DELETE / PATCH** — the named-CRUD verbs already cover `update`/`delete`; finer-grained PATCH support is a separate concern.
- **The `is_read_op` predicate's behavior** — unchanged. This task only changes which `OpKind` variant the classifier returns; the OpKind → HTTP-method mapping in `is_read_op` is correct as-is.

## Dependencies

- Ship after `[[2026-05-24-ontogen-classifier-add-post-attribute-opt-in]]`. The opt-in attribute gives consumers a forward-compatible migration path: they can annotate their mutating zero-param handlers on the OLD default, ship it under the old behavior, then no-op when the default flips. Without this sequencing, the default-flip is a hard breaking change with no per-consumer escape hatch.

## Discovery context

- Surfaced by sksizer/pumice#225's inline review comment at `src-tauri/src/api/transport/http/generated.rs:590` (sksizer, 2026-05-24): "a lot of these should probably be posts since they mutate data not gets. what are the ontogen generation rules in that regard?"
- The current behavior is documented at `src/servers/classify.rs:61-64`: "Zero-param custom fns are always read-shaped (no body to carry)." That comment captures the old reasoning — "no body, must be a read" — which conflates "syntactic body-presence" with "HTTP semantic safety." A function with no body can still mutate state (any zero-param action verb does this).
- Companion to `[[2026-05-24-ontogen-classifier-add-post-attribute-opt-in]]`: the conservative opt-in lands first; this principled default lands second once consumers have had a chance to annotate.

## Post-mortem

_Captured 2026-05-24. PR: pending._

### Acceptance criteria coverage

- AC-1: auto — new test `test_classify_zero_param_prefix_matrix` in `src/servers/tests.rs` covers all 7 known-read prefixes plus 7 action-verb counterexamples; `test_classify_no_params_defaults_to_post` covers the directional flip; the existing `test_force_method_post_overrides_classifier` and `test_post_attr_*` were updated to use `get_status()` (read prefix) where they previously used `status()` (would now default to POST).
- AC-2: agent-manual — `cargo build` in `examples/iron-log/src-tauri/` succeeds. One route diff surfaced: `stats::workout(_store)` was renamed to `stats::get_workout(_store)` so it keeps `get(stat_get_workout)` classification (the URL `/api/stats/workout` is preserved because the naming convention strips the `get_` prefix when computing the URL slug). No frontend code referenced the camelCase generated function, so the rename is non-breaking at the consumer surface.
- AC-3: deferred-user — verification lives in the Pumice consumer repo (sksizer/pumice#225 follow-up). Once this PR lands and Pumice bumps the ontogen dependency, the user can drop the `#[ontogen::http::post]` annotations from the action-verb handlers (`pause`, `resume`, `backup`, etc.) and confirm the regenerated transport emits POST without the explicit attribute.
- AC-4: auto — `just full-check` passes (fmt, clippy with `--deny warnings`, 221 unit tests + 3 integration tests + 1 doctest all green). Baseline-gated quality-check runner reports `OK`.
- AC-5: partial — CHANGELOG.md has the breaking-change entry under `[Unreleased]` (cites the new prefix allowlist, the consumer-side migration story, and the companion-task sequencing). The alpha tag bump itself is a release-prep step rather than a code change and will be cut as part of the next `alpha0.0.3` tag alongside any other queued breaking changes.

### What worked

- The companion task `[[2026-05-24-ontogen-classifier-add-post-attribute-opt-in]]` shipping the `#[ontogen::http::post]` opt-in first was the right sequencing — consumers had a forward-compatible escape hatch before the default flipped.
- The `KNOWN_READ_PREFIXES` const slice + `name_implies_read()` helper keeps the prefix allowlist in one place; future additions (or the symmetric `#[ontogen::http::get]` if it ever lands) extend the same surface.
- The `get_*`-with-body-carrying-param branch (OF-016) was deliberately left UNCHANGED — `name_implies_read` only gates the zero-param branch, not the body-carrying reclassification. Avoided scope creep and a separate semantic question (should `list_things(filter: FilterInput)` reclassify too?).

### Friction and automation gaps

- The `/sdlc:task-work` sub-agent dispatch stalled for ~50 minutes on Step 2's resume-detection `AskUserQuestion`, which sub-agents can't interactively answer. The operator had to `TaskStop` the sub-agent and carry the work forward directly. This is the same verdict-contract escape pattern that motivated `[[2026-05-24-task-work-sub-agent-verdict-contract-escape-recurrence]]`, but with a different proximate cause (interactive prompt vs. intermediate marker leak). Worth a follow-up: when sub-agents hit `AskUserQuestion` they should fail with a structured ERROR rather than spin indefinitely. (Upstream-plugin: sdlc — cross-repo dispatch not invoked from this orchestrator context.)
- The pre-flight permissions probe didn't catch missing permissions for the SDLC plugin's own scripts (`classify_pr.py`, `start_task.py`, `quality_baseline.py`, etc.); a dispatched sub-agent hit those denials mid-flow. Already tracked in `[[2026-05-24-task-work-preflight-permissions-probe-extension-for-skill-internal-scripts]]`; this incident is one more data point.
- The sub-agent did meaningful work on the implementation (classify.rs, tests.rs, iron-log rename) but never committed before stalling. After `TaskStop`, the operator inherited a dirty worktree with 175 lines of unstaged changes. Recoverable, but the failure mode is "uncommitted progress lost to anomaly state" which is a worse end-state than "verdict surfaced cleanly with the work captured."

### Spawned follow-up tasks

- none — the friction items above are already tracked in existing tickets (`[[2026-05-24-task-work-sub-agent-verdict-contract-escape-recurrence]]`, `[[2026-05-24-task-work-preflight-permissions-probe-extension-for-skill-internal-scripts]]`).
