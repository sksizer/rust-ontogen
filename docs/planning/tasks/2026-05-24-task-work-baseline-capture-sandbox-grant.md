---
type: task
schema_version: '3'
status: planning/draft
created: '2026-05-24'
impact: medium
complexity: small
last_reviewed: '2026-05-24'
tags: []
related:
- 2026-05-24-ontogen-ts-configurable-string-literal-quote-style
---
# task-work Step 3a baseline capture needs explicit sandbox permission grant

_Auto-generated from a /sdlc:task-work post-mortem. Review and
promote to `open/ready` before picking up._

## Goal

`/sdlc:task-work` Step 3a invokes `${CLAUDE_PLUGIN_ROOT}/scripts/quality_baseline.py capture …` against the project's `sdlc.yaml` to pin pre-existing drift before any work lands. In runs from sub-agents whose Bash allowlist only covers `just full-check *` (the verb the baseline runner shells out to), the outer `quality_baseline.py` invocation itself is denied — even though every verb it would run is granted. The runner has to skip the baseline and proceed without `--diff-against-baseline` at Step 7, defeating the very gap [[2026-05-21-run-quality-checks-isolates-pre-existing-drift]] was meant to close. Adding an explicit permission entry lets the baseline path run end-to-end without operator intervention.

## Today

Surfaced during [[2026-05-24-ontogen-ts-configurable-string-literal-quote-style]]'s post-mortem:

> The Step-3a quality-check baseline capture (`quality_baseline.py capture ...`) hit a sandbox denial in this run despite the project's `sdlc.yaml` declaring `just full-check`. The runner had to skip the baseline and proceed without `--diff-against-baseline` at Step 7; in this case it didn't matter (no pre-existing drift), but a runner that hits actual drift on a fresh main would have to triage manually.

| Location | Role today |
|---|---|
| `.claude/settings.local.json` | Carries the project's Bash allowlist (verbs like `just full-check *`, `cargo test *`, `gh pr *`). No entry for `quality_baseline.py` or its sibling plugin scripts that task-work shells out to. |
| `${CLAUDE_PLUGIN_ROOT}/scripts/quality_baseline.py` | The plugin script Step 3a invokes; under sandbox profiles that gate on absolute script paths, it needs its own allow entry. |

## Proposed

Project `.claude/settings.local.json` carries an explicit allow entry for `${CLAUDE_PLUGIN_ROOT}/scripts/quality_baseline.py *` (and, defensively, for the small set of other task-work helper scripts that share the same sandbox shape — `preflight_permissions.py`, `start_task.py`, `check_ancestry.py`, `dedup_search.py`). After the grant lands, a fresh task-work run can execute Step 3a capture and Step 7 baseline-diffed gate without the operator having to approve each invocation.

## Approach

1. Inventory the task-work plugin scripts that get shelled out per the SKILL.md (Step 1a hook, Step 3a baseline, Step 3b preflight, Step 5b start_task, Step 8 dedup_search, Step 9 check_ancestry). Settle on whether to allow each one explicitly or use a single wildcard over `${CLAUDE_PLUGIN_ROOT}/scripts/* + ${CLAUDE_PLUGIN_ROOT}/skills/task-work/*`.
2. Add the chosen entries to `.claude/settings.local.json` under `permissions.allow`.
3. Verify by running `/sdlc:task-work` against the next available task and confirming Step 3a's `Baseline captured at <sha>: N pre-existing findings` line surfaces without prompting.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `.claude/settings.local.json` | modify | add allow entries for the task-work plugin scripts so Step 3a baseline capture (and the other helper shell-outs) run without per-invocation prompts. |

## Acceptance criteria

- [ ] AC-1: A fresh `/sdlc:task-work` invocation on a task with `sdlc.yaml` declaring `quality_checks:` runs Step 3a baseline capture to completion and surfaces `Baseline captured at <sha>: N pre-existing findings` without an intermediate `Permission to use Bash has been denied` error.
- [ ] AC-2: Step 7's `run_quality_checks.py --diff-against-baseline "$ORIGIN_MAIN_SHA"` invocation in the same run completes without permission prompts.

## Out of scope

- Re-architecting the sandbox model itself (e.g. switching to skill-level rather than per-script grants). Out-of-scope for this targeted fix.
- A plugin-side change to make the helper scripts invokable via a single permitted entry point. Worth considering as a separate upstream task once consumer projects accumulate enough of these entries to justify it.

## Dependencies

- none

## Discovery context

Spawned by /sdlc:task-work post-mortem of [[2026-05-24-ontogen-ts-configurable-string-literal-quote-style]] on 2026-05-24.

### Dedup search (spawn-from-post-mortem)

Bullet: The Step-3a quality-check baseline capture (quality_baseline.py capture ...) hit a sandbox denial in this run despite the project's sdlc.yaml declaring just full-check. The runner had to skip the baseline and proceed without --diff-against-baseline at Step 7
Keywords searched: diff-against-baseline, quality_baseline, quality-check, full-check, declaring, baseline, step-3a, capture
Excluded: 2026-05-24-ontogen-ts-configurable-string-literal-quote-style
Top candidates (score / status / headline):
  - 8 / closed/wontdo / 2026-05-20-bump-dependencies-rust-and-js-workspaces — Bump Rust and JS/pnpm dependencies across all workspaces
  - 6 / closed/done / 2026-05-20-ontogen-ts-entity-field-type-closure — ontogen-ts: include transitively-referenced field types of schema entities in the long-tail root set
  - 6 / closed/done / 2026-05-24-ontogen-classifier-add-post-attribute-opt-in — ontogen-macros: add #[ontogen::post] to force POST classification on zero-user-param mutating handlers
  - 4 / closed/done / 2026-05-19-split-clients-from-servers — Split client SDK generation out of the `servers` module
  - 4 / closed/done / OF-023-relocate-workspace-members-under-crates — OF-023 - Move workspace members under a `crates/` subdirectory
Decision: SPAWNED
