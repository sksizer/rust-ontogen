# Releasing

Releases are **fully automated** by [`release-plz`](https://release-plz.dev/)
running in CI. You do **not** run any release command locally — you merge a PR
that a bot prepares for you.

## The flow, end to end

```
  merge feature PRs to main
            │
            ▼
  ┌───────────────────────────────────────────────┐
  │  release-plz CI (on every push to main)         │
  │  opens/updates a "chore: release" PR:           │
  │    • bumps each changed crate's version         │
  │    • regenerates CHANGELOG.md (git-cliff)        │
  └───────────────────────────────────────────────┘
            │
            ▼
  you review + merge the "chore: release" PR
            │
            ▼
  ┌───────────────────────────────────────────────┐
  │  release-plz CI (on that merge):                │
  │    • git tag each released crate                │
  │    • publish to crates.io                        │
  │    • create a GitHub Release                     │
  └───────────────────────────────────────────────┘
```

So the only human steps are: **merge feature work**, then **merge the release
PR** when you're ready to cut a release. Everything else is the bot.

## Where it's configured

| File | What it controls |
|------|------------------|
| [`.github/workflows/release-plz.yml`](./.github/workflows/release-plz.yml) | The CI job: triggers on push to `main`, runs the `release-plz` action. |
| [`release-plz.toml`](./release-plz.toml) | Release behavior: which crates, publish on/off, semver gate, GitHub releases. |
| [`cliff.toml`](./cliff.toml) | Changelog format and how commits are grouped (shared with nothing else now). |

Key `release-plz.toml` settings:

- `publish = true` — publishes to crates.io when the release PR merges.
- `git_release_enable = true` — creates a GitHub Release per release.
- `semver_check = true` — runs `cargo-semver-checks` before publishing.
- `changelog_config = "cliff.toml"` — reuse the git-cliff config for changelogs.
- `pr_name = "chore: release"` — the title of the automated release PR.

## What gets released

release-plz processes **every workspace member by default**; the `[[package]]`
blocks in `release-plz.toml` are *overrides*, not an allowlist. Current scope:

| Crate | Released? | Why |
|-------|-----------|-----|
| `ontogen` | ✅ | The main crate. |
| `ontogen-core` | ✅ | Published dependency. |
| `ontogen-macros` | ✅ | Published dependency. |
| `ontogen-ts` | ✅ | Processed by default (not excluded). |
| `markdown-store` | ❌ `release = false` | Not yet API-stable; also `publish = false` in its `Cargo.toml`. |
| `markdown-pilot` | ❌ `release = false` | CI pilot consumer, never published. |

To add or drop a crate from the release set, edit its `[[package]]` block in
`release-plz.toml` (add `release = false` to exclude).

## How the version is decided

release-plz reads **Conventional Commits** since each crate's last release and
picks the bump automatically. `ontogen` is pre-1.0, so 0.x SemVer applies — the
**minor** slot is the breaking tier:

| Commit type | Bump (0.x) |
|-------------|-----------|
| `fix:` | patch |
| `feat:` | minor |
| `feat!:` / `fix!:` / `BREAKING CHANGE:` | minor (the 0.x breaking tier) |

You influence the release by how you write commit messages on the feature
branches — there is no manual version bump. `cargo-semver-checks` runs as a gate
before publishing and will block a bump that's too small for the API delta.

> Merge commits (`Merge pull request #NNN …`) are non-conventional and are
> filtered out of the changelog by `cliff.toml`; the underlying `feat:`/`fix:`
> commits on the branches are what appear. A merge-based workflow produces a
> correct changelog.

## Prerequisites (one-time setup)

These must be in place for the automation to work end to end:

1. **`CARGO_REGISTRY_TOKEN` repo secret** — a crates.io API token. Needed only at
   the *publish* step (when a release PR merges), not to open the PR.
   - Set via: `gh secret set CARGO_REGISTRY_TOKEN` (paste a crates.io token), or
   - Prefer **crates.io Trusted Publishing (OIDC)** — no long-lived token; see
     the release-plz docs.
2. **"Allow GitHub Actions to create and approve pull requests"** must be enabled
   (Settings → Actions → General → Workflow permissions). Without it, release-plz
   cannot open the release PR. The workflow already grants the job
   `contents: write` and `pull-requests: write`.

> Optional but recommended: have release-plz open the PR with a **PAT or GitHub
> App token** instead of the default `GITHUB_TOKEN`. PRs opened by the default
> token do **not** trigger other workflows (like `ci.yml`), so the release PR
> won't get CI runs otherwise.

## Common gotcha: "working directory has uncommitted changes"

release-plz refuses to run if any file is **both git-tracked and matched by a
committed `.gitignore`** — it sees them as uncommitted noise. Symptom in the CI
log:

```
ERROR failed to update packages
  1: the working directory of this project has uncommitted changes. If these
     files are both committed and in .gitignore, either delete them or remove
     them from .gitignore.
```

Find offenders and untrack them (they stay on disk):

```sh
git ls-files --cached --ignored --exclude-standard
git rm --cached <each file>
```

(This is what tripped the pre-`0.3.0` runs — four Tauri `gen/schemas/*.json`
build artifacts were checked in *and* gitignored. They're now untracked.)

## Manual escape hatch

There is no local release recipe anymore — release-plz owns releases. If you
ever must publish by hand (registry outage, emergency), do it deliberately and
outside this pipeline:

```sh
cargo semver-checks --baseline-rev "$(git describe --tags --abbrev=0)"  # just semver-check
cargo publish -p <crate>                                                # then a real publish
```

...and reconcile the tag/changelog afterward so release-plz's next run agrees
with reality.
