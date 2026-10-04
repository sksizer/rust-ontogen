---
type: backlog
schema_version: '2'
id: B-FMQT
tags:
- markdown
- frontmatter
- okf
- fidelity
last_reviewed: '2026-10-04'
---

# A markdown record rewrite unquotes string dates in its frontmatter

When the markdown store rewrites a record, it re-emits the whole frontmatter block through the YAML emitter. A string such as `'2026-06-06'` comes back unquoted (`2026-06-06`), so a YAML 1.1 consumer (Obsidian, most OKF tooling, PyYAML) reads a date where the file held a string. This is a fidelity loss for OKF vaults ([ADR 0005](../../architecture/0005-okf-markdown-vaults.md)), and E0004 phase 3a widened it: every relationship write and every `has_many` child re-parent now rewrites a record, not only a plain `update`.

## Repro

Against `examples/tasks-tracker` (verified at https://github.com/sksizer/rust-ontogen/pull/203). `data/vault/tasks/ship-the-emitter.md` starts with:

```yaml
---
type: Task
title: Ship the emitter
status: closed/done
created: '2026-06-06'
epic_id: '[[markdown-backend]]'
tags:
- '[[codegen]]'
---
```

Run `cargo run` in `examples/tasks-tracker`, then link a tag:

```sh
curl -X POST -H 'Content-Type: application/vnd.api+json' \
  -d '{"data":[{"type":"tags","id":"release"}]}' \
  http://127.0.0.1:3002/api/tasks/ship-the-emitter/relationships/tags   # 204
git diff examples/tasks-tracker/data/vault
```

```diff
-created: '2026-06-06'
+created: 2026-06-06
 epic_id: '[[markdown-backend]]'
 tags:
 - '[[codegen]]'
+- '[[release]]'
```

The new tag line is intended. The `created` line is not. Key order is preserved (`merge_serialize` inserts in place); the quoting of scalars is what changes.

## Where

`crates/markdown-store/src/frontmatter.rs`: `Document::render` writes a dirty document with `serde_norway::to_string(&self.fm)`. A clean document renders as its original source byte for byte, so only records that actually change are affected, but for those the whole block is normalised (scalar quoting, list layout, comments dropped). The doc comment on `render` already names this as planned work alongside the corpus fidelity harness.

## What fixing it needs

Either of:

- Surgical per-key rewrites: splice the changed keys into the original source text and leave every other line, comment and quoting style alone. This is the complete fix and what the `render` comment anticipates.
- A smaller step: after emitting, quote every string scalar that YAML 1.1 would resolve to a non-string (dates, timestamps, `yes`/`no`/`on`/`off`, numbers), so strings stay strings across a rewrite. Quoting style and comments are still normalised.

Add a fidelity test that rewrites a record holding a string date and asserts the value is still a string under a YAML 1.1 reading.
