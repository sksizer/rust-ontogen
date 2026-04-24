## Context

You are running inside a **clone of a cousin repository** (your current working directory). It is NOT a fork of the template; it is an independent project whose structure, CI, and conventions may be very different. You are here only to suggest improvements for explicitly opted-in Rust code within it.

Environment variables:

- `TEMPLATE_DIR` — absolute path to a local checkout of the template repo (the "source of ideas").
- `COUSIN_CONFIG_JSON` — a JSON string describing THIS cousin only. Schema:

  ```
  {
    "name": "<short name>",
    "url": "<git url>",
    "notes": "<free-form context>",
    "targets": [
      {
        "path": "<relative path inside the cousin repo, e.g. crates/foo>",
        "applies": ["<file relative to template root>", ...],
        "check": "<optional shell command for this target>"
      }
    ],
    "global_applies": ["<file relative to template root>", ...],
    "forbidden_paths": ["<glob>", ...]
  }
  ```

- `COUSIN_NAME` — same as `.name` above, pre-extracted for convenience.

## Safety rules — these are not guidelines

1. **Opt-in only.** You may ONLY examine and discuss these file slots:
   - For each `targets[i]`: the files listed in `targets[i].applies`, evaluated at path `targets[i].path/<file>` inside the cousin.
   - Each file in `global_applies`, evaluated at the cousin repo root.
   - Anything else is off-limits — do not diff it, do not open it, do not recommend touching it.

2. **Forbidden paths win.** If a file slot derived from rule 1 matches any pattern in `forbidden_paths`, drop it immediately. Forbidden paths are a hard deny list and always take precedence.

3. **Respect the cousin's conventions.** Read the cousin's own `notes` field carefully. If the cousin says "Bazel-managed", do not recommend changes that assume Cargo/just. If the cousin uses a different formatter style, note it but do not recommend clobbering it.

4. **Never recommend repo-wide changes.** The template is not authoritative here — it is only a source of suggestions scoped to the opted-in slots.

## Task

### 1. Parse the config

```
echo "$COUSIN_CONFIG_JSON" | jq .
```

Extract `targets`, `global_applies`, `forbidden_paths`, and `notes`. If `targets` is empty and `global_applies` is empty, print `No opted-in paths for ${COUSIN_NAME}.` and exit.

### 2. For each opted-in file slot, diff against the template

Walk each `targets[i]`:

```
# For each file in targets[i].applies:
diff -u "./${targets[i].path}/<file>" "$TEMPLATE_DIR/<file>"
```

…and each `global_applies` entry:

```
diff -u "./<file>" "$TEMPLATE_DIR/<file>"
```

If the cousin file does not exist at the expected location, note it as a **candidate new file** rather than a diff.

Before opening any path, verify it does not match `forbidden_paths`. If it does, log "forbidden: <path>" and skip it entirely.

### 3. Classify each diff

For every non-empty diff, decide:

- **SUGGEST-ADOPT** — the template version is clearly better for this target, and the cousin's surrounding project would tolerate the change. Must be ~95% confident. Consider: does the cousin already have this file? Is the diff small and low-risk? Does the cousin's notes field contradict this?
- **SUGGEST-NEW-FILE** — the cousin doesn't have this file at all and would benefit from it. Even more conservative: only recommend if the file is clearly standalone (e.g. a per-crate `rustfmt.toml`) and the target crate path exists.
- **KEEP-COUSIN** — the cousin's version is fine or better; no change recommended.
- **NOT-APPLICABLE** — the diff exists but the change doesn't make sense in this cousin (e.g. the template assumes tooling the cousin doesn't have).

Check the cousin's git history for intent where helpful:

```
git log --oneline -5 -- "<path>"
```

A commit like "match internal style guide" is a strong KEEP-COUSIN signal.

### 4. Produce the report

Print a single markdown report to stdout with this exact structure:

```
# Cousin Review: <COUSIN_NAME>

## Summary
<one-line summary — candidate count and any high-level observations>

## Suggestions

### <target path or "(global)"> — <file>
**Classification:** SUGGEST-ADOPT | SUGGEST-NEW-FILE
**What changes:** <1-2 sentence description>
**Why it might help:** <reason scoped to this target>
**Risk to surrounding project:** <specific things the reviewer should double-check>
**Suggested action:** <"copy file verbatim" | "merge hunk" | "adapt idea">

<repeat per suggestion>

## Considered but skipped
- <target path> — <file>: KEEP-COUSIN / NOT-APPLICABLE — <one-line reason>
<repeat, or omit section if empty>

## Forbidden paths honored
- <path>: skipped because it matched <pattern>
<repeat, or omit section if empty>
```

If there are no suggestions, still print the report with an empty Suggestions section and a clear summary line. Do not create any files, branches, or PRs. Do not modify the working tree.
