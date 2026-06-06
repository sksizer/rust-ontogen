---
type: task
schema_version: '5'
id: T-Z0PE
status: planning/draft
created: '2026-06-06'
related: []
tags:
- ontological-integration
- testing
need_human_review: false
impact: high
depends_on:
- '[[T-YO00]]'
complexity: medium
---
# Round-trip fidelity harness over the SDLC planning corpus (zero-diff gate)

## Goal

Prove the markdown round-trip on the real consumer before anything
ships against it: a harness that parses every entity file under the
SDLC repo's `docs/planning/<plural>/` and re-renders it, asserting
`git diff --exit-code` — byte-stable over the live corpus. Must cover:
frontmatter field order and quoting, unknown/extra fields, body prose
including `^summary` block-ids and template H2 sections, wikilink
arrays, compound statuses (`open/ready`), and mixed date/datetime
precision. Any normalization whitelist must be explicit and reviewed;
silence means byte-stable. This gate is release-blocking for the data
plane per dev D-DX1Q — the SDLC corpus is hand- and LLM-authored, and
a lossy writer would corrupt the system of record.

## Today

<Current state of the relevant area as a typed table. One row per touched
location; the Location column uses the five-form grammar documented in
the template's header comment. The Role-today column is a one-line note
on what that location does today (or what's wrong/missing there).

Pure-narrative Todays (no path-bearing rows) may also be expressed as
prose — but tables are the preferred shape because the verifier resolves
each row against the live codebase, so the description doesn't go stale
as code drifts.>

| Location | Role today |
|---|---|
| `path/to/file.ext` | <what this file does today> |
| `path/to/dir/` | <what's in this directory today> |

## Proposed

<Target state after this task ships. Concrete enough that an implementer
can tell when they're done. Not the steps — the destination.>

## Approach

<Numbered, ordered steps to get from Today to Proposed. Each step should
be small enough to commit on its own if useful. Call out any decisions
still open inside the step.>

1. <step>
2. <step>
3. <step>

## Files to touch

<Typed table of every location you expect to touch. Location uses the
same five-form grammar as `## Today` (see header comment). Kind is one
of `new`, `modify`, or `delete`. Change is a one-line note on what
happens there.

The verifier resolves each row by Kind: `new` rows require no existing
file; `modify` and `delete` rows must resolve in the codebase (file /
symbol / dir must exist; glob must expand to ≥1 match). Symbols on
glob rows are rejected.>

| Location | Kind | Change |
|---|---|---|
| `path/to/file.ext` | modify | <what changes> |
| `path/to/new-file.ext` | new | <what gets created> |

## Acceptance criteria

<Each AC must be observable from outside the change — a test that passes,
a user-visible behavior, a removed wart. Avoid "the code is cleaner" style
ACs; pick something verifiable.>

- [ ] AC-1: <criterion>
- [ ] AC-2: <criterion>
- [ ] AC-3: <criterion>

## Out of scope

<Things adjacent to this task that are deliberately NOT being addressed
here. Useful for keeping PR review focused and for future tasks to point
back to. Always required: if scope is obvious and nothing is excluded,
leave a single "- none" bullet so the explicit signal is "scope
considered, nothing to exclude.">

- none

## Dependencies

<Other tasks, branches, infra changes, or external decisions this task
waits on. For hard "B cannot start until A closes" dependencies on
other tasks or epics, also record them in the frontmatter
`depends_on:` array (strict wikilink shape, e.g. `[[T-0010]]`) — the
audit walks that graph for cycle detection. This prose section is the
human-readable narrative; `depends_on:` is the machine-readable
canonical list. Leave a single "- none" bullet if there are none.>

- [[T-YO00]] — the markdown store path the harness exercises
  (recorded in frontmatter `depends_on`).
- [[T-66TG]] — entity definitions for the SDLC corpus arrive via the
  extended-JSON-Schema front-end; until it lands the harness may run
  against interim hand-declared structs.

## Discovery context

Seeded by dev D-DX1Q §Consequences (the fidelity gate) and the
strain-point table in the dev repo's
`docs/planning/decisions/ontological-integration/README.md`.
