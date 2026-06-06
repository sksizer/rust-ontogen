# Golden spec — the markdown-backend consumer experience

Hand-authored **target artifacts** for ADR 0001's markdown store backend.
Nothing here compiles or runs yet; these files are the *spec* the
implementation PRs are graded against, and the user's edit surface — change
a golden and the conformance harness forces the generators to follow.

| File | Pins |
|---|---|
| `build.rs.golden` | The consumer wiring: `MarkdownIoOptions` (vault_root/layout/id_strategy/list_cap), `gen_markdown_io → MarkdownIoOutput`, `StoreConfig.backend: Backend::Markdown(md)`, the Pipeline builder shape + backend-inference rule, the hand-written `Store`/`AppError::Md` consumer contract |
| `store/note.rs.golden` | The generated-store **shape contract** for one minimal entity: method signatures, hook + `emit_change` sites (byte-identical to SeaORM emission), the `{Entity}Frontmatter` typed boundary, `create_record_derived` create path, RMW update, NotFound mapping |
| `vault/**.md.golden` | The on-disk record format: frontmatter field shapes, single-quoted wikilinks for `belongs_to`/m2m, block lists, body conventions |

Becomes executable in the conformance PR: `tests/golden_conformance.rs` runs
the real generators over a pilot schema and diffs (rustfmt-normalized)
against `store/*.golden`, and a seed-write against `vault/*.golden`. The
full multi-entity store goldens are expanded there **generate-then-review**
— hand-writing byte-exact emitter output for relation-heavy entities inverts
spec authority (the first mismatch would be a golden typo, not an emitter
bug); one minimal entity is the honest hand-authorable surface.

Design rationale and the frozen cross-backend contract live in
[`docs/markdown-backend-campaign.md`](../../../docs/markdown-backend-campaign.md).
Until the conformance PR lands, `tests/golden_tree_guard.rs` only asserts
this tree's existence so it can't be silently dropped.
