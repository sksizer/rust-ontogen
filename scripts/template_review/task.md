## Context

You are in a clone of a downstream project that was originally based on a shared Rust template.
The path to a local checkout of the template is provided in the environment variable `TEMPLATE_DIR`.

The template contains shared infrastructure files that are intended to be identical (or nearly so) across all downstream projects. Examples include:

- `.editorconfig`, `.gitignore`, `.prettierrc.yml`
- `.github/workflows/**`
- `.vscode/settings.json`
- `CLAUDE.md`, `cliff.toml`, `justfile`, `release.toml`, `rust-toolchain.toml`, `rustfmt.toml`
- `scripts/**` (except `scripts/downstream.txt`, which is template-only)

Files that are intentionally project-specific and should be IGNORED:
- `Cargo.toml`, `Cargo.lock`
- `README.md`
- `src/**`
- `LICENSE.md`
- `scripts/downstream.txt`

## Your task

Find changes in this downstream project that represent **generalizable improvements** to the shared infrastructure, which the template itself does not yet have.

### 1. Diff the shared files

For each shared-infrastructure file listed above (and any other non-project file you notice), compare the downstream copy against the template copy at `$TEMPLATE_DIR`. Use:

```
diff -u "$TEMPLATE_DIR/<file>" "<file>"
```

or walk directories with `diff -ruN "$TEMPLATE_DIR/<dir>" "<dir>"`.

### 2. Classify each difference

For every non-trivial difference, decide whether it is:

- **BACKPORT** — a genuine improvement (bug fix, new capability, better default, cleaner config, new script, etc.) that would benefit every downstream project if added to the template.
- **PROJECT-SPECIFIC** — a customization that only makes sense for this project (e.g. a workflow step tailored to this repo's deploy target). Do not recommend these.
- **DRIFT** — stale local edits, accidental changes, or noise. Do not recommend these.
- **TEMPLATE-NEWER** — the template has changes the downstream doesn't. Ignore; `cargo_update_all` handles that direction.

Be conservative: when in doubt, mark it PROJECT-SPECIFIC or DRIFT, not BACKPORT.

### 3. Check git log for intent

For each candidate BACKPORT, run `git log --oneline -- <file>` on the most recent commits touching it, to understand *why* the change was made. A commit message like "fix ci flake on macos" is a strong BACKPORT signal; "tweak for acme deployment" is PROJECT-SPECIFIC.

### 4. Produce the report

Print a single markdown report to stdout with this exact structure:

```
# Template Backport Review: <repo name>

## Summary
<one-line summary: e.g. "3 candidates found" or "No backport candidates — downstream is in sync with template on shared files.">

## Backport Candidates

### <file path>
**What changed:** <1-2 sentence description>
**Why it matters:** <why this benefits all downstream projects>
**Origin commit(s):** <short sha — subject line, if found>
**Suggested action:** <"copy file verbatim" | "merge hunk" | "adapt idea">

<repeat per candidate>

## Skipped (project-specific or drift)
- <file>: <one-line reason>
<repeat, or omit section if empty>
```

If there are no backport candidates, still print the report with an empty "Backport Candidates" section and a clear summary line. Do not create any files, branches, or PRs. Do not modify the working tree.
