## Goal

Bring this repository up to date with its upstream template, producing a PR whose diff is **minimal and surgical** — exactly the bytes that changed upstream, nothing more.

> **CRITICAL: do not retype file contents.** If you read a template file and then rewrite it via the Write tool, even tiny formatting differences (whitespace, trailing newline, comment reflow, EOL) will make the PR look like a full-file rewrite. Always propagate changes by **copying bytes** from the upstream clone, never by reconstructing them.

## 1. Clone the upstream template into a tempdir

```
TEMPLATE_DIR="$(mktemp -d)/rust-template"
git clone --quiet https://github.com/sksizer/rust-template "$TEMPLATE_DIR"
```

You will compare against, and copy from, `$TEMPLATE_DIR` for the rest of this task. Do NOT read template files from any other source (training data, context, etc.).

## 2. Identify candidate files

Shared infrastructure files that should track the template:

- `.editorconfig`, `.gitignore`, `.prettierrc.yml`
- `.github/workflows/**`
- `.vscode/settings.json`
- `CLAUDE.md`, `cliff.toml`, `justfile`, `release.toml`, `rust-toolchain.toml`, `rustfmt.toml`
- `scripts/**` (except `scripts/downstream.txt`, which is template-only)

Files that are project-specific and MUST be left alone:

- `Cargo.toml`, `Cargo.lock`
- `README.md`
- `src/**`
- `LICENSE.md`
- `scripts/downstream.txt`

## 3. Diff every candidate

For each candidate path, run:

```
diff -u ./<path> "$TEMPLATE_DIR/<path>"
```

(or `diff -ruN ./<dir> "$TEMPLATE_DIR/<dir>"` for directories).

If the diff is empty, **skip the file entirely** — do not touch it. If the file does not exist locally but exists in the template, treat it as a new file to bring over.

## 4. Propagate changes — copy, never retype

For each file with a non-empty diff:

- **Default: copy the file verbatim from the template**, preserving bytes exactly:
  ```
  cp "$TEMPLATE_DIR/<path>" ./<path>
  ```
  This is the right choice for ~95% of files. It guarantees a minimal diff because the resulting file is byte-identical to upstream.

- **Only use surgical `Edit` hunks** when the local file has intentional, project-specific modifications you must preserve. In that case, apply the smallest possible edit that brings the upstream change in. **Never use the `Write` tool to rewrite a whole template-tracked file from your own output** — always `cp` instead.

For new files (present in template, absent locally), `cp` them in.

If a file is project-specific (e.g. local edits to `justfile` for this project's commands), record it in the "Did not bring over" list and leave it untouched.

## 5. Verify diffs are minimal

After all copies/edits, run:

```
git diff --stat
git diff
```

For each touched file, the displayed diff should match (a subset of) what `diff -u ./<path> "$TEMPLATE_DIR/<path>"` showed in step 3 **before** you copied. If any file shows a much larger diff than expected — especially if it looks like every line changed — you have a line-ending or whitespace problem. Investigate before continuing:

```
file ./<path> "$TEMPLATE_DIR/<path>"   # check for CRLF vs LF
git diff --check ./<path>              # check for whitespace errors
```

If you cannot produce a minimal diff for a given file, revert it (`git checkout -- <path>`) and list it under "Did not bring over" with the reason.

## 6. Run quality checks

```
just full-check
```

If formatting fails, run `just full-write` and re-check. If clippy errors remain, fix them. Re-run until `just full-check` passes.

## 7. Decide whether to PR

If after all of the above `git status` shows no changes, print:

```
Already up to date with upstream template. No PR needed.
```

…and exit. Do not branch, commit, or push.

## 8. Branch, commit, push, PR

```
git checkout -b chore/update-from-template
git add -A
git commit -m "$(cat <<'MSG'
chore: update project from upstream template

Performed the following:
- <list each file changed, one per line>

Did not bring over the following because of project-specific overrides:
- <list anything intentionally skipped, or "None">
MSG
)"
git push -u origin HEAD
```

Open a PR with:

- Title: `chore: update from upstream template`
- Body: the same summary as the commit message, plus a "Verification" line confirming you ran `git diff` after copying and the diffs were minimal.

Print the PR URL on the final line of your output.
