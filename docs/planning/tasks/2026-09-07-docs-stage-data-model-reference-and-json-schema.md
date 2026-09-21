---
type: task
schema_version: '3'
status: in-progress
created: '2026-09-07'
last_reviewed: '2026-09-20'
impact: medium
complexity: medium
tags:
- docs
- schema
- open-format
related:
- 2026-09-07-schema-enums-labels-and-id-type.md
relevance_note: In review as #161. Spec ported from the dev monorepo, where it was authored as T-5IDQ.
---
# A docs stage that emits the data-model reference and JSON Schema from the parsed schema

## Goal

A consumer whose data model is meant to be an open format needs a published,
versioned spec. The schema already exists as Rust structs with doc comments and
`belongs_to` relations, and nothing renders it. The driving consumer worked
around this with a hand-written `build/docs.rs` that calls `ontogen::parse_schema`
and re-walks the source with `syn` for the doc comments — work that belongs in
the generator.

ontogen should carry a `docs` stage that emits a data-model reference
(per-entity tables, relations, enum values, a Mermaid ER diagram) and JSON
Schema files from the parsed schema.

## Today

- `crates/ontogen-core/src/model.rs` — `FieldDef` has no doc field; `EntityDef`
  has no doc field.
- `src/schema/parse.rs` — `parse_field` reads `#[ontology]` and `#[serde]`
  attributes and ignores `#[doc]`.
- `src/lib.rs` — `parse_schema` is `pub`, so a consumer's `build.rs` can call it
  directly. That is the shim path consumers take today.
- `src/pipeline.rs` — stages: seaorm, markdown_io, dtos, store, api, servers,
  clients. No docs stage.
- `src/persistence/seaorm/gen_entity.rs` — `field_db_type` is the column-type
  mapping the reference should print.
- `crates/ontogen-core/src/ir.rs` — `SchemaOutput { entities }`, plus `enums`
  once the schema-enums work lands.

## Proposed

- `FieldDef` and `EntityDef` gain `doc: String` (the joined `///` lines).
  `parse_field` and `parse_entity_struct` fill them.
- A new `src/docs/` module with

  ```text
  DocsConfig { markdown_output: PathBuf, json_schema_dir: PathBuf, title: String, format_version: String }
  ```

  and `gen_docs(&SchemaOutput, &DocsConfig)`.
- `data-model.md`: one section per entity with a table
  `field | type | column | required | doc`, a relations list (`belongs_to`
  targets and the entities that point back), enum values from
  `SchemaOutput.enums` where a field references one, and one Mermaid
  `erDiagram` over every entity.
- `<entity>.schema.json` (JSON Schema draft 2020-12) per entity: properties from
  the fields, `required` from non-optional fields, `enum` from resolved enums,
  `description` from the doc, `$id` from the entity name.
  `export.schema.json` describes the export bundle: a `manifest` object and one
  NDJSON table per entity, each item `$ref`-ing the entity schema.
- `Pipeline::docs(DocsConfig)` runs after `parse_schema` and is independent of
  the other stages.

## Approach

1. In `crates/ontogen-core/src/model.rs`, add `doc: String` to `FieldDef` and
   `EntityDef`. In `src/schema/parse.rs`, collect `#[doc = "..."]` attributes
   into it (trim one leading space per line, join with newlines). Update the
   constructors in `src/snapshots.rs` and `src/servers/tests.rs` fixtures.
2. Add `src/docs/mod.rs` (`DocsConfig`, `gen_docs`), `src/docs/markdown.rs` (the
   reference and the `erDiagram`), `src/docs/json_schema.rs` (entity schemas and
   `export.schema.json`). Type mapping: `String → string`,
   `Option<T> → T` not required, `i32/i64 → integer`, `f32/f64 → number`,
   `bool → boolean`, `Vec<String> → array of string`,
   `Vec<Struct> → array of object`, enum → `enum`.
3. Add `DocsStage` and `Pipeline::docs` in `src/pipeline.rs`; export
   `DocsConfig` from `src/lib.rs`.
4. Tests: an insta snapshot of `data-model.md` and one entity schema over the
   `comment_belongs_to_post_entity` fixture in `src/snapshots.rs`; a test that
   the emitted JSON parses and has `required` matching the non-optional fields.

## Files to touch

`crates/ontogen-core/src/model.rs`, `src/schema/parse.rs`, `src/docs/mod.rs`
(new), `src/docs/markdown.rs` (new), `src/docs/json_schema.rs` (new),
`src/pipeline.rs`, `src/lib.rs`, `src/snapshots.rs`, `src/servers/tests.rs`,
`CHANGELOG.md`.

## Acceptance criteria

- [ ] AC-1: Parsing a struct field with `/// Integer metres.` yields
      `FieldDef.doc == "Integer metres."`.
- [ ] AC-2: `gen_docs` over the `comment_belongs_to_post_entity` fixture writes
      `data-model.md` containing a `## Comment` section with a field table, a
      `belongs_to Post` line, and one `erDiagram` block that names both
      entities.
- [ ] AC-3: `gen_docs` writes `comment.schema.json` that `serde_json` parses,
      with `required` listing exactly the non-optional fields, and
      `export.schema.json` that references it by `$ref`.
- [ ] AC-4: An enum field appears in the markdown table with its values and in
      the JSON Schema as `enum`.

## Out of scope

- Any consumer's export bundle, its manifest and upgraders.
- Surfacing `data-model.md` through a host's docs contribution mechanism.

## Dependencies

- [Enum variants, field labels and the id type](./2026-09-07-schema-enums-labels-and-id-type.md)
  — enum values in the reference and `enum` in JSON Schema come from
  `SchemaOutput.enums`; the stage runs without it and prints string fields as
  strings.
