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
- Phase 2 (`okf.index`, `okf.generated_by`) is recorded in the amendment
  below.

## Amendment (2026-10-03): phase 2, index files and provenance stamps

Phase 2 of [E0005](../planning/epics/okf-markdown-vault.md) adds the two opt-in
OKF artifacts. Both are off by default; with both off, vault bytes do not
change. Canonical user documentation stays in the markdown backend guide.

- **Config placement.** `MarkdownIoConfig` (and `MarkdownIoOptions`) gain
  `okf: OkfOptions { index, generated_by }`. `gen_markdown_io` emits a
  `vault.rs` with `VAULT_ROOT` and `open_vault(root)`, which builds the
  `VaultHandle` with the layout, id strategy, list cap and OKF options from the
  config. This makes the build configuration the single source for vault
  settings that every consumer previously repeated by hand in
  `VaultHandle::new(...)`, where they could drift from the build. The options
  are not carried on `MarkdownIoOutput`: `vault.rs` is emitted straight from
  the config, `gen_store` never builds a vault, and the parity test constructs
  `MarkdownIoOutput` literally. The runtime knobs are also builder methods
  (`with_okf_index`, `with_generated_by`, `with_clock`) so tests can use them.
- **Index files (§8).** The root and every directory holding records carry an
  `index.md`: type-grouped `# <type>` sections sorted by name, then `# Untyped`,
  then `# Directories`; the root declares `okf_version: "0.2"`. The format is
  byte-stable so regeneration can skip unchanged files.
- **Regeneration.** After every real write, the store regenerates the record's
  directory and each ancestor under the vault write lock, atomically and only
  if the bytes change. A no-op update regenerates nothing. An emptied directory
  loses its index. `rebuild_indexes()` is the repair path and how seed vaults
  get their indexes.
- **Crash semantics.** The record write and the index writes are separate
  atomic renames. A crash between them leaves a stale but valid index, repaired
  by the next real write in that directory or by `rebuild_indexes()`. An index
  write error after the record write returns `Err` with the record written.
  Atomic multi-file commits were rejected: the backend already offers
  single-record atomicity only (ADR 0001 amendment 6).
- **Generated stamps (§5.2).** With `generated_by` set, every real write stamps
  `generated: { by, at }` (UTC, second precision). Real means the document is
  dirty after the mutation and `type` stamping, so a no-op update stays a
  no-op. All write paths share one hook. The clock is injectable.
- **Actor validation (§7).** `generated_by` must be `<producer>/<version>` or
  `process:<id>`, checked at build time. `human:<id>` is rejected: the writer
  is a program, and OKF trust tiers read `human:` as a person, so a program
  claiming it would pose as a human author.
- **`generated` key.** While `generated_by` is set, a schema field whose
  effective key is `generated` is a build error (the generator owns it);
  with the option off the phase 1 warning stands.
- **`vault` entity name.** An entity whose snake-case name is `vault` is a build
  error, since its module would collide with the emitted `vault.rs`.
- **Demonstrator.** `examples/notes-kb` turns both knobs on and commits its
  index files; `tests/okf_conformance.rs` guards them against drift and runs the
  conformance checker over every example vault.
