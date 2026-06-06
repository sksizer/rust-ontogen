# markdown-store

Typed, lossless YAML-frontmatter markdown storage — treat a folder of
`---`-fenced markdown files as a datastore without ever destroying what a
human wrote in them.

This is the runtime layer beneath ontogen's markdown store backend
([ADR 0001](../../docs/architecture/0001-markdown-as-store-backend.md)):
generated CRUD code calls into this crate instead of having helper code
emitted into every consumer. It is deliberately **schema-agnostic and free of
ontogen dependencies** — usable as a plain library.

## The core idea: a lossless round-trip

```rust
use markdown_store::{Document, IdStrategy, VaultHandle, VaultLayout};

let vault = VaultHandle::new("docs/data", VaultLayout::PerEntityDir, IdStrategy::Provided);

// Files are parsed into an order-preserving frontmatter mapping + verbatim body.
vault.modify_record("tasks", "t-1", |doc| {
    let mut task: MyTask = doc.deserialize()?;       // typed view of the keys you model
    task.status = "closed".into();
    doc.merge_serialize(&task, &["title", "status"]) // fold back ONLY the keys you own
})?;
// Keys a human added by hand, key order, and the markdown body all survive.
```

`Document::merge_serialize(&value, owned_keys)` is the heart: it overwrites
the keys your type owns (removing owned keys your value no longer emits, so a
cleared `Option` doesn't leave a stale line) and leaves every other key
untouched. That property is what makes it safe to point generated CRUD code
at a vault people also edit in Obsidian or a text editor.

## What's in the box

| Module | Purpose |
|---|---|
| `frontmatter` | `split` (fence parsing, byte-compatible with `markdown-vault`), `Document` round-trip model |
| `wikilink` | `[[id]]` encode / strip / parse (Obsidian-compatible; strip is idempotent) |
| `layout`, `id` | id ↔ filename mapping with **validated, traversal-proof** path construction; `IdStrategy` (`Provided` / `SlugFromField` / `Uuid`*) |
| `fsops` | atomic single-file write (same-dir tempfile + fsync + rename), read-modify-write |
| `walk` | gitignore-aware record listing, **sorted** (the backend's stable order) |
| `store` | `VaultHandle`: create / read / modify / remove / list / id-derivation, with a shared intra-process write lock |

\* `Uuid` needs the `uuid` cargo feature.

Feature flags: `frontmatter`/`wikilink`/`layout`/`id` are always on;
`fsops`, `walk`, and `store` (default) gate the I/O layers and their deps
(`tempfile`, `ignore`).

## Guarantees and limits (read this before depending on it)

- **Single-record atomicity.** A write lands via same-directory tempfile +
  fsync + rename: readers see old-or-new, never torn. There are **no
  multi-record transactions** — by design (see the ADR).
- **Stable list order**: lexicographic by filename (= record id).
- **Single-process stance.** Clones of a `VaultHandle` share a write lock,
  so concurrent tasks in one process serialize; concurrent writers in other
  processes are out of scope.
- **Creation never overwrites** (`create_record` → `AlreadyExists`), and
  **ids cannot escape the vault** (path construction validates ids and
  segments — no separators, no `..`, no hidden-file stems).
- **Meaning-lossless, not byte-lossless.** Unknown keys, key order, and the
  body survive rewrites; YAML cosmetics (quoting style, list layout) are
  normalized to the emitter's deterministic output.
- **Scale ceiling.** Listing parses every record; the configurable list cap
  (default 10k) turns overgrowth into a loud error. This crate is for
  small-N, human-editable, read-heavy data — not a database.
- **Strict reads.** A malformed record fails the operation; it is never
  silently skipped (and read-modify-write refuses to rewrite a file it
  couldn't parse).

## Examples

Each example exercises one API group end-to-end (all self-asserting):

```sh
cargo run -p markdown-store --example roundtrip   # Document round-trip, hand-edit preservation
cargo run -p markdown-store --example wikilinks   # encode/strip/parse
cargo run -p markdown-store --example vault_crud  # full CRUD + pagination + cap guard
cargo run -p markdown-store --example relations   # belongs_to / has_many walk / m2m lists
```

`tests/generated_call_pattern.rs` rehearses the exact per-operation call
shape ontogen's markdown backend generates — if you want to know what
generated code will look like against this API, read that file.

## Relationship to rust-markdown / markdown-vault

This crate is the **write/round-trip complement** to
[`markdown-vault`](https://github.com/sksizer/rust-markdown) (read-only tag
extraction) and is expected to migrate into that workspace once stable:

- the fence-splitting contract is kept byte-compatible with
  markdown-vault's `frontmatter::split` (its edge-case tests are mirrored
  here case for case);
- the YAML stack matches (`serde_yml 0.0.12`) so the eventual merge carries
  one YAML dependency;
- tag extraction is deliberately **not** duplicated here — compose the two
  crates (the ontogen notes-kb example does exactly that).

The crate is currently `publish = false` and excluded from release-plz;
publishing is a deliberate later step.
