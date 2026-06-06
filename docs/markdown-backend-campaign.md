# Design record — ADR 0001 implementation campaign (markdown store backend)

The checked-in design record for the stacked-PR campaign implementing
[ADR 0001](architecture/0001-markdown-as-store-backend.md), in the spirit of
OF-015's design pass. The campaign is the implementation vehicle for task
**T-YO00** (PR #96's SDLC integration program); the contract surfaces live as
code in the two showcase PRs (the `markdown-store` crate, and
`tests/golden/markdown-backend/`) — this note records the *decisions* and
their rationale so the accepted ADR can be amended honestly at campaign end.

## The stack

`main → 00 preflight → 01 markdown-store (showcase A) → 02 goldens+this note
(showcase B) → 03 store-backends lift → 04 core IR → 05 breaking cutover →
06 wikilink policy → 07 persistence retarget → 08 markdown emitter + pilot →
09 conformance + parity → {10 iron-log-md, 11 tasks-tracker, 12 notes-kb} →
13 site docs → 14 ADR amendment + accept + shipping log.`

Edits to the showcases propagate mechanically: goldens become conformance
assertions (09); the emitter (08) is adjusted until green.

## Contract freeze (what every later PR conforms to)

1. **Backend choice is generation-time**: `StoreConfig.backend:
   Backend::Seaorm(Option<SeaOrmOutput>) | Backend::Markdown(MarkdownIoOutput)`
   — **owned, no lifetime** (the ADR's `Backend::Markdown(&md)` borrow would
   force a viral `<'a>` through `StoreConfig`/`Pipeline`; deviation flagged
   for the ADR amendment).
2. **No `Store` trait exists or will be generated.** Backends are unified by
   emitting identical *inherent* `impl Store` blocks against a
   consumer-supplied struct exposing `db()` (SeaORM) / `vault()` (markdown).
   The ADR's `gen_store_trait()` / `impl Store for {Backend}Store` language
   is superseded (amendment item). Nothing in the contract needs a trait:
   downstream layers never name the backend type.
3. **Byte-identical downstream**: `gen_api`/`gen_servers`/`gen_clients`
   output is byte-identical across backends for the same schema — enforced
   by `tests/backend_parity.rs` (09) with a committed negative control.
   `StoreMethodMeta` collection never branches on backend.
4. **Hook lifecycle and `emit_change` points fire identically** on both
   backends, with identical signatures.
5. **Markdown consumer contract** (the whole delta vs SeaORM):
   `Store { vault: markdown_store::VaultHandle, change_tx }` + `vault()`
   accessor; `AppError::Md(String)` + `From<markdown_store::Error>`;
   `{Entity}NotFound` reused; `DbError` never referenced; **no**
   `sync_junction`/`load_junction_ids`.
6. **`id: String` everywhere** (audited clean — `docs/id-string-constraint.md`),
   enforced at the runtime's validated path boundary (no separators, no `:`
   — a Windows drive prefix escapes the root via `Path::join` —, no dot
   paths, no hidden/trailing-dot-or-space stems).
7. **List semantics**: lexicographic by id (extension-stripped path sort —
   raw path order diverges at suffix boundaries), in-memory
   `skip(offset).take(limit)`, loud `list_cap` (default 10k). **No `OrderBy`
   parameter**: ADR contract item 3 promises one, item 5 forbids the
   signature change that would deliver it; item 5 wins (amendment item).
8. **Atomicity**: single-record only — same-dir tempfile + fsync + rename.
   No multi-record transactions; no batch ops in v1, so the ADR's
   `AtomicityError`-vs-`BatchPartialFailure` naming conflict is resolved by
   reserving both as conventions and shipping neither (amendment item).
9. **Byte-stability over hand-authored corpora** (program requirement
   T-Z0PE, folded in mid-campaign): a parsed `Document` renders its original
   source **byte-for-byte while semantically untouched**; all mutators are
   change-aware, so no-op updates are zero-diff writes. A real mutation
   currently re-emits the whole frontmatter block (the explicit interim
   normalization whitelist; YAML comments in that block are lost). Narrowing
   to surgical per-key rewrites is scheduled with the fidelity harness —
   release-blocking for the data plane, per T-Z0PE.
10. **Wikilink convention**: relation fields store `[[id]]` (emitter
    single-quotes them; Obsidian parses either quoting). Stripping happens
    at the generated `{Entity}Frontmatter` typed boundary — one place, both
    directions — and becomes **markdown-only**: the SeaORM DTO `From` impls
    go passthrough (the strip_wikilink wart fix, with iron-log's no-op stubs
    deleted).
11. **m2m**: authoritative-side wikilink list in frontmatter; no junction
    sync. Reverse/derived views and delete-time cascade link-cleanup are
    **deferred** (amendment items; the ADR's "if configured" cascade has no
    config in v1).
12. **YAML stack**: `serde_norway` (maintained serde_yaml fork).
    markdown-vault's `serde_yml`/`libyml` are archived and RUSTSEC-flagged
    unsound (2025-0068/-0067, serializer segfault path, no patch); the
    crates-merge alignment flips direction — markdown-vault migrates.

## Crate extraction survey (user ask: "other general functional crates")

| Candidate | Verdict |
|---|---|
| Frontmatter/wikilink/vault runtime | **Extracted this campaign** → `crates/markdown-store` (neutral name, zero ontogen deps, destined for rust-markdown) |
| Naming utils (`to_snake_case`, pluralize…) | Already extracted → `ontogen-core::naming`; no further motion |
| `rustfmt_string` / build-utils | Stay in `ontogen-core::utils` — generator-side only, no external consumer |
| Rust→TS emitter | Already extracted → `ontogen-ts` |
| Schema parse front-end | Tracked by `docs/crate-extraction.md` Phase 2 (`ontogen-schema`); unchanged by this campaign; ADR-0002 (T-66TG) may force it |
| JSON Canvas bridge (entity graph → `.canvas`) | Deferred with a follow-up ticket in the shipping log — natural `jsoncanvas` consumer, zero current demand |

## Program alignment (PR #96 / dev D-DX1Q)

- **T-YO00** = this campaign.
- **T-Z0PE** (zero-diff fidelity harness over the live SDLC corpus): the
  byte-stability property above is its substrate; the harness itself lands
  with/after conformance (09), pointed at the real corpus, with surgical
  per-key rewriting as the gating follow-up for the *mutation* path.
- **T-9NJO** (read-only pilot over the real corpus via the ADR-0002
  JSON-Schema front-end): out of campaign scope; the campaign ships its
  substrate (CI-compiled pilot crate, HTTP+MCP tasks-tracker example,
  parity gate).

## ADR amendment checklist (PR 14 — mandatory, not optional)

- [ ] Rewrite contract item 3: lexicographic-by-id order; `OrderBy`
      deliberately absent (conflicts with item 5); future ADR if needed.
- [ ] Supersede the Store-trait / `gen_store_trait()` language with the
      inherent-impl + `db()`/`vault()` reality.
- [ ] `Backend` is owned (no `&md` borrow).
- [ ] Resolve `AtomicityError` vs `BatchPartialFailure` → `AppError::Md` +
      reserved names; no batch ops shipped.
- [ ] Mark cascade link-cleanup and m2m derived views **deferred** (the "if
      configured" cascade has no config).
- [ ] Record the byte-stability guarantee (T-Z0PE) the ADR never asked for.
- [ ] Fix the dangling "Out of scope" self-references; strike the
      revisit-cadence note; record the id-audit closing affirmatively.
