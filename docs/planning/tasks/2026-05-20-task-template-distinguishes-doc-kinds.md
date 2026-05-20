---
type: task
schema_version: '2'
status: planning/draft
created: '2026-05-20'
impact: low
complexity: small
tags:
- readiness
- sdlc
- tooling
related:
- 2026-05-19-split-clients-from-servers
---
# task template should distinguish API-reference docs from design-narrative docs in Files to touch

_Auto-generated from a /sdlc:task-work post-mortem. Review and
promote to `open/ready` before picking up._

## Goal

When a task spec lists docs under "Files to touch", it implicitly treats every
doc as an API reference that must track the actual code. But projects also
carry design-narrative docs (`proposal.md`, `walkthrough.md`, ADRs) that are
deliberate snapshots of intent and should *not* track the live API. Refactor
specs that default to "update all referenced docs" can spend effort drifting
design narratives, or — worse — overwrite intent with implementation details.
A convention in the task template + the readiness gate would surface the
distinction at spec-write time.

## Today

From the post-mortem of [[2026-05-19-split-clients-from-servers]]:

> Spec defaulted to "update docs/proposal.md and docs/walkthrough.md" but those
> turned out to be aspirational design narratives, not API reference. The
> default was wrong; updating them would have introduced compile-checked drift
> between aspirational design and current code. Future spec-writing
> convention: distinguish "API reference docs" (must track actual API) from
> "design narrative docs" (deliberate snapshot of intent) so the readiness
> gate or task-define can prompt appropriately.

The task template (`plugin/entities/task/template.md`) has a single
"Files to touch" section that does not prompt the spec author to mark
the *kind* of doc each entry is. `/sdlc:task-define` and the implementation-
ready contract (`plugin/entities/task/implementation-ready.md`) don't
distinguish either.

## Proposed

The task template (and, optionally, the readiness contract) introduces a
lightweight convention for marking docs by kind in "Files to touch" — e.g.

```
- `site/src/content/docs/guides/foo.mdx` (API reference) — update API examples
- `docs/proposal.md` (design narrative) — confirm no edits needed; spot-check
```

`/sdlc:task-define` prompts on doc-shaped paths ("Is this an API reference
that must track the live code, or a design narrative that is a deliberate
snapshot of intent?") and writes the marker into the bullet. The
implementation sub-agent then knows to leave design narratives alone unless
the spec explicitly says otherwise.

## Approach

1. Update `plugin/entities/task/template.md` "Files to touch" section's
   inline guidance to introduce the `(API reference)` / `(design narrative)`
   marker convention.
2. Update `plugin/skills/sdlc/task-define/SKILL.md` (or equivalent) so that
   when it processes doc-shaped paths in Files to touch, it asks the user
   to classify each one and writes the marker.
3. Optionally extend `plugin/entities/task/implementation-ready.md` to call
   out that doc kinds should be marked when present. Defer if scope creeps.

## Files to touch

- `plugin/entities/task/template.md` (API reference) — add the doc-kind
  marker convention to the "Files to touch" inline guidance.
- `plugin/skills/sdlc/task-define/SKILL.md` (API reference) — prompt to
  classify each doc-shaped path during interactive definition.
- `plugin/entities/task/implementation-ready.md` (API reference) — optional;
  reference the convention so the readiness gate can warn on un-marked
  doc-shaped entries.

## Acceptance criteria

- [ ] AC-1: A fresh task created via `/sdlc:task-new` or `/sdlc:task-define`
  with doc-shaped paths in "Files to touch" gets each path marked
  `(API reference)` or `(design narrative)`.
- [ ] AC-2: The template's inline guidance in "Files to touch" documents
  the convention so spec authors writing tasks by hand also use it.

## Out of scope

- Mechanical enforcement (a validator that errors on un-marked doc paths).
  The convention is descriptive; enforcement is a separate hardening task.
- Defining a taxonomy beyond the two-kind distinction. If projects need
  more (e.g. "tutorial" vs "cookbook" vs "concept"), file separately.

## Dependencies

- none

## Discovery context

Spawned by /sdlc:task-work post-mortem of [[2026-05-19-split-clients-from-servers]] on 2026-05-20.
