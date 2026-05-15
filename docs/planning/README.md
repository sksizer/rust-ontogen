# Planning

How ontogen's work is organized. This directory holds the planning artefacts —
roadmaps live one level up at [`../roadmap.md`](../roadmap.md); architecture
decision records (ADRs) at [`../architecture/`](../architecture/). Everything
here is intentionally lightweight prose; the source of truth for what shipped
is git history, not these files.

```
docs/
├── roadmap.md                     ← tiered capability roadmap
├── architecture/                  ← ADRs (numbered, accepted decisions)
└── planning/
    ├── README.md                  ← this file
    ├── epics/                     ← capability slices, multi-task scope
    └── tasks/                     ← PR-sized units of work
        └── README.md              ← task schema + backlog tables
```

## Two layers of work

**Tasks** are PR-sized: one unit of focused work, one branch, one PR, one
review cycle. A task ships on its own and means something on its own. Most
work in ontogen happens at this granularity. See
[`tasks/README.md`](tasks/README.md) for the task schema and the open / resolved
backlog tables.

**Epics** are capability slices that span more than one task. Each epic is a
single file under `epics/`, with its own goal, scope, AC list, and a checklist
of constituent tasks. Tasks belonging to an epic carry an `epic:` frontmatter
field pointing back to the epic's slug; the epic's `tasks:` frontmatter list
points forward to those task files. The link is bidirectional so you can
navigate either direction without grepping.

Not every task belongs to an epic. Small, standalone work goes straight into
`tasks/` with no epic linkage. Epics earn their keep when the work needs
explicit phase sequencing, cross-task acceptance criteria, or its own design
narrative that doesn't fit inside a single task file.

## Task naming

New tasks use a date-prefixed kebab slug:

```
YYYY-MM-DD-<descriptive-slug>.md
```

The date is the file's `created:` date (UTC). The slug describes the work
("scaffold-ontogen-ts-crate"), not its category. No ticket-tracker IDs in
filenames.

Existing `OF-XXX-*.md` files are legacy artefacts from Pumice-feedback-era
numbering and keep their names. The new convention is for *new* tasks only;
nothing in this repo bulk-renames the legacy files.

## Epic naming

Drafted epics carry **no number prefix** — just a slug:

```
epics/ts-pipeline.md
```

A number prefix (`epics/01-ts-pipeline.md`) is added later, when execution
order against other epics is settled. Drafts and unscheduled epics stay
unnumbered. This avoids reshuffling numbers every time the roadmap moves.

## Frontmatter

### Task frontmatter

```yaml
---
status: <stage> | <stage>/<reason>
created: 2026-05-15
last_reviewed: 2026-05-15
relevance_note: optional one-line note from the most recent review
epic: <epic-slug>           # omit if the task is standalone
completion_note: |          # required on closed tasks; multiline allowed
  Shipped via #N. <one-paragraph summary of what landed>
---
```

Statuses use a `stage` or `stage/reason` form:

| Stage | Meaning | Allowed reasons |
|---|---|---|
| `draft` | Capturing an idea; not yet workable | — |
| `proposed` | Ready for review; not yet committed to the backlog | — |
| `backlog` | Accepted but not scheduled | — |
| `ready` | Spec is complete enough to start; `work-task` will pick it up | — |
| `in-progress` | Active work; a worktree + branch exist | `blocked` (a `<blocked>` body section explains what's stuck) |
| `closed` | Terminal | `done`, `superseded`, `partially-superseded`, `obsoleted`, `relocated`, `no-repro`, `wontdo` |

The flow most tasks follow: `draft` → `proposed` → `backlog` → `ready` →
`in-progress` → `closed/done`. Skipping stages is fine when the task is
obviously workable. `closed/<reason>` is the only terminal state; never set
`status: closed` without a reason suffix.

`completion_note:` is a required body field on every closed task. It records
what shipped, the merged PR number(s), and any follow-up tickets spawned.
Stale `relevance_note:` text is dropped when transitioning to closed.

### Epic frontmatter

```yaml
---
status: <stage> | <stage>/<reason>
created: 2026-05-15
last_reviewed: 2026-05-15
tasks:
  - 2026-05-15-foo.md
  - 2026-05-16-bar.md
  - OF-XXX-legacy.md          # legacy filenames listed verbatim
---
```

Epic statuses use the same `stage` / `stage/reason` form. Epic status
typically tracks the rollup of its tasks: `ready` while at least one task is
ready or earlier; `in-progress` once any task starts; `closed/done` when
every task is closed.

## How the `work-task` and `assess-tasks` skills consume this

[`work-task`](../../.claude/skills/work-task/SKILL.md) picks up a `status: ready`
task (or one named by arg), marks it `in-progress`, spins up a worktree, runs
the implementation through a sub-agent, opens a PR, and — after merge —
moves it to `closed/done` with a `completion_note:` filled in. It looks for
tasks under `docs/planning/tasks/` only.

[`assess-tasks`](../../.claude/skills/assess-tasks/SKILL.md) triages the
unfinished tasks: verifies each is still relevant against the current
codebase, checks that the spec is complete enough to act on, updates
frontmatter (impact, complexity, last_reviewed, plus closed/done when code
shows the work shipped), and produces a ranked recommendation of the next
1-2 tasks to tackle.

Neither skill currently knows about `epics/` directly — they operate on tasks
and read epic linkage only via the `epic:` frontmatter on tasks. Epic-level
state (which task to pick next inside an epic, when an epic transitions to
`closed/done`) is human-driven for now.

## Cross-references

- [`../roadmap.md`](../roadmap.md) — capability tiers; lists epics per tier
- [`../architecture/`](../architecture/) — ADRs for non-obvious decisions
- [`tasks/README.md`](tasks/README.md) — task backlog tables (open / in-progress / closed)
- [`epics/`](epics/) — epic docs
