---
type: task
schema_version: '2'
status: planning/draft
created: '2026-05-20'
impact: medium
complexity: small
tags:
- rust
- sdlc
- task-work
- tooling
related:
- 2026-05-19-split-clients-from-servers
---
# task-work setup step should detect project stack instead of hard-coding just setup-worktree

_Auto-generated from a /sdlc:task-work post-mortem. Review and
promote to `open/ready` before picking up._

## Goal

`/sdlc:task-work` Step 4 hard-codes `mise trust && just setup-worktree` as the
worktree-bootstrap invocation, which assumes a JS/Node stack with a
`setup-worktree` recipe. Rust-only projects (no `package.json`, no
`setup-worktree` justfile recipe) error on this call, even though they don't
need any bootstrap because Cargo handles worktrees natively. The step should
detect the project's stack and skip or substitute the call accordingly.

## Today

From the post-mortem of [[2026-05-19-split-clients-from-servers]]:

> `/sdlc:task-work` Step 4's setup-worktree call (`mise trust && just
> setup-worktree`) is hard-coded for JS-stack projects. This is a Rust-only
> project — `just setup-worktree` doesn't exist; the call errored. Setup
> completed without it because Cargo handles Rust worktrees natively.

The skill silently recovered (Cargo doesn't need bootstrapping), but a user
seeing the error mid-run has no way to know whether it's fatal or cosmetic.

## Proposed

Step 4 detects project stack at runtime — at minimum, presence of `package.json`
at the project root vs presence of `Cargo.toml` only — and conditionalizes the
bootstrap invocation:

- If `package.json` exists: run `mise trust && just setup-worktree` as today.
- If only `Cargo.toml` exists (no `package.json`): skip the bootstrap call and
  log a one-line "Rust-only project; no JS bootstrap needed" notice.
- If a project-local override exists (e.g. a `.claude/setup-worktree-cmd`
  config file or a `just bootstrap-worktree` recipe), prefer that. Defer if
  scope creeps.

## Approach

1. Read `plugin/skills/sdlc/task-work/SKILL.md` Step 4 and find the
   bootstrap-invocation block.
2. Add a stack-detection branch: check for `package.json` at the worktree root;
   if absent and `Cargo.toml` is present, skip the JS bootstrap call.
3. Document the detection rule inline so future maintainers know why the branch
   exists.

## Files to touch

- `plugin/skills/sdlc/task-work/SKILL.md` — add stack-detection logic to Step 4
  worktree-bootstrap invocation.

## Acceptance criteria

- [ ] AC-1: Running `/sdlc:task-work` against a Rust-only project (Cargo.toml
  at root, no package.json) completes Step 4 without invoking
  `just setup-worktree` and without producing an error.
- [ ] AC-2: Running `/sdlc:task-work` against a JS/Node project (package.json
  at root) still invokes `mise trust && just setup-worktree` as today.
- [ ] AC-3: The SKILL.md change documents the stack-detection rule inline.

## Out of scope

- Generalizing to every possible stack (Python, Go, etc.). Two-stack detection
  (JS vs Rust-only) covers the cases observed today; add others when motivated.
- Adding a project-local override mechanism (config file or custom recipe).
  Mentioned in Proposed as a deferred enhancement.

## Dependencies

- none

## Discovery context

Spawned by /sdlc:task-work post-mortem of [[2026-05-19-split-clients-from-servers]] on 2026-05-20.
