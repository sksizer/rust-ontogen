# Releasing `ontogen`

This document explains how a release happens in this repo — the tooling, each
step, and what to expect (including a couple of non-obvious gotchas). It is a
reference, not a script you run by hand: the actual work is driven by
`cargo-release` via `just`.

## TL;DR

```sh
# 1. Be on a clean `main`, synced with origin.
git switch main && git pull

# 2. Preview (never writes/pushes anything you can't undo — but see gotchas below).
just release-dry-run minor      # or: patch / major

# 3. Do it for real (bumps, changelogs, commits, tags, pushes).
just release minor
```

That is the whole flow. Everything below explains what those two commands
actually do.

## The tooling

Three tools, wired together in [`release.toml`](./release.toml) and invoked
through recipes in the [`justfile`](./justfile):

| Tool | Role |
|------|------|
| [`cargo-release`](https://github.com/crate-ci/cargo-release) | Orchestrates the release: version bump, commit, tag, push. Reads `release.toml`. |
| [`git-cliff`](https://git-cliff.org/) | Regenerates `CHANGELOG.md` from Conventional Commits. Config in [`cliff.toml`](./cliff.toml). |
| [`cargo-semver-checks`](https://github.com/obi1kenobi/cargo-semver-checks) | Gate: confirms the version bump is big enough for the API changes since the last tag. |

The `just` recipes:

```
release level="patch"          # cargo release {{level}} --execute
release-dry-run level="patch"  # cargo release {{level}} --no-confirm   (no --execute = dry run)
changelog                      # git-cliff --output CHANGELOG.md        (regenerate by hand)
semver-check                   # cargo semver-checks --baseline-rev <latest tag>
```

`level` is `patch`, `minor`, or `major`.

## What gets released

**Only the root `ontogen` crate.** The workspace has six members, but the root
`Cargo.toml` is both `[workspace]` and `[package]`, and `cargo release` run from
the root operates on that one package. The sub-crates
(`ontogen-core`, `ontogen-macros`, `ontogen-ts`, `markdown-store`,
`markdown-pilot`) keep their own versions and are **not** bumped, tagged, or
published by this flow — you will see them logged as "skipped" during a release.

**Nothing is published to crates.io.** `release.toml` sets `publish = false`.
A release produces a git commit and a git tag, pushed to `origin`. That's it.

## Versioning model (0.x)

`ontogen` is pre-1.0, so SemVer's 0.x rules apply — the **minor** slot is the
breaking-change tier:

| Change | Bump | Example |
|--------|------|---------|
| Breaking API change | **minor** | `0.2.2 → 0.3.0` |
| New feature, backwards compatible | **minor** (or patch) | `0.3.0 → 0.4.0` |
| Bug fix, no API change | **patch** | `0.3.0 → 0.3.1` |

`cargo-semver-checks` understands this: for a 0.x crate it treats a minor bump
as sufficient for breaking changes. (The `0.3.0` release carried three breaking
changes and passed the gate on a minor bump for exactly this reason.)

## The steps, in order

When you run `just release minor`, `cargo-release` performs these steps. Each
maps to a setting in `release.toml`.

### 1. Preconditions

- **Branch check** — `allow-branch = ["main"]`. Refuses to run off `main`.
- **Clean tree** — refuses to run with uncommitted changes. Commit or stash first.

### 2. Version bump

Rewrites `version = "..."` in the root `Cargo.toml` (and updates `Cargo.lock`).
For a minor bump: `0.3.0 → 0.4.0`.

### 3. Pre-release hook

`release.toml`'s `pre-release-hook` runs a shell command that does two things,
in order, with `NEW_VERSION` already set to the bumped version:

1. **Regenerate the changelog** — `git-cliff --tag "v${NEW_VERSION}" --output
   CHANGELOG.md && git add CHANGELOG.md`. git-cliff reads the whole history,
   groups commits by Conventional-Commit type (`feat` → Added, `fix` → Fixed,
   `refactor`/etc. → Changed) per `cliff.toml`, and writes a fresh
   `CHANGELOG.md` with a new `## [x.y.z] - <date>` section on top. It **stages**
   the result so it lands in the release commit.
2. **Semver gate** — `cargo semver-checks --baseline-rev <latest tag>`. Builds
   the current crate and the crate at the previous tag, diffs the public API,
   and fails if the bump is too small for the changes. Skipped only if there is
   no prior tag.

> Note: `filter_unconventional = true` in `cliff.toml` means non-conventional
> commits (including the `Merge pull request #NNN …` commits) are dropped from
> the changelog. The underlying `feat:`/`fix:`/`refactor:` commits on the
> branches still appear, so a merge-commit workflow produces a correct
> changelog — the merge commits themselves are just filtered out. You'll see a
> `git-cliff … N commit(s) were skipped due to parse error(s)` line; that's the
> filtering, not an error.

### 4. Release commit

Commits the version bump + regenerated changelog with the message from
`pre-release-commit-message`: `chore: release v{{version}}` →
`chore: release v0.4.0`.

> **Why `consolidate-commits = false` is set.** Because the root is a workspace,
> `cargo-release` defaults to *consolidated* commits, and in that mode
> `{{version}}` is **not** available in the commit message — it renders
> literally as `chore: release v{{version}}` (this is exactly what the `0.3.0`
> release commit shows). Setting `consolidate-commits = false` gives each
> released package its own commit and re-enables `{{version}}`. Since we only
> ever release the one root crate, this is a single commit anyway. `{{version}}`
> in `tag-name` is unaffected either way because tag names are per-crate.

### 5. Tag

Creates an **annotated** git tag: `tag-name = "v{{version}}"` → `v0.4.0`, with
`tag-message = "Release {{tag_name}}"`. Annotated means `git rev-parse v0.4.0`
returns the tag object; `git rev-parse v0.4.0^{commit}` returns the commit it
points at.

### 6. Push

`push = true` pushes the release commit and the tag to `origin` in one step:

```
   <old>..<new>  main -> main
 * [new tag]      v0.4.0 -> v0.4.0
```

### 7. Publish — skipped

`publish = false`, so `cargo publish` never runs. No crates.io upload.

## Dry-run first — and its two gotchas

Always preview with `just release-dry-run <level>`. It simulates steps 1–7
without committing, tagging, or pushing. **But two side effects of the
pre-release hook are real even in a dry run**, because the hook is a real shell
command:

### Gotcha 1: the semver gate can *fail spuriously* in a dry run

In a dry run, `cargo-release` does **not** write the version bump to
`Cargo.toml`. So when the hook runs `cargo semver-checks`, it compares the last
tag against a working tree that is still at the *old* version. If there are
breaking changes since the tag, semver-checks sees "breaking changes with no
version bump" and fails with:

```
Summary semver requires new major version: N major and 0 minor checks failed
error: release of ontogen aborted by non-zero return of prerelease hook.
```

This is a **dry-run artifact, not a real problem.** On the real
`just release`, step 2 writes the bumped version *before* the hook runs, so
semver-checks compares the bumped version and passes. To confirm the real run
will pass, temporarily set the new version in `Cargo.toml` and run the gate
directly:

```sh
# check that 0.4.0 covers the API changes since the last tag
sed -i '' 's/^version = ".*"$/version = "0.4.0"/' Cargo.toml
cargo semver-checks --baseline-rev "$(git describe --tags --abbrev=0)"
git checkout -- Cargo.toml   # revert
```

A green `Summary no semver update required` there means the real release's gate
will pass.

### Gotcha 2: the dry run dirties `CHANGELOG.md`

The hook runs `git-cliff --output CHANGELOG.md && git add CHANGELOG.md`
unconditionally — so after a dry run, `CHANGELOG.md` is modified and staged.
Restore it before doing anything else:

```sh
git restore --staged CHANGELOG.md && git checkout -- CHANGELOG.md
```

You can preview the changelog section a release *would* generate without
touching the file at all:

```sh
git-cliff --tag v0.4.0 --unreleased     # prints the new section to stdout
```

## After the release

`cargo-release` pushes the tag but does **not** create a GitHub Release page
(that's a crates.io/`gh` concern, separate from the git tag). If you want the
notes to show under GitHub → Releases:

```sh
gh release create v0.4.0 --title v0.4.0 --notes-from-tag
# or paste the CHANGELOG section:
# gh release create v0.4.0 --title v0.4.0 --notes "$(sed -n '/## \[0.4.0\]/,/## \[/p' CHANGELOG.md | sed '$d')"
```

For a release with breaking changes, also consider a short migration note for
downstream consumers.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `error: uncommitted changes detected` | Dirty tree — often a leftover `CHANGELOG.md` from a prior dry run | `git restore --staged CHANGELOG.md && git checkout -- CHANGELOG.md`, then retry |
| Dry-run semver gate fails, "requires new major version" | Gotcha 1 — version not bumped in dry run | Not a real failure; verify via the manual `cargo semver-checks` snippet above |
| Commit message is literally `chore: release v{{version}}` | Consolidated-commit mode swallowed `{{version}}` | Already fixed via `consolidate-commits = false`; if it reappears, that setting was lost |
| `not on allowed branch` | Running off `main` | `git switch main` |
| Sub-crate versions unexpectedly bumped | `consolidate-commits`/`shared-version` misconfigured | Only the root `ontogen` should bump; check `release.toml` |

## Reference: the config files

- [`release.toml`](./release.toml) — cargo-release settings (branch guard,
  commit/tag templates, push, the pre-release hook).
- [`cliff.toml`](./cliff.toml) — git-cliff changelog format and commit grouping.
- [`justfile`](./justfile) — the `release`, `release-dry-run`, `changelog`, and
  `semver-check` recipes.
