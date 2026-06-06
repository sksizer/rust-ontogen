# Golden spec — the markdown-backend consumer experience

Hand-authored **target artifacts** for ADR 0001's markdown store backend.
Nothing here compiles or runs yet; these files are the *spec* the
implementation PRs are graded against, and the user's edit surface — change
a golden and the conformance harness forces the generators to follow.

| File | Pins |
|---|---|
| `build.rs.golden` | The consumer wiring: `MarkdownIoOptions` (vault_root/layout/id_strategy/list_cap), `gen_markdown_io → MarkdownIoOutput`, `StoreConfig.backend: Backend::Markdown(md)`, the Pipeline builder shape + backend-inference rule, the hand-written `Store`/`AppError::Md` consumer contract |
| `store/note.rs.golden` | The generated-store **shape contract** for one minimal entity (see below) |
| `vault/**.md.golden` | The on-disk record format, in two deliberate roles (see below) |

## The store golden (`store/note.rs.golden`)

The module the markdown emitter must generate for the minimal entity

```rust
#[ontology(directory = "notes")]
pub struct Note {
    #[ontology(id)]
    pub id: String,
    pub title: String,
    #[ontology(body)]
    pub body: String,
}
```

Contract pinned by it, beyond the literal text:

- **Method signatures, hook call sites, and `emit_change` points are
  byte-identical to the SeaORM emission** — the downstream byte-identical
  invariant depends on it. The shared sections (`{Entity}Update` struct,
  `apply()`, the DTO `From` impls) are the **exact `gen_update.rs` emission**,
  unmodified: derive set, doc comment, private `fn apply`, `clone_from`,
  and `id: input.id` passthrough in `From<CreateInput>`.
- **Create-with-derived-id contract**: `CreateInput.id` stays `String`
  (DTO output is backend-identical); an **empty id means "derive"** — the
  generated create filters empty and passes `None` to
  `create_record_derived`, which derives + dedups + writes under one lock.
  Whether Create DTOs additionally gain `#[serde(default)]` on `id` (so
  JSON clients can omit the field on both backends) is a declared shared
  decision for the wikilink-policy PR.
- Frontmatter access goes through the generated `{Entity}Frontmatter`
  newtype (markdown-io generated module); wikilink encode/strip lives
  there, never inline in CRUD bodies. `body` never routes through
  frontmatter — it flows `From<CreateInput>` → entity → `doc.set_body`.
- Update is read-modify-write inside `modify_record`; a **no-op update is
  literally no write** (the runtime skips clean documents — no tempfile,
  no mtime/inode churn).
- **Hook value parity for relation entities** (not visible in the
  relationless Note, but contractual): on entities with `has_many`/derived
  relations, `get` and the `current` passed to `before_update` must be
  populated via the reverse walk **before** the hook fires, exactly as the
  SeaORM emission populates relations before its hooks — signature parity
  AND value parity.

### Conformance normalization (exact, no surprises)

`tests/golden_conformance.rs` (conformance PR) compares
`rustfmt(generated)` to the golden **as committed**, where rustfmt runs
with the repo `rustfmt.toml` and the pilot consumer's edition (**2024** —
generated output is formatted with the consuming crate's edition by
`write_and_format`). The goldens are therefore kept in edition-2024
rustfmt-normal form, with **no banner or non-emitter content** — the
committed bytes are the assertion, nothing is stripped at test time.

## The vault goldens (two roles)

- **`vault/tasks/ship-the-emitter.md.golden`**, `vault/notes/…`,
  `vault/epics/…` — the **hand-authored parse exemplars**: quoting variety
  a human/Obsidian writes (e.g. `created: '2026-06-06'` quoted). Validated
  by `tests/golden_tree_guard.rs`: they must parse with the real
  `markdown-store` crate and **round-trip byte-for-byte** (the
  byte-stability contract).
- **`vault/tasks/seeded-by-writer.md.golden`** — the **typed-write
  exemplar**: what generated create/update emits for the same shapes
  (date-like strings unquoted, wikilinks single-quoted, block lists). The
  conformance seed-write check builds these records through the typed path
  and diffs against this file; hand-authored cosmetics are *not* expected
  to match this role.

Design rationale and the frozen cross-backend contract:
[`docs/markdown-backend-campaign.md`](../../../docs/markdown-backend-campaign.md).
