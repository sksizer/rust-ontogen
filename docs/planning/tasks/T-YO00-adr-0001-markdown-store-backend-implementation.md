---
type: task
schema_version: '5'
id: T-YO00
status: planning/draft
created: '2026-06-06'
related: []
tags:
- ontological-integration
- store
need_human_review: false
impact: high
complexity: medium
---
# Implement ADR-0001: markdown as a first-class store backend

## Goal

Implement ADR-0001 (`docs/architecture/0001-markdown-as-store-backend.md`,
status proposed): a generation-time `Backend::Seaorm | Backend::Markdown`
choice at the store layer, so generated CRUD routes to the existing
markdown persistence generators instead of SeaORM. Scope per the ADR:
the pre-lift refactors (SeaORM coupling sites out of the dispatcher
path), a `MarkdownIoOutput` IR type, `store::backends::{seaorm,markdown}`
modules, the markdown CRUD emission table, and the cross-backend
semantic contract (String ids, single-record atomicity, identical hook
lifecycle, byte-identical api/servers/clients output). No behavior
change for SeaORM consumers — gated by the existing snapshot tests.
This is Item 0 of the SDLC integration program: nothing downstream
consumes the data plane until this exists.

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

- None — the ADR is accepted design; its pre-lift table enumerates the
  refactor sites.

## Discovery context

ADR-0001 was authored 2026-05-29 with the SDLC planning corpus named as
the obvious pilot; the dev repo's D-DX1Q (2026-06-06) now depends on it
as migration phase 2, Item 0. The ADR's revisit-cadence note (re-evaluate
if unimplemented after a milestone) is answered: the consumer exists.
