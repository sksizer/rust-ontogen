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
   `log` stays reserved. (Amended after phase 2: `log.md` was dropped as
   decided; `LinkEncoding::MarkdownPath` was deferred to a follow-up, the
   backlog item
   [B-OKFM](../planning/backlog/B-OKFM-okf-markdown-path-link-encoding.md);
   and the docs phase was renumbered to phase 3 and shipped.)
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
  stems in the walk, in the create rule `validate_id` and in the lookup
  check `validate_lookup_id`). This is deliberate, so that a
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
  `okf: OkfOptions { index, generated_by }`. `gen_markdown_io` emits
  `VAULT_ROOT` and `open_vault(root)` into the generated `mod.rs`, which builds
  the `VaultHandle` with the layout, list cap and OKF options from
  the config (the id strategy is the store's, passed to each create from
  `StoreConfig.id_strategy`). This makes the build configuration the single source for vault
  settings that every consumer previously repeated by hand in
  `VaultHandle::new(...)`, where they could drift from the build.
  `MarkdownIoOutput` carries only what the store emitter reads; the store
  emitter needs none of the vault settings and the vault constructor is their
  one consumer, so `open_vault` reads the config directly. `open_vault` emits
  `.with_okf(..)` only when an option is on. A relative `vault_root` resolves
  against the program's working directory.
- **Runtime seam.** The extractable `markdown-store` crate sees one struct,
  `OkfPolicy { index, generated_by, clock }` (default: off, off,
  `SystemTime::now`), set with `VaultHandle::with_okf` and read with `okf()`.
  It knows nothing of the build-time `OkfOptions`.
- **Index files (§8).** The root and every directory holding records carry an
  `index.md`: type-grouped `# <type>` sections sorted by name, then `# Untyped`,
  then `# Directories`; the root declares `okf_version: "0.2"`. The format is
  byte-stable so regeneration can skip unchanged files. `# Untyped` and
  `# Directories` are the store's own headings: a record type equal to either
  (or either followed by ` (type)` suffixes) is headed with ` (type)` appended.
  In titles, id fallbacks, descriptions, type headings and directory link text
  only `` \ ` * _ [ ] < & # ~ $ % = ^ `` are backslash-escaped (CommonMark
  code, emphasis, brackets, HTML, autolinks, entities and closing `#`, plus
  Obsidian tags, `~~`, `$`, `%%`, `==`, `^`); other punctuation is written as
  is. Link URLs stay percent-encoded. A record that cannot be read or parsed is
  listed under Untyped, titled by its id.
- **Regeneration.** After every real write, the store regenerates the record's
  directory and each ancestor under the vault write lock, atomically and only
  if the bytes change. A no-op update regenerates nothing. A write parses the
  records directly in each directory on the path to the root (under Flat, every
  record in the vault); subdirectory probes stop at the first record.
  `rebuild_indexes()` is the repair path and how seed vaults get their indexes.
- **Ownership.** With `index` on, the store overwrites `index.md` in every
  directory holding records, including a hand-written one. It removes an
  `index.md` from a record-less directory (in `rebuild_indexes()`, or when a
  write empties the directory) only when the file has exactly the store's own
  shape (optional root `okf_version`-only frontmatter, then `# heading`
  sections of `* [text](link)` entries with an optional ` - description`,
  linking record files or `dir/`) and every link in it is dangling (record
  file missing, or `dir/` missing or without records). Any other `index.md`,
  including a hand-written one whose links resolve even if store-shaped, is
  left alone.
- **Failure semantics.** The record write and the index writes are separate
  atomic renames. The index refresh is non-fatal: `create`, `modify` and
  `remove` return `Ok` once the record is committed. A failed refresh marks the
  directory stale, exposed by `VaultHandle::stale_indexes()` (sorted `index.md`
  paths, in memory, shared by clones); a later successful refresh of that
  directory or a successful `rebuild_indexes()` clears it. `rebuild_indexes()`
  itself returns `Err` on the first index it cannot write, being an explicit
  request. A crash between the record rename and the index rename is not
  recorded: the index is stale but valid until the next real write there or a
  rebuild. Atomic multi-file commits were rejected: the backend already offers
  single-record atomicity only (ADR 0001 amendment 6).
- **Generated stamps (§5.2).** With `generated_by` set, every real write stamps
  `generated: { by, at }` (UTC, second precision). Real means the document is
  dirty after the mutation and `type` stamping, so a no-op update stays a
  no-op. All write paths share one hook. The clock is injectable through `OkfPolicy`.
- **Actor validation (§7).** `generated_by` must be `<producer>/<version>` or
  `process:<id>`, checked at build time. `human:<id>` is rejected: the writer
  is a program, and OKF trust tiers read `human:` as a person, so a program
  claiming it would pose as a human author.
- **`generated` key.** While `generated_by` is set, a schema field whose
  effective key is `generated` is a build error (the generator owns it);
  with the option off the phase 1 warning stands.
- **Demonstrator.** `examples/notes-kb` turns both knobs on and commits its
  index files; `tests/okf_conformance.rs` guards them against drift and runs the
  conformance checker over every example vault.
