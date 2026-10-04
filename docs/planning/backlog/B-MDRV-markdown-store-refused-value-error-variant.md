---
type: backlog
schema_version: '2'
id: B-MDRV
tags:
- markdown
- store
- errors
last_reviewed: '2026-10-04'
---

# A dedicated `markdown_store::Error` variant for a value the store refuses

The markdown store refuses two kinds of value before it writes: a float `NaN` ([ADR 0006](../../architecture/0006-ordering-on-both-store-backends.md) §3, `src/store/nan.rs`) and an integer outside `i64` (`emit_integer_range_checks` in `src/store/backends/markdown/gen_crud.rs`). Both reuse `markdown_store::Error::Serialize { message }` through the generator's `serialize_error` helper, because no variant fits. That variant is documented as "A value could not be serialized into a YAML frontmatter mapping" (`crates/markdown-store/src/error.rs`) and its `Display` is `frontmatter serialize error: {message}`. A caller who gets `frontmatter serialize error: Node.weight: NaN cannot be stored` is told the YAML emitter failed, when the store declined the value on purpose.

## Proposal

Add a variant, for example `Error::Refused { message }` with `Display` `value refused: {message}`, and emit it from both checks instead of `Serialize`. Keep `Serialize` for what it names.

The HTTP status stays `500`. A refused value cannot arrive over a transport (JSON has no `NaN`, and the integer range is the schema's concern), so reaching the check is a server bug: a hook or a direct caller supplied it. The change is to the message and the variant, not to the status mapping.

## What it touches

- `crates/markdown-store/src/error.rs`: the new variant, its doc and `Display`.
- `src/store/backends/markdown/gen_crud.rs`: `serialize_error` (or a renamed helper) and the two emitters that call it.
- The `markdown_store_*` insta snapshots in `src/snapshots/` and the `src/store/tests.rs` assertions that name `Error::Serialize`.
- Regenerated markdown examples that contain a NaN or integer-range check.
- Consumers: a catch-all `From<markdown_store::Error>` impl that matches variants one by one gains an arm. This is a breaking change, so it belongs in the upgrading guide, and the sentence in `site/src/content/docs/guides/field-types-and-roles.mdx` that names `Error::Serialize` changes with it.
