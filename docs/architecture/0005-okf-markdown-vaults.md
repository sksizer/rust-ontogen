# ADR 0005 — OKF-conformant markdown vaults

## Status

**accepted** (2026-10-03). Implemented in phase 1 of
[E0005](../planning/epics/okf-markdown-vault.md) by
[#194](https://github.com/sksizer/rust-ontogen/pull/194). Amends
[ADR 0001](0001-markdown-as-store-backend.md).

## Context

The vault the markdown backend writes is one emitted field short of a
conformant [Open Knowledge Format 0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
bundle: records need a non-empty `type`, and `index.md` / `log.md` are
reserved filenames. OKF also gives fixed meanings to a handful of frontmatter
keys (`status`, `generated`, `verified`, `sources`, ...) that consumers may
already use for other things. The gap analysis lives in the epic; this ADR
records the decisions that closed phase 1.

## Decision

Epic decisions (2026-10-03):

1. **`type` casing.** The vault writes the entity struct name by default
   (`WorkoutSet`), overridable per entity with
   `#[ontology(entity, type_name = "...")]`. The IR default for `type_name`
   changes from the snake name; nothing read it before.
2. **`index.md` is off by default**, behind one knob (`okf.index`, phase 2).
3. **Relation encoding stays `[[id]]` wikilinks.** `LinkEncoding::MarkdownPath`
   is a deferred opt-in.
4. **Scope.** `log.md` is dropped (git history is the change log) but the id
   `log` stays reserved; the docs phase is removed from the epic.
5. **A `status` field with a non-OKF vocabulary is a build-time warning**, not
   an error. `#[ontology(frontmatter_name = "...")]` is the fix path.

Choices phase 1 made that refine or depart from the epic text:

- **The reserved-key check lives in the markdown generator, not in
  `parse.rs`.** Only markdown consumers write a vault; SeaORM-only consumers
  must not get OKF diagnostics. The check needs the enum definitions (for
  `status`), hence `gen_markdown_io(&SchemaOutput, ...)`. The code is
  `src/persistence/markdown/okf.rs`.
- **`type` is stamped only on real (dirty) writes.** A no-op update writes
  nothing, so an untyped hand-authored vault reads unchanged and gains `type`
  only when a record is genuinely modified.
- **Flat layout filters by `type`.** Untyped records are admitted by every
  entity. Another entity's records are NotFound on get/modify/remove and are
  excluded from list/count. `PerEntityDir` tolerates a mismatched `type` (the
  directory decides) and normalizes it on the next real write.
- **Reserved ids are matched case-insensitively**, because case-insensitive
  filesystems alias `Index.md` onto `index.md`. A derived slug that would be
  reserved dedupes to `index-2`. Directory segments are unaffected.
- **`markdown-store` adopts the OKF bundle profile as fixed policy** (reserved
  stems in the walk and in `validate_id`). This is deliberate, so that a
  future extraction of the crate (for example into rust-markdown) knows the
  coupling is intentional rather than accidental.
- **Flat `remove` of a record with unparseable frontmatter errors** rather
  than deleting. The store cannot tell which entity owns the file, and
  deletion is not widened because an unparseable file may belong to another
  entity. It can still be deleted under `PerEntityDir`, or by hand.
- **`frontmatter_name` is the fix path** for reserved-key warnings. Renaming
  a key on an existing vault orphans the old key: existing records must be
  renamed by hand, otherwise reads fail with a missing-field error for a
  required field.

## Consequences

- Every example vault and golden fixture gains one `type:` line; byte-stable
  goldens change, consumers do not break. An existing vault without `type`
  still reads.
- Other OKF consumers can read an ontogen vault, and Obsidian compatibility is
  kept.
- `type` becomes generator-owned: a schema field whose effective key is `type`
  is a build error.
- Consumers with their own `status` vocabulary see a warning until they
  rename the key.
- `markdown-store` is no longer a neutral markdown library: it hard-codes two
  reserved ids.

## Alternatives considered

- **Reserved-key check in `parse.rs`.** Rejected: it would emit OKF
  diagnostics for SeaORM-only consumers.
- **Stamping `type` on every write, or on read.** Rejected: dirties files that
  did not change and breaks the byte-for-byte no-op guarantee (ADR 0001
  amendment 6).
- **Making reserved-key violations errors.** Rejected by decision 5: a
  consumer vocabulary is not wrong, only not OKF's.
- **Widening Flat `remove` to delete unparseable records.** Rejected: it could
  delete another entity's file.

## Notes

- Epic: [E0005](../planning/epics/okf-markdown-vault.md). Canonical user
  documentation: the
  [markdown backend guide](https://github.com/sksizer/rust-ontogen/blob/main/site/src/content/docs/guides/markdown-backend.mdx).
- Phase 2 (`okf.index`, `okf.generated_by`) will extend this ADR or add an
  amendment.
