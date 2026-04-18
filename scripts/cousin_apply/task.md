## Context

You are running inside a **clone of a cousin repository** (your current working directory). Your job is to apply a small set of high-confidence, template-sourced improvements — limited strictly to the opted-in slots in `COUSIN_CONFIG_JSON` — and open a single PR for this cousin containing all surviving changes.

Environment variables:

- `TEMPLATE_DIR` — absolute path to a local checkout of the template repo (the source of bytes to copy).
- `COUSIN_CONFIG_JSON` — full JSON object describing THIS cousin. Schema documented in `scripts/cousin_review/task.md`.
- `COUSIN_NAME` — short name, safe for branch names and PR titles.

## Hard safety rules

1. **Opt-in only.** The only file slots you may read, diff, or modify:
   - For each `targets[i]`: files listed in `targets[i].applies`, resolved at path `targets[i].path/<file>` inside the cousin.
   - Files in `global_applies`, resolved at the cousin repo root.
   Anything else is off-limits. Do not edit, delete, rename, or create anything outside these slots.

2. **Forbidden paths win.** If a candidate path matches any pattern in `forbidden_paths`, drop it immediately.

3. **Never retype bytes.** Propagate changes by `cp "$TEMPLATE_DIR/<file>" "<cousin target path>/<file>"`. Never use the `Write` tool to reconstruct a template file from memory — that produces bogus full-file diffs from subtle formatting drift.

4. **One PR per cousin, total.** All surviving changes across all targets go in a single branch and a single PR.

5. **If a check fails, revert the offending change.** Do not try to "fix up" the cousin's surrounding code. Your mandate is limited to the opted-in slots.

## Task

### 1. Parse the config and verify constraints

```
echo "$COUSIN_CONFIG_JSON" | jq .
```

Build two flat lists:
- `SLOTS` — the full set of allowed file slots from rule 1 above, as absolute paths inside the cousin.
- `FORBIDDEN` — the `forbidden_paths` globs.

Remove any SLOT whose path matches any FORBIDDEN glob. If the SLOTS list is now empty, print `No opted-in slots for ${COUSIN_NAME}.` and exit without branching or committing.

### 2. Diff each slot against the template

For each slot, run:

```
diff -u "<cousin slot path>" "$TEMPLATE_DIR/<template file>"
```

Decide per slot:

- **APPLY** — the template version is clearly better and low-risk for this cousin. ~95% confidence bar.
- **SKIP** — anything else. Log the reason.

Use the cousin's `notes` field as context for judging applicability. If notes say "Bazel-managed, no Cargo", do not apply template files that assume Cargo/just tooling.

### 3. Apply surviving changes

For every APPLY slot, copy bytes verbatim:

```
mkdir -p "$(dirname "<cousin slot path>")"
cp "$TEMPLATE_DIR/<template file>" "<cousin slot path>"
```

Use `cp` for full-file replacements. Use surgical `Edit` hunks ONLY if the cousin's existing file has project-specific content you must preserve — and even then, change as few lines as possible.

### 4. Run per-target checks

For each `targets[i]` whose files you modified, determine the check command using this priority order:

1. If `targets[i].check` is set in the JSON, run exactly that command from the cousin repo root.
2. Otherwise, if a `Cargo.toml` declares a package at `targets[i].path`, run `cargo check --manifest-path "targets[i].path/Cargo.toml"`.
3. Otherwise, skip checks for that target and note it in the PR body as "unchecked".

If a check fails, revert every change you made inside that target (`git checkout -- <slot path>`) and record the target under "reverted" in your notes. Do NOT attempt to fix the cousin's surrounding code.

`global_applies` changes are not checked here — the cousin's CI will catch anything broken when the PR lands.

### 5. Decide whether to PR

After reverts, run `git status`. If nothing is modified, print `No cousin changes survived checks for ${COUSIN_NAME}.` and exit without branching or committing.

### 6. Branch, commit, push, single PR

```
git checkout -b chore/template-sync-$(date +%Y-%m-%d)
git add -A
git commit -m "$(cat <<'MSG'
chore: sync opted-in Rust files from shared template

Applied the following template-sourced changes to opted-in paths:
- <slot> — <what changed, one line>

Skipped (with reason):
- <slot> — <reason>

Reverted after failed checks:
- <target> — <reason>, or "None"
MSG
)"
git push -u origin HEAD
```

Open a single PR with:

- Title: `chore: sync opted-in Rust files from shared template`
- Body: a markdown summary grouping applied changes by target, plus a "Skipped" section explaining what was considered but not applied, and a "Checks" section listing which command ran against which target (or "unchecked").

Print the PR URL on the final line of your output.
