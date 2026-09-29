# `sdlc.yaml` reference

> **Generated** from `SdlcConfigSchema` and its key meta by the `docs`
> skill (`sdlc docs generate sdlc-yaml`). **Do not hand-edit** — edit
> `lib/config/load.ts`/`lib/config/meta.ts` and regenerate.

Every top-level `sdlc.yaml` key, generated from `SdlcConfigSchema`
(`lib/config/load.ts` in the sdlc source) and the `.meta()` each key carries
(`lib/config/meta.ts`). The prose guide to what each section is for is
`conventions/sdlc-yaml.md` in the sdlc source. From a project, `sdlc config list`
shows the effective value of every key and `sdlc config explain <key>` shows one.

Meta columns: **layers** are the files that may set the key (`project` =
`sdlc.yaml`, `local` = `sdlc.local.yaml`, `machine` = the machine config,
`subtree` = a nested `sdlc.yaml`); **applies via** is `read` when consumers read
the key at run time, or `apply:<applier>` when `sdlc apply` must run before a
change is live. A key whose consumers are all `planned:` is reserved: it is
accepted but nothing reads it yet.

## Keys

| Key | Category | Shape | Applies via | Layers |
|---|---|---|---|---|
| [`workflows`](#workflows) | lifecycle | `map<string, list<object> \| object>` | `read` | project, local |
| [`verify`](#verify) | lifecycle | `object` | `read` | project, local, subtree |
| [`chain`](#chain) | lifecycle | `object` | `read` | project, local |
| [`hooks`](#hooks) | lifecycle | `map<'pre-commit' \| 'pre-merge-commit' \| 'prepare-commit-msg' \| 'commit-msg' \| 'post-commit' \| 'pre-rebase' \| 'post-checkout' \| 'post-merge' \| 'pre-push' \| 'post-rewrite', list<object> \| object>` | `apply:hooks` | project, local |
| [`orchestrator`](#orchestrator) | automation | `object` | `read` | project, local |
| [`pr_check`](#pr_check) | automation | `object` | `read` | project, local |
| [`pr_update`](#pr_update) | automation | `object` | `read` | project, local |
| [`pr_review`](#pr_review) | automation | `object` | `read` | project, local |
| [`notify`](#notify) | automation | `object` | `read` | project, local |
| [`usage`](#usage) | automation | `object` | `read` | project, local |
| [`forge`](#forge) | integration | `object` | `read` | project, local |
| [`host`](#host) | integration | `object` | `read` | project, local |
| [`harness`](#harness) (reserved) | integration | `object` | `apply:harness` | project, local |
| [`mcp`](#mcp) | integration | `object` | `apply:mcp` | project, local |
| [`issues`](#issues) (reserved) | integration | `object (no fields)` | `read` | project, local |
| [`task`](#task) | planning | `object` | `read` | project, local |
| [`backlog`](#backlog) | planning | `object` | `read` | project, local |
| [`knowledge`](#knowledge) (reserved) | planning | `object (no fields)` | `read` | project, local |
| [`lease_authority`](#lease_authority) | infrastructure | `string` | `read` | project |
| [`docs_site`](#docs_site) | infrastructure | `string` | `read` | project, local |
| [`paths`](#paths) | infrastructure | `map<string /^[A-Za-z][A-Za-z0-9_]*$/, string>` | `read` | project, local, machine |
| [`repos`](#repos) | infrastructure | `list<string>` | `read` | project, local, machine |
| [`checkout`](#checkout) | infrastructure | `object` | `read` | project, local, machine |

## Lifecycle

### workflows

Named lifecycle-stage workflows: implement, check, judge, pr-review, pr-respond, pr-update, close-out, define (`lib/config/workflows.ts`'s `BUILTIN_WORKFLOWS`), plus any project-defined name — including the `setup`/`setup-hooks`/`check` baseline names that used to live under their own `verbs:` section, retired: everything lives here now, resolved via `resolveWorkflow(name, projectRoot)`. A step is `{skill}`, `{run}`, `{script}`, `{prompt}`, or a bare string (sugar for `{script: <string>}` — a verb's old shell-command list is spelled identically, just under this key). Every built-in name ships a default step list IN CODE, not here — this map only carries PROJECT overrides. An entry replaces the built-in outright (a bare array, or an object's `steps` key — the two are equivalent), or wraps it with `prepend`/`append` step lists; setting both a full list and `prepend`/`append` on one entry fails validation. A `{run: <name>}` step splices in ANOTHER workflow's already-resolved steps, recursively (cycle-guarded) — not a verb lookup. An entry may also set `engine`/`host` with no `steps` at all — the one place a workflow's session engine/host live, read by `resolveWorkflow` and dispatched by the orchestrate loop and `sdlc session launch` alike, replacing the retired `orchestrator.workflows`/`orchestrator.review.engine` settings (see `WorkflowEntryObjectSchema`'s own doc). A name absent from this map, and not one of the eight built-ins, resolves to an empty step list. This section is data only: it carries no runner logic, so `sdlc session launch` and a later flowline runner can execute the same resolved list.

| Meta | Value |
|---|---|
| Shape | `map<string, list<object> \| object>` |
| Default | `{}` |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, loop |
| Consumers | `lib/config/workflows.ts#resolveWorkflow`<br>`lib/config/hooks.ts#resolveWorkflow`<br>`lib/services/config/config-file.ts#resolveWorkflow`<br>`lib/services/workflow/ops/show.ts#resolveWorkflow`<br>`lib/services/workflow/ops/run.ts#resolveWorkflow`<br>`lib/services/orchestrator/step-runner.ts#resolveWorkflow`<br>`lib/services/session/ops/launch.ts#resolveWorkflow`<br>`lib/services/git/arm-worktree.ts#resolveWorkflowCommands`<br>`lib/services/pr/ops/review.ts#resolveWorkflowCommands`<br>`lib/services/pr/ops/update.ts#resolveWorkflowCommands`<br>`lib/services/quality/ops/run.ts#resolveWorkflowCommands`<br>`lib/services/quality/baseline.ts#resolveWorkflowCommands`<br>`lib/services/project/ops/doctor.ts#resolveWorkflowCommands`<br>`lib/model/entities/task/ops/preflight-permissions.ts#resolveWorkflowCommands` |

### verify

Settings for `sdlc verify changes`, which checks a changeset against every instruction file (`CLAUDE.md`/`AGENTS.md`) and standard whose scope encloses a changed file. A directory beneath the project root may carry its own `sdlc.yaml` whose `verify:` block shallow-merges over this one for rule sources scoped inside it. Only `engine`, `model`, and `timeout_ms` nest: `concurrency` and `exclude` govern the whole run, and `blocking`/`severity_floor` decide one process exit code, so a subtree cannot opt itself out of a blocking run.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local, subtree |
| Runs in | cli, loop |
| Consumers | `lib/services/verify/ops/changes.ts#verify`<br>`lib/services/verify/nested-config.ts#verify` |

Default:

```yaml
verify:
  engine: claude
  concurrency: 32
  blocking: false
  severity_floor: violation
  exclude: []
  max_prompt_bytes: 400000
  max_files_per_job: 12
  timeout_ms: 600000
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `engine` | `'claude' \| 'codex' \| 'gemini' \| 'opencode'` | `"claude"` | Headless agent `sdlc verify changes` dispatches each judgement to, translated through `@sksizer/agent-call`. The named CLI must be on PATH; an engine that is not installed fails every job rather than silently passing. |
| `model` | `string` | — | Concrete model id passed to the engine, e.g. `claude-haiku-4-5-20251001`. Absent means the engine's own default. Verification fans out one call per governing rule source, so a small fast model is usually the right trade. |
| `concurrency` | `integer >= 1` | `32` | Ceiling on judgement calls in flight, not a target — a run never opens more than it planned. Purely a rate limit: verification is read-only and no job mutates the working tree, so jobs cannot conflict. Coupled to `max_files_per_job`, and the coupling decides whether sharding helps: a judgement costs about the same wall clock whatever its prompt size, so a run costs roughly `ceil(jobs / concurrency)` of them. Measured on a 63-file branch: 12 jobs at a cap of 6 took 190s, 30 shards at a cap of 12 took 253s, and those same 30 at a cap of 32 took 155s. Lower it if the engine rate-limits, but lowering it below the planned job count is what makes sharding cost more than it saves. |
| `blocking` | `boolean` | `false` | Whether findings at or above `severity_floor` make `sdlc verify changes` exit non-zero. Default false: the run reports and gets out of the way, which is the right posture until a project trusts its rule corpus enough to gate on it. |
| `severity_floor` | `'violation' \| 'tension' \| 'drift'` | `"violation"` | Least serious finding that can fail a blocking run. `violation` (the change contradicts a rule) only; `tension` also admits defensible-but-contrary changes; `drift` admits everything. No effect when `blocking` is false. |
| `exclude` | `list<string>` | `[]` | Glob patterns removed from the changeset before rule sources are selected. Use for generated or vendored paths that no rule meaningfully governs. A rule source left with no files is dropped and costs nothing. |
| `max_prompt_bytes` | `integer >= 0` | `400000` | Prompt size, in bytes, past which a judgement is reported as oversized. Advisory: nothing is truncated and no job is skipped, because dropping files silently is a worse failure than a degraded judgement. It exists because a judge asked about two hundred files reasons worse about each one than a judge asked about five, and nothing else surfaces that — you get vaguer findings and no signal that scope caused it. With `max_files_per_job` sharding a wide source into slices, this is now a backstop rather than the primary control: it trips on a job whose few files are individually enormous, which sharding by file count cannot prevent. Set 0 to disable. |
| `max_files_per_job` | `integer >= 0` | `12` | Largest number of changed files one judgement may carry. A rule source governing more is split into that-many-file shards, each judged against the same rules and dispatched concurrently. The binding constraint is attention, not context: a judge handed sixty diffs reasons worse about each one than a judge handed a dozen, well before any window fills, and the sixty-file job also sets the run's wall clock because everything else finishes while it is still reading. Findings are attributed to the rule source, not the shard, so a split is invisible in the report. Set 0 to disable sharding. |
| `timeout_ms` | `integer >= 1000` | `600000` | Per-judgement wall clock in milliseconds. Applied by `@sksizer/agent-call`, which no agent CLI provides itself. Generous because a job that times out contributes no findings at all, which reads as a clean pass from the rule source that produced it. Applies per judgement, so a sharded source gets this budget for each of its slices rather than sharing one. A timeout degrades the run rather than failing it: an unreachable agent is not evidence of a violation. |

### chain

The `sdlc` chain (capture → triage → define → ready → implement → check → judge → review → respond → merge → close-out): a fixed, ordered 11-stage pipeline (`lib/config/chain.ts`'s `CHAIN_STAGE_NAMES`), each bound to a built-in workflow or explicitly `null` for a checkpoint stage — see [[D-0019-sdlc-chain-stages-and-labels]] for the full stage table and rationale. `resolveChain(projectRoot)` returns the effective 11-entry binding list, each stage tagged with the layer (`project`/`local`/`default`) that produced it. `stages` lets a project override one stage's bound workflow; it can never change the stage list itself.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | unset |
| Applies via | `read` |
| Layers | project, local |
| Runs in | loop, cli |
| Consumers | `lib/config/chain.ts#resolveChain`<br>`lib/services/orchestrator/ticks/issues.ts#runIssuesTick` |

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `stages` | `map<'capture' \| 'triage' \| 'define' \| 'ready' \| 'implement' \| 'check' \| 'judge' \| 'review' \| 'respond' \| 'merge' \| 'close-out', object>` | — | Per-stage override of `lib/config/chain.ts`'s `DEFAULT_CHAIN_BINDINGS`, keyed by one of the 11 fixed stage names (`capture`, `triage`, `define`, `ready`, `implement`, `check`, `judge`, `review`, `respond`, `merge`, `close-out`). An unknown key fails validation — a project may change a stage's bound workflow, never add, remove, reorder, or rename a stage. |

### hooks

Git hooks sdlc runs, keyed by client-side hook name (`pre-commit`, `commit-msg`, `post-checkout`, `pre-push`, …), each resolved by `resolveHook` (`lib/config/hooks.ts`) and run by `sdlc hooks run <hook>`. An entry is a bare array (full replacement) or `{steps, prepend, append}` (replace vs. wrap the resolved list — `sdlc.local.yaml` wraps or replaces whatever `sdlc.yaml` resolved to, each step tagged with its own `{layer, mode}` provenance; hooks have no `env` layer and no built-in default). A hook step is a workflow `run`/`script` step (a git hook cannot drive a session, so `skill`/`prompt` are rejected — including one spliced in from a `run:` target) plus `when` (`always` | `deps-missing`), `on_fail` (`warn` | `block` | `park`) and an optional `name`. Steps come from the repository, so a hook with steps to run needs the repo trusted (`sdlc repo trust`) and exits 21 otherwise; a hook with no steps exits 0 without a trust check. Editing a step list takes effect on the next run; adding or removing a hook name takes effect only after `sdlc apply` rewrites the hook shims.

| Meta | Value |
|---|---|
| Shape | `map<'pre-commit' \| 'pre-merge-commit' \| 'prepare-commit-msg' \| 'commit-msg' \| 'post-commit' \| 'pre-rebase' \| 'post-checkout' \| 'post-merge' \| 'pre-push' \| 'post-rewrite', list<object> \| object>` |
| Default | unset |
| Applies via | `apply:hooks` |
| Layers | project, local |
| Runs in | hook, cli |
| Consumers | `lib/config/hooks.ts#resolveHook`<br>`lib/services/hooks/run.ts#runHooks`<br>`lib/services/apply/hooks.ts#hooksApplier` |

## Automation

### orchestrator

Configures the categorized in-flight limits enforced by `/sdlc:orchestrate` (via `sdlc task inflight`) and the deterministic `sdlc orchestrate run --loop work,prs,merges` tick loop. Missing block or missing keys default to `max_implementations: 5`, `max_awaiting_review: 20`, `review.max_rounds: 3`. An empty `orchestrator: {}` block is treated the same as missing. `.strict()` at this level and on `review`/`pr_filters` below rejects unknown keys — including a leftover `orchestrator.notify` block from before notification config moved to the top-level `notify:` section.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local |
| Runs in | loop, cli, session |
| Consumers | `lib/services/orchestrator/ops/run.ts#interval_seconds`<br>`lib/services/orchestrator/ticks/prs.ts#pr_filters`<br>`lib/services/orchestrator/ticks/_pr_action.ts#max_rounds`<br>`lib/model/entities/task/ops/inflight.ts#max_implementations`<br>`lib/services/dispatch/router.ts#router`<br>`lib/services/orchestrator/ticks/prs.ts#router`<br>`lib/services/orchestrator/ticks/merges.ts#router` |

Default:

```yaml
orchestrator:
  max_implementations: 5
  max_awaiting_review: 20
  loops:
    work: true
    prs: true
    merges: true
    issues: true
  interval_seconds: 300
  review:
    max_rounds: 3
  pr_filters:
    exclude_labels: []
    exclude_authors: []
  router:
    enabled: true
    routes:
      - live
      - resume
      - fresh
    live_enabled: false
    max_deliveries_per_pr: 20
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `max_implementations` | `integer >= 0` | `5` | Hard cap on the count of tasks currently in the `implementing` category (status `in-progress`, no open PR). When the count is at or above this limit, the orchestrator does NOT dispatch any new `/sdlc:task-work` sub-agents this tick. This is the only limit that blocks dispatch. |
| `max_awaiting_review` | `integer >= 0` | `20` | Informational ceiling on the count of tasks in the `awaiting-review` category (status `in-progress` AND an open PR exists for `task/<basename>`). When the count is at or above this limit, the orchestrator's digest line records `caps-reached=max_awaiting_review` as a warning so the human knows the review queue is saturated. Does NOT block new implementations — awaiting-review tasks have already handed control back to the human; they don't consume implementation slots. |
| `loops` | `object` | `{"work":true,"prs":true,"merges":true,"issues":true}` | Per-loop enable flags for `sdlc orchestrate run --loop <names>`. A name not passed to --loop never runs regardless of this flag; a name that IS passed but whose flag here is false is dropped from the run (absent from the `loop` field of the op output) rather than run. |
| `interval_seconds` | `integer >= 1` | `300` | Default --interval (seconds) for `sdlc orchestrate run` when the flag is omitted. Ignored under --once. |
| `review` | `object` | `{"max_rounds":3}` | Configures how the `prs`/`merges` ticks dispatch the built-in `pr-review` workflow. Missing block or missing keys default to `max_rounds: 3`, no `rubric_path` (generic inlined rubric only). The engine that dispatch runs `pr-review` under is set at `workflows.pr-review.engine`, not here — a static per-project value, deliberately separate from `pr_review.engine` (the interactive `sdlc pr review` engine) so an unattended review can run a different model from the one the PR author used; the loop does NOT infer an engine from the PR author (nothing records which engine authored a PR). (An earlier `command` field here backed the OLD `/sdlc:orchestrate` Step 2a opt-in review phase; that phase and its `get-review-policy` op were retired when the orchestrate skill was rewritten as a thin `orchestrate run` wrapper.) |
| `pr_filters` | `object` | `{"exclude_labels":[],"exclude_authors":[]}` | A PR matching either filter is skipped by the `prs`/`merges` ticks entirely — not even classified — for excluding e.g. dependabot or a human-owned exploratory PR from autonomous handling. |
| `router` | `object` | `{"enabled":true,"routes":["live","resume","fresh"],"live_enabled":false,"max_deliveries_per_pr":20}` | Configures the Router: the delivery fallback chain, whether the optional live-send route may run, and whether the Router is wired into the `prs`/`merges` ticks at all. Missing block or missing keys default to enabled, `[live, resume, fresh]`, `live_enabled: false`, `max_deliveries_per_pr: 20`. |

##### `orchestrator.loops`

Per-loop enable flags for `sdlc orchestrate run --loop <names>`. A name not passed to --loop never runs regardless of this flag; a name that IS passed but whose flag here is false is dropped from the run (absent from the `loop` field of the op output) rather than run.

| Field | Shape | Default | Description |
|---|---|---|---|
| `work` | `boolean` | `true` | Whether the `work` loop (drive ready tasks through implement/check/judge) may run. |
| `prs` | `boolean` | `true` | Whether the `prs` loop (classify open PRs, dispatch review/respond) may run. |
| `merges` | `boolean` | `true` | Whether the `merges` loop (close out merged PRs, update stale ones, respond to conflicts) may run. |
| `issues` | `boolean` | `true` | Whether the `issues` loop (walk open, chain-labeled GitHub issues one stage at a time up to their autonomy ceiling) may run. |

##### `orchestrator.review`

Configures how the `prs`/`merges` ticks dispatch the built-in `pr-review` workflow. Missing block or missing keys default to `max_rounds: 3`, no `rubric_path` (generic inlined rubric only). The engine that dispatch runs `pr-review` under is set at `workflows.pr-review.engine`, not here — a static per-project value, deliberately separate from `pr_review.engine` (the interactive `sdlc pr review` engine) so an unattended review can run a different model from the one the PR author used; the loop does NOT infer an engine from the PR author (nothing records which engine authored a PR). (An earlier `command` field here backed the OLD `/sdlc:orchestrate` Step 2a opt-in review phase; that phase and its `get-review-policy` op were retired when the orchestrate skill was rewritten as a thin `orchestrate run` wrapper.)

| Field | Shape | Default | Description |
|---|---|---|---|
| `max_rounds` | `integer >= 1` | `3` | Cap on `prs`/`merges`-tick review/respond round-trips per PR (tracked in the `review_rounds` field of the per-PR cursor) before the loop stops dispatching `pr-review`/`pr-respond` for that PR and fires a `pr.ci_failed` or `pr.needs_response` notification instead of looping forever — repeating every tick thereafter for as long as the PR stays stuck (see the `notify:` section's own event descriptions). |
| `rubric_path` | `string` | — | Project-root-relative path to a code-review rubric doc that `/sdlc:pr-review` reads in full and judges the diff against in its Step 2. Unset (the default for a project that has not configured one) falls back to the eight dimensions and severity scale inlined directly in the skill's own prose, with no project-specific "How to apply" guidance beyond that. This repository sets it to `docs/planning/standards/S-0017-code-review-rubric.md`. |
| `response_policy` | `string` | — | Path (repo-relative) to a standard document (e.g. `docs/planning/standards/S-0018-review-response-policy.md`) the `pr-respond` skill follows when triaging review comments — fix / decline-with-a-reply / verify-before-acting / minimize-churn / loop-guard-at-`max_rounds`. Unset falls back to a short built-in policy inlined in the skill. Read via `sdlc config get orchestrator.review.response_policy`; also surfaced in the Router feedback bundle (`lib/services/dispatch/feedback.ts`) as a path plus one-line summary, so a resumed or live session applies it too. |

##### `orchestrator.pr_filters`

A PR matching either filter is skipped by the `prs`/`merges` ticks entirely — not even classified — for excluding e.g. dependabot or a human-owned exploratory PR from autonomous handling.

| Field | Shape | Default | Description |
|---|---|---|---|
| `exclude_labels` | `list<string>` | `[]` | PR labels whose PRs the ticks skip. |
| `exclude_authors` | `list<string>` | `[]` | PR author logins whose PRs the ticks skip. |

##### `orchestrator.router`

Configures the Router: the delivery fallback chain, whether the optional live-send route may run, and whether the Router is wired into the `prs`/`merges` ticks at all. Missing block or missing keys default to enabled, `[live, resume, fresh]`, `live_enabled: false`, `max_deliveries_per_pr: 20`.

| Field | Shape | Default | Description |
|---|---|---|---|
| `enabled` | `boolean` | `true` | Whether the `prs`/`merges` ticks route NEEDS-RESPONSE, CI-FAILED, and conflict feedback through the Router (`lib/services/dispatch/`) into a PR's producing session before falling back to a fresh `pr-respond` dispatch. `false` restores the old cold-dispatch-only behavior for every tick, though `sdlc pr route`/`sdlc task dispatch` can still be invoked manually regardless of this flag. |
| `routes` | `list<'live' \| 'resume' \| 'fresh'>` | `["live","resume","fresh"]` | Ordered delivery fallback the Router tries for a subject with a resolvable session, first list entry that succeeds wins: `live` (send into a running session via whichever host opened it — `orca`/`tmux` today, any host with a `send` part), `resume` (headless native resume of the originating harness session with the feedback as the follow-up prompt — `claude -p --resume <id> -- <message>` / `codex exec resume <id> -- <message>`), `fresh` (today's cold dispatch: open a new session with the feedback as the prompt via the existing `pr-respond` workflow). A route missing from this list is never attempted. |
| `live_enabled` | `boolean` | `false` | Whether the `live` route may actually be attempted. Defaults off: it is optional in the MVP and unverified end-to-end against a real hosted session outside this repo's own dev loop (Orca's own send argv is itself an inferred, unconfirmed shape — see `hosts/orca.ts`'s `orcaSendArgv` doc). `false` skips straight to `resume` even when `routes` lists `live` first. |
| `max_deliveries_per_pr` | `integer >= 1` | `20` | Cap on entries kept in one subject's DeliveryRecord `attempts` log (oldest dropped past this). Does not cap how many distinct feedback items can be delivered, and does not cap the `delivered` signature map, which is pruned by PR state and head SHA instead — only how much attempt history is retained. |

### pr_check

Configures how `/sdlc:pr-check` (the `pr classify` op) and the `orchestrate watch` pending-work gate treat review comments and submitted reviews: which authors are ignored as automation noise (`ignored_authors`), and how the PR author's own comments are treated (`author_comments`). The defaults favour solo-developer workflows where the PR author and the reviewer are the same human; teams should set `author_comments: self-notes` to filter the author's own comments out of the actionable-feedback signal. Missing block, missing key, or an unrecognised value all default to `actionable` (and an empty `ignored_authors`).

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, loop, session |
| Consumers | `lib/services/pr/ops/classify.ts#author_comments`<br>`lib/services/orchestrator/ticks/prs.ts#ignored_authors`<br>`lib/services/orchestrator/ops/watch.ts#ignored_authors`<br>`lib/services/dashboard/server.ts#ignored_authors` |

Default:

```yaml
pr_check:
  author_comments: actionable
  ignored_authors: []
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `author_comments` | `'actionable' \| 'self-notes'` | `"actionable"` | `actionable` (default, solo) — author's review comments and submitted reviews count as actionable feedback and can flip the verdict to NEEDS-RESPONSE. `self-notes` (team) — author's comments on their own PR are filtered out as notes-to-self. Automation authors (see `ignored_authors`) are always excluded regardless of this setting. |
| `ignored_authors` | `list<string>` | `[]` | Project-specific automation logins whose comments and reviews never count as actionable feedback (and never wake the orchestrator). The built-in set `github-actions` and `cloudflare-workers-and-pages` is always applied; this list ADDS to it (e.g. `vercel`, `netlify`, `dependabot`) so a new project's CI/deploy bots need config, not a plugin release. Matching is shape-normalized: a trailing `[bot]` suffix is stripped before comparison, so an entry written as `dependabot` or `dependabot[bot]` matches both the suffix-less GraphQL login (`gh pr view`) and the suffixed REST login (`gh api`). Missing key or a non-array value means 'no extra authors' — the built-in set still applies. |

### pr_update

Configures `sdlc pr survey` and `sdlc pr update`: which PRs are in scope (`author`, `include_drafts`), how an out-of-date branch is brought up to date (`strategy`, `overrides`), the cases that automatically avoid a history rewrite (`guards`), and how conflicts are resolved without a human (`lockfile_install`, `resolvers`, `verify`). Every key has a default and the whole block is optional, so both verbs work with no `sdlc.yaml` at all.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, loop |
| Consumers | `lib/services/pr/ops/survey.ts#pr_update`<br>`lib/services/pr/ops/update.ts#pr_update` |

Default:

```yaml
pr_update:
  author: null
  include_drafts: true
  strategy: rebase
  overrides: []
  guards:
    unpushed_local: skip
    reviewed: merge
    max_rebase_commits: 20
  lockfile_install: null
  resolvers: []
  verify: none
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `author` | `string \| null` | `null` | Only survey/update PRs by this author. `@me` is gh's own token for the authenticated user. Null (the default) means every author — appropriate for a solo repository, where filtering by author would hide nothing. |
| `include_drafts` | `boolean` | `true` | Whether draft PRs are surveyed and updated. Drafts drift like any other branch and a draft left behind for months is exactly the case this exists to surface, so they are included by default. |
| `strategy` | `'rebase' \| 'merge'` | `"rebase"` | How an out-of-date branch is brought up to date. `rebase` (default) keeps each PR's history linear and its diff exactly its own commits, which matters most in a stack — a merge leaves every descendant carrying a merge commit per ancestor update. `merge` never rewrites history and so never needs a force-push. See `guards` for the cases that fall back to merge automatically. |
| `overrides` | `list<object>` | `[]` | Per-PR strategy overrides, first match wins. A rule matches when every field it declares matches. Use it to pin a protected base branch to `merge`, or to let a label opt one PR out. An override beats the `reviewed` and `max_rebase_commits` guards, which are preferences — it does NOT beat `unpushed_local`, which exists to stop a force-push destroying work and is not negotiable. |
| `guards` | `object` | `{"unpushed_local":"skip","reviewed":"merge","max_rebase_commits":20}` | Automatic falls-back to `merge` (or `skip`) for the cases where rewriting history costs more than the tidier tree is worth. `unpushed_local` is checked first and cannot be overridden; the other two are preferences an `overrides` rule may beat. Guards only ever step down from `rebase` — none of them turns a `merge` into a rebase. |
| `lockfile_install` | `string \| null` | `null` | Shell verb that rebuilds lockfiles from their manifests (`bun install`, `cargo generate-lockfile`, …), run in the update worktree. A lockfile conflict is resolved by taking the base side and re-running this, because a lockfile is derived and the merged manifest is what it must be derived from. Null (the default) means lockfile conflicts are left for a human: taking a side without re-deriving would push a lockfile that disagrees with its own manifest, which is worse than not updating. |
| `resolvers` | `list<object>` | `[]` | Project-specific conflict resolvers for generated artifacts, first match wins. A conflict in a file a generator owns is not a judgement call — it is `re-run the generator`, and the run's output is the resolution. Lockfiles are handled without configuration; this is for a project's own codegen (`docs/index.md` rebuilt by `sdlc docs generate`, say). Every conflicted path in a PR must be claimed by some rung or the update is left for a human. |
| `verify` | `'none' \| 'check'` | `"none"` | What must pass before an updated branch is pushed. `none` (default) because the deterministic tier only ever produces a merge git itself resolved or an artifact a generator rebuilt, and running the project's whole gate once per PR is not affordable across a 50-PR run — CI on the PR is the real gate. `check` runs the `workflows.check` list anyway, for a project where a bad push is expensive — trust-gated like every other project-sourced workflow command list ([[D-V2XJ-verb-cascade-and-repo-trust]]): needs `sdlc repo trust` or `sdlc pr update --trust`. |

##### `pr_update.guards`

Automatic falls-back to `merge` (or `skip`) for the cases where rewriting history costs more than the tidier tree is worth. `unpushed_local` is checked first and cannot be overridden; the other two are preferences an `overrides` rule may beat. Guards only ever step down from `rebase` — none of them turns a `merge` into a rebase.

| Field | Shape | Default | Description |
|---|---|---|---|
| `unpushed_local` | `'skip' \| 'merge'` | `"skip"` | What to do when the head branch is checked out in another worktree that holds commits the remote does not. `--force-with-lease` does NOT protect against this — it compares against the remote, which is unchanged — so a rebase here silently orphans work in progress. `skip` (default) leaves the PR alone; `merge` updates it without rewriting history. |
| `reviewed` | `'merge' \| 'rebase'` | `"merge"` | What to do when a PR carries at least one submitted review. Rebasing rewrites the commits review comments are anchored to, so GitHub marks those threads outdated. `merge` (default) keeps them anchored. |
| `max_rebase_commits` | `integer >= 0` | `20` | Above this many commits ahead of base, fall back to `merge`. A rebase replays every commit and can hit the same conflict once per commit, while a merge resolves it once; past some length that trade stops paying. 0 disables the guard. |

### pr_review

Settings for `sdlc pr review`, which opens a pull request in a local worktree and starts an interactive review session there. Missing block defaults to `claude` in the current terminal with the built-in brief.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli |
| Consumers | `lib/services/pr/ops/review.ts#pr_review` |

Default:

```yaml
pr_review:
  engine: claude
  untrusted: skip-init
  setup: true
  prompt:
    mode: extend
    text: ''
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `engine` | `'claude' \| 'codex' \| 'cursor' \| 'pi'` | `"claude"` | Interactive coding agent `sdlc pr review` starts in the PR worktree. The named CLI must be on PATH. `--engine` overrides per run. |
| `host` | `'terminal' \| 'orca' \| 'cmux' \| 'tmux'` | — | Where the review session opens, when it should differ from `host.default`: terminal \| orca \| cmux \| tmux. A host whose binary is missing fails the run rather than falling back. |
| `untrusted` | `'skip-init' \| 'full' \| 'refuse'` | `"skip-init"` | What `sdlc pr review` does with a PR whose head branch lives in another repository (a fork), whose tree is a stranger's code. `skip-init` (default): create the worktree but run no `setup` verbs there — an install would execute that PR's own lifecycle scripts before anyone has read it — and warn that the tree's instruction files and hooks are the PR author's. `full`: treat it like a same-repo PR. `refuse`: exit with an error. `--trust` runs one PR as `full` whatever this says. Same-repo PRs are never affected. |
| `setup` | `boolean` | `true` | Whether `sdlc pr review` runs the `setup` verb in the PR worktree on every launch — created or reused — before the session starts. Idempotency is the verb's own contract, so running it again on a reused worktree is cheap by design. `--no-setup` skips it for one run. |
| `prompt` | `object` | `{"mode":"extend","text":""}` | How the review brief handed to the session is composed. See `mode` and `text`. |

##### `pr_review.prompt`

How the review brief handed to the session is composed. See `mode` and `text`.

| Field | Shape | Default | Description |
|---|---|---|---|
| `mode` | `'extend' \| 'replace'` | `"extend"` | `extend` appends `text` after the built-in review brief; `replace` uses `text` as the whole prompt. In either mode the placeholders `{pr_number}`, `{pr_url}`, `{pr_title}`, `{pr_author}`, `{base}`, `{head}`, `{repo}`, `{worktree}` and `{default_prompt}` resolve (unknown ones render empty), so a replacement can embed the built-in brief wherever it likes. |
| `text` | `string` | `""` | Project-specific review instructions. Empty (the default) leaves the built-in brief unchanged in `extend` mode; in `replace` mode an empty text is an error. |

### notify

Notification channels and the event catalog that routes each event to zero or more of them. `notify(event, payload, ctx)` (`lib/services/notify/dispatch.ts`) resolves `events[event]` to channel names, looks each one up in `channels`, and dispatches best-effort per channel — one channel failing never suppresses another or fails the caller. `sdlc notify send|test` fires a named channel directly, bypassing event routing. This cross-field check assumes a fully-merged config; validating `sdlc.yaml` or `sdlc.local.yaml` alone (where the split may legitimately span both files) uses the un-refined `NotifyObjectSchema` instead — see `lib/config/schema.ts`.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | unset |
| Applies via | `read` |
| Layers | project, local |
| Runs in | loop, cli, hook |
| Consumers | `lib/services/notify/dispatch.ts#notify`<br>`lib/services/notify/ops/send.ts#runNotifySend`<br>`lib/services/notify/ops/test.ts#runNotifyTest` |

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `channels` | `map<string, object>` | `{}` | Named notification channels, each an object discriminated by `type`: `{type: "desktop"}` (no other fields — raises a notification on the machine running the loop); `{type: "ntfy", topic_url}` (POSTs to an ntfy HTTP topic URL); or `{type: "webhook", url, format?}` (POSTs a JSON body to an arbitrary HTTP endpoint — `format` is `slack`, `discord`, or `raw`, default `raw`). Each name is referenced by one or more `events.*` lists below; a channel not referenced by any event is valid but inert. |
| `events` | `object` | `{"pr.needs_response":[],"pr.ci_failed":[],"task.parked":[],"stage.ceiling_reached":[],"phase.ready_for_review":[],"lease.expired":[],"session.ended":[],"hook.blocked":[],"orchestrator.dead_stop":[],"usage.budget_exceeded":[]}` | Exactly the 10 canonical event names, each an ordered list of channel names (from `channels` above) to fire when that event happens. An event key that is absent, or set to `[]`, is a silent no-op — no channel call, no error. An unknown event key fails validation. |

##### `notify.events`

Exactly the 10 canonical event names, each an ordered list of channel names (from `channels` above) to fire when that event happens. An event key that is absent, or set to `[]`, is a silent no-op — no channel call, no error. An unknown event key fails validation.

| Field | Shape | Default | Description |
|---|---|---|---|
| `pr.needs_response` | `list<string>` | `[]` | A PR classified NEEDS-RESPONSE has exhausted `orchestrator.review.max_rounds` review/respond round-trips (`ticks/_pr_action.ts`). Fires on every orchestrator tick thereafter for as long as the PR stays stuck in that state — there is no dedup or one-shot suppression, so a configured channel repeats this notification, not just once. |
| `pr.ci_failed` | `list<string>` | `[]` | A PR classified CI-FAILED has exhausted `orchestrator.review.max_rounds` review/respond round-trips (`ticks/_pr_action.ts`). Fires on every orchestrator tick thereafter for as long as the PR stays stuck in that state — there is no dedup or one-shot suppression, so a configured channel repeats this notification, not just once. |
| `task.parked` | `list<string>` | `[]` | A task's dispatch is stood down, whether by a blocking hook (`sdlc hooks run` consumes a park marker for the task's branch) or by the `work` tick's own step-failure handling when a dispatched workflow step fails outright (`ticks/work.ts`). |
| `stage.ceiling_reached` | `list<string>` | `[]` | A chain-labeled issue is already at its `sdlc:auto/<x>` autonomy ceiling (the `issues` tick). |
| `phase.ready_for_review` | `list<string>` | `[]` | A phase reached ready-for-review. No detector wired yet. |
| `lease.expired` | `list<string>` | `[]` | A lease expired (`sdlc lease reconcile`'s `expired-task-leases` detector). `notifyExpiredLeaseAnomalies` (`reconcile.ts`) collapses every anomaly found in ONE reconcile run into a single notify, but that dedup does not persist across runs: the same still-expired lease fires again on every subsequent `sdlc lease reconcile` run for as long as it stays expired — there is no cross-run dedup or one-shot suppression, the same unbounded-repeat shape as `pr.needs_response`/`pr.ci_failed` above. |
| `session.ended` | `list<string>` | `[]` | An agent session ended: fires whenever a headless session record is finalized. The orchestrate step-runner launching a headless skill/prompt step for a workflow tick is the common case in practice; a direct `sdlc session launch --headless` invocation is the other. |
| `hook.blocked` | `list<string>` | `[]` | An `on_fail: block` hook step failed (`sdlc hooks run`). |
| `orchestrator.dead_stop` | `list<string>` | `[]` | The orchestrate loop stopped dead. No detector wired yet. |
| `usage.budget_exceeded` | `list<string>` | `[]` | A usage.budget ceiling (tokens or cost_usd) was exceeded within its rolling window AND the work tick actually had a ready candidate it would otherwise have dispatched this tick (ticks/work.ts) — never fires while already `orchestrate pause`d (that gate is the more informative status) or when there was nothing ready to dispatch anyway. Still repeats every tick that meets both conditions for as long as usage stays over the ceiling — no one-shot-per-crossing suppression, but scoped to ticks where the event's claim ("dispatch was suppressed") is actually true. |

### usage

Token/cost usage reporting and the budget gate. `budget` sets a rolling-window token/cost ceiling that pauses the work tick's dispatch and fires `usage.budget_exceeded` when exceeded. Absent, or present with `budget` absent (or both `budget.tokens`/`budget.cost_usd` unset), means no cap — inert, matching `notify.channels`' 'valid but inert' precedent.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | unset |
| Applies via | `read` |
| Layers | project, local |
| Runs in | loop |
| Consumers | `lib/services/usage/budget.ts#checkUsageBudget`<br>`lib/services/orchestrator/ticks/work.ts#runWorkTick` |

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `budget` | `object` | — | Token/cost ceiling that pauses the orchestrator work tick's dispatch and fires usage.budget_exceeded when exceeded. Absent, or present with both `tokens`/`cost_usd` unset, means no cap — inert, matching notify.channels' 'valid but inert' precedent. |

##### `usage.budget`

Token/cost ceiling that pauses the orchestrator work tick's dispatch and fires usage.budget_exceeded when exceeded. Absent, or present with both `tokens`/`cost_usd` unset, means no cap — inert, matching notify.channels' 'valid but inert' precedent.

| Field | Shape | Default | Description |
|---|---|---|---|
| `window_hours` | `number > 0` | `24` | Rolling lookback window (hours, ending now) that budget totals are summed over. |
| `tokens` | `integer > 0` | — | Total token ceiling, compared against a BILLABLE per-window sum: UsageRow.inputTokens + outputTokens + cacheWriteTokens, nulls counting as 0 — deliberately EXCLUDING cacheReadTokens (see lib/services/usage/record.ts#sumBillableTokens). Cache reads dominate raw usage and are far cheaper than fresh input, so a ceiling compared against the cache-inclusive ledger total (UsageRow.totalTokens) would trip within a single session. Unset: no token ceiling. |
| `cost_usd` | `number > 0` | — | Total USD cost ceiling (sum of UsageRow.costUsd, nulls counting as 0) across rows inside the window. Only a headless Claude run ever populates a non-null costUsd (claude -p --output-format json's own total_cost_usd) — every interactive row and every codex row (headless or interactive) is always costUsd: null, since codex reports no dollar figure and there is no local price table. This ceiling therefore only ever caps headless-Claude spend, not total spend. Unset: no cost ceiling. |

## Integration

### forge

Which code-hosting forge `pr classify` and the orchestrator `prs` tick talk to, via `selectForge`. `pr survey`/`pr update` (the standalone `@sksizer/pr-update` package) and the dashboard read GitHub directly and do not consult this key yet. Every key has a default, so this block is optional.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, loop |
| Consumers | `lib/services/pr/forge/select.ts#selectForge` |

Default:

```yaml
forge:
  override: null
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `override` | `'github' \| 'forgejo' \| null` | `null` | Force which Forge adapter `selectForge`'s callers use, bypassing auto-detection from the `origin` remote's URL. Null (the default) auto-detects: a `github.com` remote uses the GitHub adapter, anything else uses Forgejo. |

### host

The default host — see `host.default`.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, loop |
| Consumers | `lib/services/host/index.ts#pickHost`<br>`lib/services/session/ops/launch.ts#pickHost`<br>`lib/services/pr/ops/review.ts#pickHost` |

Default:

```yaml
host:
  default: terminal
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `default` | `'terminal' \| 'orca' \| 'cmux' \| 'tmux'` | `"terminal"` | Where sdlc puts work in front of you when a feature does not say otherwise: `terminal` runs it in the current terminal and returns when it ends; `orca` opens a terminal tab in the Orca app; `cmux` opens a cmux workspace; `tmux` opens a detached tmux session named deterministically from the working directory. Every host runs whichever agent CLI (`engine`) the feature resolves, in a terminal/workspace it opens — the two are independent, not a fixed pairing. Every host fulfils the host contract in parts (an agent session, a command in a terminal, a message sent into an already-open one); a feature that needs a part the host lacks fails rather than falling back. A feature's own `host` key (e.g. `pr_review.host`) overrides this, and a `--host` flag overrides both. |

### harness

Which agent harnesses `sdlc apply` installs sdlc into. A change here takes effect only after `sdlc apply` re-installs.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | unset |
| Applies via | `apply:harness` |
| Layers | project, local |
| Runs in | cli |
| Consumers | `planned:the harness applier` |

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `targets` | `list<string>` | — | Agent harnesses to install the sdlc plugin surface for, by exporter name (`sdlc harness export --target` lists them). Absent means the harness applier picks its default. |

### mcp

Which ops the `sdlc mcp` stdio server exposes as tools. Safe default: only ops marked `mutating: false` are exposed; `allow` opts specific mutating ops in, `deny` excludes specific ops. `sdlc apply` registers the server with each harness, so a change to the registration takes effect only after it runs.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | unset |
| Applies via | `apply:mcp` |
| Layers | project, local |
| Runs in | session, cli |
| Consumers | `lib/services/apply/mcp.ts#mcpApplier`<br>`lib/services/mcp/server.ts#buildMcpServer` |

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `allow` | `list<string>` | `[]` | Additional op paths (space-joined, e.g. "task create") to expose as MCP tools even though they are not marked mutating: false. Opt-in for mutating ops. |
| `deny` | `list<string>` | `[]` | Op paths (space-joined) to exclude from the MCP tool set even if they would otherwise qualify (e.g. a read-only op you don't want exposed). |

### issues

Reserved for issue-tracker sync and the issues loop (sdlc 1.0 Phase 7). No field is defined yet.

| Meta | Value |
|---|---|
| Shape | `object (no fields)` |
| Default | unset |
| Applies via | `read` |
| Layers | project, local |
| Runs in | loop, cli |
| Consumers | `planned:sdlc issues sync and the issues loop (sdlc 1.0 Phase 7)` |

## Planning

### task

Task-domain configuration. Groups task-lifecycle execution policy under `execution` (see `execution.spawn_from_post_mortem`).

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, session |
| Consumers | `lib/services/config/ops/get-spawn-policy.ts#spawn_from_post_mortem` |

Default:

```yaml
task:
  execution:
    spawn_from_post_mortem:
      enabled: true
      drive_to_ready: true
      fallback_status: planning/draft
      target:
        pr_grouping: per-task
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `execution` | `object` | `{"spawn_from_post_mortem":{"enabled":true,"drive_to_ready":true,"fallback_status":"planning/draft","target":{"pr_grouping":"per-task"}}}` | Configures how this project executes task-lifecycle automation. Today the only sub-block is `spawn_from_post_mortem`. |

##### `task.execution`

Configures how this project executes task-lifecycle automation. Today the only sub-block is `spawn_from_post_mortem`.

| Field | Shape | Default | Description |
|---|---|---|---|
| `spawn_from_post_mortem` | `object` | `{"enabled":true,"drive_to_ready":true,"fallback_status":"planning/draft","target":{"pr_grouping":"per-task"}}` | Policy for converting a `/sdlc:task-work` post-mortem's friction bullets into follow-up tasks. Missing block defaults to enabled, drive-to-ready, landing undriveable follow-ups at `planning/draft`. |

###### `task.execution.spawn_from_post_mortem`

Policy for converting a `/sdlc:task-work` post-mortem's friction bullets into follow-up tasks. Missing block defaults to enabled, drive-to-ready, landing undriveable follow-ups at `planning/draft`.

| Field | Shape | Default | Description |
|---|---|---|---|
| `enabled` | `boolean` | `true` | Whether `/sdlc:task-work` Step 8 spawns follow-up tasks from the post-mortem's 'Friction and automation gaps' bullets (the `/sdlc:spawn-from-post-mortem` → `/sdlc:spawn-task-pr` flow). `true` (default) runs the spawn sub-step; `false` skips it entirely — the post-mortem is still written and committed, but no follow-up task or PR is created. A missing key defaults to enabled. |
| `drive_to_ready` | `boolean` | `true` | Whether each spawned follow-up is driven toward `open/ready` before its PR is opened. `true` (default) runs `/sdlc:task-auto-define` then `/sdlc:task-ensure-ready` on the scaffolded task: when the spec can be synthesized from the friction bullet plus codebase context AND passes the readiness gate, ensure-ready promotes the task to `open/ready` with `readiness_verified_at:` stamped. When auto-define reports INSUFFICIENT the task lands at `fallback_status:`; when the gate finds a gap it lands at `planning/needs-definition` — the drive is best-effort, never fabricated. `false` skips the drive and always lands the task at `fallback_status:`. |
| `fallback_status` | `'planning/draft' \| 'planning/needs-definition' \| 'planning/proposed' \| 'planning/backlog'` | `"planning/draft"` | The status a spawned follow-up lands at when it is NOT driven to `open/ready` — either because `drive_to_ready:` is false, or because the best-effort drive's auto-define could not synthesize the spec (a spec that fails the readiness gate lands at `planning/needs-definition` instead). Must be a non-ready `planning/*` status; default `planning/draft` (today's behavior). Set `planning/needs-definition` to route undriveable follow-ups straight into the definition backlog — note that with `drive_to_ready: true` this value also skips the drive entirely (`planning/needs-definition` is not a status `/sdlc:task-ensure-ready` accepts as input), landing the follow-up there directly instead of attempting the gate. |
| `target` | `object` | `{"pr_grouping":"per-task"}` | Controls which PR a spawned follow-up lands in and how that PR is titled. See `pr_grouping` and `pr_title_pattern`. |

###### `task.execution.spawn_from_post_mortem.target`

Controls which PR a spawned follow-up lands in and how that PR is titled. See `pr_grouping` and `pr_title_pattern`.

| Field | Shape | Default | Description |
|---|---|---|---|
| `pr_grouping` | `'per-task' \| 'per-execution' \| 'per-project'` | `"per-task"` | How spawned follow-up tasks are packaged into PRs (per target repo — classification routes Local / Upstream-plugin / Cross-project-request follow-ups to different repos, and a PR cannot span repos). `per-task` (default, today's behavior): one PR per spawned task. `per-execution`: one PR per `/sdlc:task-work` run bundling all that run's follow-ups for a repo. `per-project`: a single rolling PR per repo that accumulates follow-ups across runs (append to the open PR, or start a fresh one — the `sdlc backlog create` rolling pattern). |
| `pr_title_pattern` | `string` | — | Optional template for the spawned PR's title. Placeholders resolved at PR-creation time (unknown ones render empty): `{headline}` (spawned task headline), `{slug}`, `{originating_task}`, `{classification}`, `{date}` (UTC), `{repo}` (owner/name). Absent → each grouping mode uses its built-in default: `per-task` = the task headline (today's behavior); `per-execution` = `chore(tasks): follow-ups from {originating_task}`; `per-project` = `chore(tasks): spawned follow-up tasks`. Note `{originating_task}` is meaningful only for `per-task`/`per-execution` — a `per-project` rolling PR spans many runs, so a per-run token there resolves to the run that first opened it. |

### backlog

Backlog-domain configuration. Groups backlog-capture submission policy under `capture` (see `capture.target`).

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, session |
| Consumers | `cli/backlog_cli/create.ts#capture`<br>`lib/services/config/ops/get-backlog-policy.ts#capture` |

Default:

```yaml
backlog:
  capture:
    enabled: true
    target:
      pr_grouping: per-project
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `capture` | `object` | `{"enabled":true,"target":{"pr_grouping":"per-project"}}` | Policy for `sdlc backlog create` — the deterministic tail behind `/sdlc:backlog-capture`. Missing block defaults to enabled, per-project (rolling) grouping. |

##### `backlog.capture`

Policy for `sdlc backlog create` — the deterministic tail behind `/sdlc:backlog-capture`. Missing block defaults to enabled, per-project (rolling) grouping.

| Field | Shape | Default | Description |
|---|---|---|---|
| `enabled` | `boolean` | `true` | Whether `sdlc backlog create` (the `/sdlc:backlog-capture` tail) auto-submits the captured item to a PR. `true` (default) authors the file, commits it in an ephemeral worktree, pushes, and opens-or-updates a PR — today's behavior. `false` authors the file in the working tree and stops (no commit, no push, no PR) — the item is captured locally for the author to submit by hand. |
| `target` | `object` | `{"pr_grouping":"per-project"}` | Controls which PR a captured backlog item lands in and how that PR is titled. See `pr_grouping` and `pr_title_pattern`. |

###### `backlog.capture.target`

Controls which PR a captured backlog item lands in and how that PR is titled. See `pr_grouping` and `pr_title_pattern`.

| Field | Shape | Default | Description |
|---|---|---|---|
| `pr_grouping` | `'per-project' \| 'per-item'` | `"per-project"` | How captured backlog items are packaged into PRs. `per-project` (default, today's behavior): a single rolling `backlog-capture` PR that accumulates every captured item until triaged. `per-item`: one branch/PR per captured item (`backlog/<slug>`), so each idea is reviewed on its own. (No `per-execution` — backlog capture has no run to bundle.) |
| `pr_title_pattern` | `string` | — | Optional template for the backlog PR's title, resolved when the PR is created (placeholders, unknown → empty): `{id}` (B-NNNN), `{slug}`, `{title}` (headline), `{date}` (UTC). Absent → each grouping mode's default: `per-project` = `docs(backlog): rolling capture`; `per-item` = `docs(backlog): {title}`. For a `per-project` rolling PR the title is fixed by the item that first opened it. |

### knowledge

Reserved for knowledge-base settings. No field is defined yet.

| Meta | Value |
|---|---|
| Shape | `object (no fields)` |
| Default | unset |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, session |
| Consumers | `planned:no reader yet` |

## Infrastructure

### lease_authority

Default ref authority used by the `sdlc lease *` ops when invoked without an `--authority` override. The value is whatever `git fetch` / `git push` accepts: a remote name (`origin`), an SSH URL (`ssh://…`), an HTTPS URL (`https://…`), a `file://` URL, or a filesystem path (absolute or project-root-relative). When unset, the CLI exits 1 with a setup-pointing error — the lease library accepts the authority as an explicit argument, so callers must configure one before any `sdlc lease` command can talk to the ref source. This is a minimal slice-1 surface; the richer `control_plane.ref_authority` block from the ADR (with `type`, `address`, and `namespace`) is deferred to slice 2/3 when control-plane wiring lands.

| Meta | Value |
|---|---|
| Shape | `string` |
| Default | unset |
| Applies via | `read` |
| Layers | project |
| Runs in | cli, session |
| Consumers | `lib/config/load.ts#lowReadLeaseAuthority`<br>`lib/config/sdlc_yaml.ts#resolveAuthority`<br>`lib/services/lease/runtime.ts#lowReadLeaseAuthority`<br>`lib/services/lease/ops/_common.ts#lease_authority` |

### docs_site

Project-root-relative directory of the `sdlc`-generated docs site — the Astro/Starlight app whose content `sdlc docs generate site` produces. Defaults to `site`. Set this to relocate the site, e.g. `sites/docs` when it lives under a monorepo `sites/` tree. Every site path (content root `<dir>/src/content/docs`, the `<dir>/site.yaml` manifest, `<dir>/supplemental/`, the generated sidebar `<dir>/src/generated/sidebar.mjs`, and the `<dir>/scripts/list-ops.ts` helper) is resolved beneath this directory, so consumers can host the site wherever they like.

| Meta | Value |
|---|---|
| Shape | `string` |
| Default | `"site"` |
| Applies via | `read` |
| Layers | project, local |
| Runs in | cli, hook |
| Consumers | `lib/config/load.ts#docsSiteDir`<br>`lib/services/docs/site.ts#docsSiteDir`<br>`lib/services/docs/site/manifest.ts#docsSiteDir`<br>`lib/services/docs/site/supplemental.ts#docsSiteDir`<br>`lib/services/docs/site/subsites.ts#docsSiteDir` |

### paths

Named directories other path settings refer to as `{name}`: `repos:` entries and `checkout.layout`. `dev: ~/Developer` lets `repos: ['{dev}']` and `layout: '{dev}/{owner}/{repo}'` say it once. `~` expands and a relative path resolves against the project root. The machine config's `paths:` carries the same map and wins per name, since a directory is a fact about a machine. `host`, `owner` and `repo` are the layout's own tokens and cannot be defined here. Entries do not refer to each other.

| Meta | Value |
|---|---|
| Shape | `map<string /^[A-Za-z][A-Za-z0-9_]*$/, string>` |
| Default | `{}` |
| Applies via | `read` |
| Layers | project, local, machine |
| Runs in | cli |
| Consumers | `lib/services/repo/locate.ts#pathTokens`<br>`lib/services/repo/layout.ts#pathTokens` |

### repos

Where this machine's repositories are. An entry that is itself a checkout names that repository; one that is not is searched for the checkouts under it, so `~/Developer` covers `~/Developer/dev` and `~/Developer/acme/api` alike — the search descends through directories that are not checkouts and stops at the ones that are. `sdlc pr review <url>` scans them for the checkout whose `origin` matches the URL's owner/repo, `sdlc pr survey --repo owner/name` for the checkout its drift is measured in, and `sdlc repo checkout` for a clone that is already on disk. Listing a directory here never puts what is under it into a verb's scope on its own: a verb that can fan out (`sdlc project cleanup --all`) does so only when asked, and every other verb stays in the repository you are standing in. An entry may use `{paths}` tokens; `~` expands and a relative path resolves against the project root, as for every other path value. The machine-level config (`<config-home>/sdlc/config.yaml`, key `repos`) carries the same list for paths that are specific to one machine and do not belong in a committed file; its entries are searched first.

| Meta | Value |
|---|---|
| Shape | `list<string>` |
| Default | `[]` |
| Applies via | `read` |
| Layers | project, local, machine |
| Runs in | cli |
| Consumers | `lib/services/repo/locate.ts#repos` |

### checkout

Settings for `sdlc repo checkout`, which clones a repository URL into the directory `layout` names. Missing block: `{dev}/{repo}` over https.

| Meta | Value |
|---|---|
| Shape | `object` |
| Default | see below |
| Applies via | `read` |
| Layers | project, local, machine |
| Runs in | cli |
| Consumers | `lib/services/repo/layout.ts#checkout` |

Default:

```yaml
checkout:
  layout: '{dev}/{repo}'
  protocol: https
```

#### Fields

| Field | Shape | Default | Description |
|---|---|---|---|
| `layout` | `string` | `"{dev}/{repo}"` | Where `sdlc repo checkout <url>` puts a checkout, as a path template. Tokens: `{host}` (`github.com`), `{owner}`, `{repo}`, and every `paths:` entry by name. The default `{dev}/{repo}` needs `paths.dev` and keeps checkouts directly under it; `{dev}/{owner}/{repo}` groups them by owner. Either works with `repos: ['{dev}']`, since the scan descends through directories that are not checkouts. A token nothing defines is an error, not an empty string. `~` expands and a relative result resolves against the project root. The machine config's `checkout.layout` overrides this one. |
| `protocol` | `'https' \| 'ssh'` | `"https"` | Clone URL to build when the input names a repository without saying how to reach it — a bare `owner/repo`, `github:owner/repo`. An explicit `https://…` or `git@…:` input is cloned the way it was given. The machine config's `checkout.protocol` overrides this one. |
