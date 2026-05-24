---
type: task
schema_version: '3'
status: closed/wontdo
created: '2026-05-20'
impact: medium
complexity: medium
tags:
- dependencies
- maintenance
- rust
- node
- pnpm
related: []
last_reviewed: '2026-05-24'
completion_note: |
  Dropped during /sdlc:orchestrate tick #1 cleanup. The task spec
  shipped with v2 migration placeholders in the Today table and
  malformed Location rows that didn't survive the v2-to-v3 migration
  cleanly; /sdlc:task-ensure-ready flagged it as needs-definition.
  Rather than re-spec the placeholder rows for a routine-hygiene
  task, dropping outright. Re-file fresh when a dep bump becomes
  time-critical (security advisory, breaking upstream, etc.).
---
# Bump Rust and JS/pnpm dependencies across all workspaces

## Goal

Routine dependency hygiene: bring every Rust crate and every JS/pnpm package
in the repo to current, compatible versions. Keep us close to the leading
edge so security advisories land in upgrades we already need, not crash
patches, and so dependency-driven breakage surfaces in small chunks rather
than one annual avalanche.

This task is also a deliberate end-to-end exercise of `/sdlc:orchestrate`
against work that touches multiple stacks and lockfiles.

## Today

| Location | Role today |
|---|---|
| `Root: `Cargo.toml` (workspace members: `.`, `crates/ontogen-core`,` | <migrated from v2 — no role recorded> |
| `examples/iron-log/src-tauri/Cargo.toml` | Tauri 2 / sea-orm 1 / axum 0.8 |
| `**JS/pnpm projects**` | <migrated from v2 — no role recorded> |
| `site/package.json` | Astro 6 + Starlight 0.38 docs site. |
| `packages/admin-types/package.json` | framework-agnostic types, no deps. |
| `packages/nuxt_admin_layer/package.json` | Nuxt 4 admin layer |
| `examples/iron-log/package.json` | iron-log root (lefthook, commitlint, |
| `examples/iron-log/src-nuxt/package.json` | Nuxt 4 frontend, Vue 3.5, |


## Proposed

Each of the seven projects (2 Cargo + 5 pnpm) has been bumped to the
highest set of versions that still:

1. Type-checks / compiles cleanly.
2. Passes its own test suite (`cargo test`, `pnpm test`, `nuxi typecheck`,
   etc. — whatever the project ships).
3. Passes its lint/format checks (`cargo clippy -- --deny warnings`,
   `cargo fmt --check`, `pnpm lint`, `prettier --check`, etc.).
4. Leaves `just full-check` green at the repo root.

Bumps are prioritized as **safe-by-default** (semver-compatible minor/patch
across the existing range) with **opt-in major bumps** captured separately
per project — i.e. the PR description lists each major upgrade considered,
whether it was taken, and why.

Lockfiles (`Cargo.lock`, every `pnpm-lock.yaml`) are regenerated in the
same commit as the manifest change for that project, never split.

## Approach

Work project-by-project, commit-by-commit. Do not interleave projects in a
single commit — easier to bisect a regression to one manifest later.

**Phase 1: Baseline (one commit, optional)**

1. Run `just outdated` and `cargo audit` at root; capture output in the PR
   description so reviewers can see the starting state.
2. For each pnpm project, run `pnpm outdated` and capture output.

**Phase 2: Rust bumps**

3. **Root Cargo workspace.** `cargo update` for safe (semver-compatible)
   bumps first; commit `Cargo.lock` alone. Then walk `Cargo.toml` +
   `crates/*/Cargo.toml` for major-version candidates (anything where
   `cargo outdated` shows a newer major). For each: try the bump, run
   `just full-check`. If green, keep; if red and the fix is not trivial,
   revert that specific bump and note it in the PR as deferred.
4. **`examples/iron-log/src-tauri`.** Same playbook: `cargo update` →
   commit lock; then evaluate major bumps for `tauri`, `sea-orm`, `axum`,
   `specta`, `schemars`, `thiserror`. Build via the iron-log scripts
   (`scripts/backend-test`, `scripts/backend-lint`) — these are the
   project's source of truth, not the root `justfile`.

**Phase 3: JS/pnpm bumps**

For each pnpm project, in this order (least entangled first):

5. `packages/admin-types` — typesonly; trivial bump.
6. `site` — Astro + Starlight. Run `pnpm build` after.
7. `packages/nuxt_admin_layer` — Nuxt admin layer. Bumps here can ripple
   into `examples/iron-log/src-nuxt` (which depends on it via
   `workspace:*`-style `file:` linkage); bump this first, then re-resolve
   downstream.
8. `examples/iron-log` (root) — bump pnpm itself, lefthook, commitlint,
   release-it, `@tauri-apps/cli`. Then run `pnpm install` to refresh.
9. `examples/iron-log/src-nuxt` — the heaviest. Bump in this order:
   a. Patch/minor across the board (`pnpm update`).
   b. Major bumps for Nuxt-ecosystem packages (`@nuxt/*`), Vue, Pinia,
      Tailwind, Storybook, Vitest, ESLint, TypeScript — one major bump
      per commit so a regression bisects cleanly.
   c. Run `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build` after
      each major.

**Per-project commit shape**

Each commit:

- Touches exactly one project's manifest(s) and lockfile.
- Has a message like `chore(deps): bump <project> deps` for safe bumps
  or `chore(deps): bump <project> <pkg> to <ver>` for individual majors.
- Leaves the project's own lint/test/build green.

**Final**

10. Run `just full-check` at the repo root. Must pass.
11. PR description summarizes: per-project safe-bump count, per-project
    major bumps taken, and per-project major bumps deferred (with the
    one-line reason — usually "breaking API change, scope creep").

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `Cargo.toml` | modify | root workspace deps (cruet, syn, quote, insta, tempfile). |
| `Cargo.lock` | modify | regenerated. |
| `crates/ontogen-core/Cargo.toml` | modify | crate deps. |
| `crates/ontogen-macros/Cargo.toml` | modify | crate deps. |
| `crates/ontogen-ts/Cargo.toml` | modify | crate deps. |
| `examples/iron-log/src-tauri/Cargo.toml` | modify | Tauri/sea-orm/axum stack. |
| `examples/iron-log/src-tauri/Cargo.lock` | modify | regenerated. |
| `site/package.json` | modify | Astro/Starlight. |
| `site/pnpm-lock.yaml` | new | regenerated (create if missing). |
| `packages/admin-types/package.json` | modify | likely no-op (no deps), include for |
| `packages/nuxt_admin_layer/package.json` | modify | Nuxt admin layer deps. |
| `packages/nuxt_admin_layer/pnpm-lock.yaml` | modify | regenerated. |
| `examples/iron-log/package.json` | modify | pnpm/lefthook/commitlint/release-it/tauri-cli. |
| `examples/iron-log/pnpm-lock.yaml` | new | regenerated (create if missing). |
| `examples/iron-log/src-nuxt/package.json` | modify | Nuxt frontend. |
| `examples/iron-log/src-nuxt/pnpm-lock.yaml` | new | regenerated (create if missing). |


## Acceptance criteria

- [ ] AC-1: `just full-check` passes at the repo root on the final commit.
- [ ] AC-2: `cargo build` and `cargo test` pass in
  `examples/iron-log/src-tauri/` on the final commit.
- [ ] AC-3: For every pnpm project listed above, `pnpm install` succeeds
  and the project's own declared scripts pass — at minimum lint,
  typecheck (where present), test, and build (where present).
- [ ] AC-4: Every dependency manifest change is accompanied in the same
  commit by its regenerated lockfile (no orphan manifest-only or
  lockfile-only commits).
- [ ] AC-5: PR description lists, per project, (a) the safe bumps taken,
  (b) the major bumps taken, (c) the major bumps deferred with a
  one-line reason each.
- [ ] AC-6: `cargo audit` reports no new vulnerabilities introduced
  (i.e. count of advisories does not increase versus baseline).

## Out of scope

- **MSRV bumps.** `rust-toolchain.toml` channel and the `rust-version`
  field in the root `Cargo.toml` stay as-is. If a dep requires a newer
  MSRV, defer that specific bump and note it in the PR.
- **Pinning strategy changes.** Don't switch from caret ranges to exact
  pins (or vice versa), don't introduce a repo-wide `pnpm-workspace.yaml`,
  don't restructure the Cargo workspaces.
- **Migrating to alternative packages** (e.g. swapping `dompurify` for
  another sanitizer because a major changed the API). Either upgrade the
  current package or defer.
- **Manual code refactors to chase a major bump's breaking change** that
  would balloon the diff beyond plain dep work. Defer and file a
  follow-up task.
- **The `OntogenDocs/` submodule or any external dependency tree** — this
  task is the in-repo workspaces only.

## Dependencies

- none

## Discovery context

Requested 2026-05-20 as routine dependency-hygiene work and as an
end-to-end exercise for `/sdlc:orchestrate` against a multi-stack,
multi-lockfile task.
