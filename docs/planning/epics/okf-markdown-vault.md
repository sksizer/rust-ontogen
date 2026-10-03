---
type: epic
schema_version: "1"
id: E0005
status: in-progress
title: OKF-conformant markdown vaults by default
created: 2026-10-03
last_reviewed: 2026-10-03
tags: [markdown-backend, store, okf, open-format]
---
# Epic — OKF-conformant markdown vaults by default

**Milestone:** M4 — Standard formats ([roadmap](../../roadmap.md))
**Status:** in progress — phase 1 shipped in https://github.com/sksizer/rust-ontogen/pull/194; phase 2 is next
**Spec:** [Open Knowledge Format 0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
(Google Cloud, June 2026) — a directory of markdown files with YAML
frontmatter, one required field (`type`), optional `title` / `description` /
`tags`, provenance (`generated`, `verified`, `sources`), lifecycle (`status`,
`stale_after`), reserved `index.md` / `log.md`, and plain markdown links as
the graph.
**Decisions:** [ADR 0005](../../architecture/0005-okf-markdown-vaults.md)  
**Builds on:** [ADR 0001](../../architecture/0001-markdown-as-store-backend.md),
[E0002](./markdown-backend.md)

## Does it fit?

Yes, as a thin profile over what the markdown backend already writes. The
spec is deliberately minimal and permissive, and the vault today is one
emitted field short of conformant. The two places it pulls against ADR 0001
are link syntax and a handful of reserved field names, both resolvable
without giving up Obsidian compatibility.

OKF conformance (§11) is three rules. Where the vault stands today
(citations against `crates/markdown-store` and
`src/persistence/markdown/gen_frontmatter.rs`):

| OKF rule | Today | Gap |
|---|---|---|
| Every non-reserved `.md` has parseable YAML frontmatter | Yes — `---` fence at byte 0, `serde_norway`, malformed YAML is an error (`frontmatter.rs:47-97`) | none |
| Every frontmatter block has a non-empty `type` | **No.** `EntityDef.type_name` exists in the IR and is documented as "the frontmatter `type:` discriminator" (`ir.rs:110-112`), but no markdown emitter writes or reads it (`gen_frontmatter.rs:130-141`) | emit it |
| `index.md` / `log.md` are reserved and follow their structures | **No.** The walk treats any `.md` as a record with id = file stem (`walk.rs:45`, `store.rs:220-243`); an `index.md` in a vault today would be read as entity `index` | reserve both ids and skip them in the walk |

Everything else OKF asks of a *consumer* the backend already does: unknown
keys are preserved on round-trip (`merge_serialize`, `frontmatter.rs:289-328`),
documents are never rejected for missing optional fields, broken wikilinks
are tolerated (ADR 0001 amendment 5), and there is no prescribed quoting
or key order.

## Gap analysis — the optional families

| OKF feature | Today | Fit | Proposed |
|---|---|---|---|
| `type` | Not written | Clean. `type_name` defaults to the entity's snake name (`parse.rs:146`), overridable via `#[ontology(entity, type_name = "…")]` | Write it first, by default, with the default flipped to the struct name (decision 1). Also makes `VaultLayout::Flat` finally meaningful — listing can filter on it |
| `title`, `description`, `tags` | Ordinary schema fields when the consumer declares them; `tags` in iron-log-md are wikilinks to a tag entity (`'[[strength]]'`) | Clean for `title`/`description`. `tags` as wikilink strings are tolerated but not what OKF means by "short strings" | No generator change; document that an OKF-shaped `tags` field is a `Vec<String>` of plain strings, and a relation to a tag entity is something else |
| Cross-links | `[[id]]` wikilinks in frontmatter for `belongs_to` and `many_to_many` (`wikilink.rs:24-26`); `has_many` never stored | **Tension.** OKF builds the graph from *markdown links in the body*, bundle-relative (`/tasks/foo.md`). Frontmatter wikilinks are "additional keys": preserved, but invisible to an OKF consumer | Keep wikilinks as the default encoding (ADR 0001 chose Obsidian deliberately; the SDLC corpus is written that way). Expose the graph to OKF consumers through generated `index.md` files (below), and offer `LinkEncoding::MarkdownPath` as an opt-in that renders relation values as `/<dir>/<id>.md` |
| `status` (`draft` \| `stable` \| `deprecated`, absent = `stable`) | Consumers already use `status` with their own vocabularies: tasks-tracker `closed/done`, the SDLC corpus `open/ready` | **Collision.** A consumer `status` with other values is not malformed, but an OKF reader will mis-derive lifecycle | Build-time warning when a schema field named `status` is not an enum whose variants are a subset of OKF's; a `#[ontology(frontmatter_name = "…")]` rename so the consumer can keep its own `status` under another key. Same reserved-name check for `generated`, `verified`, `sources`, `resource`, `stale_after`, `usage_window` |
| `generated: { by, at }` | No timestamps are stamped; `created_at` in fixtures is an ordinary string field | Clean, opt-in. Maps to OKF's actor convention `process:<app>/<version>` | `MarkdownIoConfig.okf.generated_by: Option<String>`; when set, the writer stamps `generated.by` and `generated.at` (ISO 8601 with `Z`) on every real write. Off by default: it dirties the file on every write and consumers with their own audit fields don't want two |
| `verified`, `sources`, `stale_after` | Hand-authored; preserved as unknown keys | Clean | Nothing to generate. Document that the writer preserves them |
| `index.md` per directory, root `index.md` with `okf_version: "0.2"` | None | Clean, opt-in. Lists `* [title](id.md) - description` per record, sorted by id (byte-stable, like the walk) | `okf.index: bool`; regenerated after every write and excluded from the walk. Off by default — Obsidian users keep their own index notes, and a derived file in git on every write is a cost the consumer should choose |
| `log.md` | None; "git is the WAL" (notes-kb README) | Clean, low value | Dropped (decision 4); the id `log` is still reserved so a hand-written `log.md` is never read as a record |
| Body conventions (`# Schema`, `# Examples`, footnote attribution) | Body is one opaque consumer-owned string | N/A | Nothing; OKF's headings are conventions for its own concept types |
| Timestamps "ISO 8601 with explicit UTC offset" | Not applicable to anything the writer stamps today | Clean | Applies only to `generated.at` |

## Goal

A vault written by the markdown backend is a conformant OKF 0.2 bundle out
of the box, and the OKF families that cost something (index files,
provenance stamps) are one config knob each.
Obsidian compatibility is not traded away for it.

## Scope

In scope: `src/persistence/markdown/gen_frontmatter.rs` (emit `type`),
`crates/markdown-store` (`layout.rs` id validation, `walk.rs` reserved
skips, a new index writer), `MarkdownIoConfig` and the markdown IR,
`src/persistence/markdown/okf.rs` (the reserved-name check, called from the markdown generator so SeaORM-only consumers never see it) and `src/schema/parse.rs` (`frontmatter_name`),
the golden fixtures under `tests/golden/markdown-backend/` and the four
example vaults, and the site pages `guides/markdown-backend`,
`guides/markdown-io`, `guides/schema-annotations`, `reference/configuration`.

Out of scope: OKF's Attested Computation type and its
`runtime`/`executor`/`attester` fields (nothing in ontogen executes), the
`references/` convention, and any change to the SeaORM backend.

## Phases

**Phase 1 — conformance by default (shipped in [#194](https://github.com/sksizer/rust-ontogen/pull/194)).** Flip the `type_name` default to the
struct name and emit `type: <type_name>` on every record; reject ids `index` and `log` at `validate_id`; skip the reserved
filenames in the walk; the reserved-field-name warning and
`#[ontology(frontmatter_name = "…")]`. Golden fixtures and example vaults
gain one line each. Breaking for byte-stable goldens, not for consumers:
an existing vault without `type` still reads, and gains `type` on its next
real write.

**Phase 2 — index files and provenance.** `okf.index` with the root
`okf_version` marker; `okf.generated_by` stamping. (The §11 conformance test
over the example vaults shipped in phase 1.)

**Phase 3 — docs.** The four site pages, the example READMEs, and a
one-page "ontogen vaults are OKF bundles" note that an OKF consumer can
read.

## Acceptance criteria

- Every example vault passes an OKF §11 conformance check in CI.
- `tests/golden_tree_guard.rs` and `tests/golden_conformance.rs` pass with
  updated goldens; `tests/backend_parity.rs` is untouched.
- A hand-authored vault with no `type` field reads unchanged; its records
  gain `type` only on a real write, and nothing else in the frontmatter
  changes beyond what the mutation-path normalization already does.
- A schema with a `status: String` field builds with one warning naming the
  OKF collision and the rename attribute; a schema using
  `frontmatter_name` builds clean.
- With `okf.index` on, every entity directory and the vault root carry a
  byte-stable `index.md`; neither is ever returned by `list`.
- With `okf.generated_by` set, a create and an update stamp `generated`;
  a no-op write does not touch the file.

## Decisions (2026-10-03)

Settled with the maintainer; [ADR 0005](../../architecture/0005-okf-markdown-vaults.md) records them.

1. **`type` casing.** The vault writes the entity struct name by default
   (`WorkoutSet`), overridable per entity via
   `#[ontology(entity, type_name = "…")]`. This changes the IR default for
   `type_name` from the snake name; nothing reads it today, so nothing
   breaks. Reads like OKF's own examples and matches the TS/Rust type.
2. **`index.md` is off by default**, one knob (`okf.index`) to enable.
   Revisit the default after T-Z0PE lands.
3. **Relation encoding stays `[[id]]` wikilinks.** `LinkEncoding::MarkdownPath`
   is an opt-in that renders `/<dir>/<id>.md`. The T-9NJO pilot decides
   whether the SDLC corpus flips it.
4. **Scope.** `log.md` is dropped: git history already is the change log,
   and the epic rated it low value. `LinkEncoding::MarkdownPath` is deferred
   to a follow-up task; phase 3 is removed from this epic.
5. **A `status` field with a non-OKF vocabulary is a build-time warning**,
   not an error; the consumer's vocabulary is not wrong, it is just not
   OKF's. `#[ontology(frontmatter_name = "…")]` is the fix path.

## Dependencies

- T-Z0PE (surgical per-key frontmatter rewrite) makes phase 1's "gains
  `type` on next write" land without normalizing the rest of a hand-authored
  record. Phase 1 can ship before it, with the normalization already
  documented in ADR 0001 amendment 6.
- T-9NJO (SDLC read-only pilot) is the first consumer whose corpus is both
  Obsidian-authored and a candidate OKF bundle; it is the right place to
  answer open questions 2 and 3.

## Tasks

Filed as each phase opens; one task per phase.

- [x] phase 1 — `type`, reserved ids, reserved-name warning, `frontmatter_name`, §11 conformance test ([#194](https://github.com/sksizer/rust-ontogen/pull/194))
- [ ] phase 2 — `index.md` writer, `okf_version`, `generated` stamping
- [ ] phase 3 — docs
