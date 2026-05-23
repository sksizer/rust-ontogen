---
type: task
schema_version: '3'
status: planning/draft
created: '2026-05-20'
impact: medium
complexity: small
tags:
- sdlc
- task-work
- tooling
related:
- 2026-05-19-split-clients-from-servers
---
# task-work brief must guard against sub-agents editing task status frontmatter

_Auto-generated from a /sdlc:task-work post-mortem. Review and
promote to `open/ready` before picking up._

## Goal

The implementation sub-agent spawned by `/sdlc:task-work` Step 6 can — and did, during
[[2026-05-19-split-clients-from-servers]] — flip the task's `status` frontmatter to a
terminal value from inside the worktree, even though the close-out is gated on PR merge
in Step 11a. The brief should explicitly forbid touching frontmatter status fields so
this can't recur.

## Today

Step 6 of `/sdlc:task-work` constructs a brief for the implementation sub-agent that
covers scope, acceptance criteria, and commit hygiene, but does not currently include
a guardrail against mutating the task file's `status` frontmatter. From the post-mortem
of [[2026-05-19-split-clients-from-servers]]:

> Sub-agent prematurely marked the task done from inside the worktree (commit 569158b
> set `status: done`, which isn't even a valid schema value — should be `closed/done`,
> and the close-out is gated on PR merge per Step 11a). Required a forward-fix commit
> (`8d8dcd4`) to revert.

The sub-agent was acting reasonably given an under-specified brief. The harness has no
mechanical enforcement of this (the frontmatter validator would catch `status: done`
as an invalid enum value, but only if it ran; and a sub-agent could just as easily
write a valid-but-premature `status: closed/done`).

## Proposed

The Step 6 brief template in `plugin/skills/sdlc/task-work/SKILL.md` includes an
explicit guardrail like:

> Do NOT modify the task file's frontmatter `status` field. Tick AC checkboxes in the
> body and update the body's "Notes" / "Open questions" sections as needed, but leave
> all frontmatter keys alone. The orchestrator transitions `status` after PR merge.

Future task-work runs surface this constraint to the sub-agent, so premature status
flips stop happening at the source rather than getting caught by post-hoc review.

## Approach

1. Read `plugin/skills/sdlc/task-work/SKILL.md` Step 6 and locate the section where
   the brief template is composed.
2. Add the frontmatter guardrail bullet to the brief template. Keep it short — one
   sentence, with a "why" clause referencing the merge-gated close-out in Step 11a.
3. Optionally: extend the same guardrail to other frontmatter keys the orchestrator
   owns (`readiness_verified_at`, `last_reviewed`, `closed_at` if/when added). Defer
   if scope creeps.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `plugin/skills/sdlc/task-work/SKILL.md` | modify | add the frontmatter-status guardrail to |


## Acceptance criteria

- [ ] AC-1: A fresh `/sdlc:task-work` run on a sample task includes the
  "do not modify frontmatter `status`" guardrail in the brief handed to the
  implementation sub-agent.
- [ ] AC-2: The guardrail text cites the orchestrator's responsibility for the
  status transition (so a sub-agent reading the brief understands *why*, not just
  the rule).

## Out of scope

- A pre-commit hook enforcing this mechanically. The brief-level guardrail is the
  first line of defense; a hook is a separate hardening task.
- Renaming or reshaping the status enum.

## Dependencies

- none

## Discovery context

Spawned by /sdlc:task-work post-mortem of [[2026-05-19-split-clients-from-servers]] on 2026-05-20.
