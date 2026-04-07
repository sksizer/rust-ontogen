## Context

You are running inside a **fresh clone of the template repo**, which is your current working directory. Do NOT `cd` away from it for edits — this is the tree you will commit and push.

Environment variables:

- `DOWNSTREAM_DIR` — absolute path to a local clone of the downstream project you are harvesting from.
- `DOWNSTREAM_NAME` — short name of the downstream project (e.g. `rust-path-opener`), safe for branch names and PR titles.

Shared infrastructure files (candidates for backport):

- `.editorconfig`, `.gitignore`, `.prettierrc.yml`
- `.github/workflows/**`
- `.vscode/settings.json`
- `CLAUDE.md`, `cliff.toml`, `justfile`, `release.toml`, `rust-toolchain.toml`, `rustfmt.toml`
- `scripts/**` (except `scripts/downstream.txt`, which is template-only and MUST NEVER be backported)

Files that are ALWAYS off-limits (project-specific — never backport):

- `Cargo.toml`, `Cargo.lock`
- `README.md`
- `src/**`
- `LICENSE.md`
- `scripts/downstream.txt`

## Task

Follow these steps in order.

### 1. Diff shared files

For each shared-infrastructure path above, diff the downstream copy against the template copy (your cwd):

```
diff -u ./<file> "$DOWNSTREAM_DIR/<file>"
```

For directories, use `diff -ruN ./<dir> "$DOWNSTREAM_DIR/<dir>"`.

### 2. Classify every difference

For each hunk, decide:

- **BACKPORT** — a clear improvement (bug fix, better default, new reusable script, CI fix, etc.) that would benefit every downstream project. Must be high confidence.
- **SKIP** — project-specific, stale drift, stylistic noise, or anything you are not ~95% sure is an improvement.

Check commit history in the downstream for intent:

```
cd "$DOWNSTREAM_DIR" && git log --oneline -10 -- <file> && cd -
```

A commit like "fix ci flake on macos" is a strong BACKPORT signal. "tweak for acme deployment" is SKIP. When in doubt, SKIP.

### 3. Decide whether to proceed

If there are ZERO backport candidates after classification, print:

```
No backport candidates from ${DOWNSTREAM_NAME}.
```

…and exit. Do NOT create a branch, commit, or PR.

### 4. Apply BACKPORT changes

For each BACKPORT candidate, copy/merge the change from `$DOWNSTREAM_DIR` into the template (your cwd). Prefer `cp "$DOWNSTREAM_DIR/<file>" ./<file>` when the entire file should be replaced; use `Edit` for surgical hunks. Never touch off-limits files.

### 5. Run checks

```
just full-check
```

If formatting fails, run `just full-write` and re-check. If clippy or other errors remain, either fix them yourself or **revert the offending backport** (`git checkout -- <file>`) and remove it from your candidate list. Re-run until `just full-check` passes.

If this leaves you with zero applied changes, print `No backport candidates from ${DOWNSTREAM_NAME} survived checks.` and exit without branching/committing/pushing.

### 6. Branch, commit, push, PR

```
git checkout -b chore/backport-from-${DOWNSTREAM_NAME}-$(date +%Y-%m-%d)
git add -A
git commit -m "chore: backport improvements from ${DOWNSTREAM_NAME}"
git push -u origin HEAD
```

Create the PR with `gh pr create`:

- Title: `chore: backport improvements from ${DOWNSTREAM_NAME}`
- Body: a markdown summary listing each backported file, one sentence per file explaining the improvement and citing the downstream commit sha (short form) and subject line when available. End with a "Skipped" section briefly noting anything you classified SKIP, so the reviewer can cross-check your judgment.

Print the PR URL on the final line of your output.
