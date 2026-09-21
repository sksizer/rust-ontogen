---
type: task
schema_version: '3'
status: in-progress
created: '2026-09-07'
last_reviewed: '2026-09-20'
impact: medium
complexity: medium
tags:
- schema
- clients
- admin
related:
- 2026-09-07-docs-stage-data-model-reference-and-json-schema.md
- 2026-09-07-admin-layer-registry-path-pickers-and-packaging.md
relevance_note: In review as #160. Spec ported from the dev monorepo, where it was authored as T-DM4X.
---
# Enum variants, field labels and the id type reach the admin registry

## Goal

A consumer's schema writes its closed sets as `a | b | c` in doc comments,
because the parser reads only `ItemStruct`. The admin registry therefore cannot
know the values, renders the field as free text, and derives labels by naive
capitalisation. The id type is a literal `'string'` whatever the id field is.

The parser should read the enums declared beside the entities, emit
`enumValues` and readable labels into `admin-registry.ts`, and set `idType`
from the id field.

## Today

- `src/schema/parse.rs` — `parse_schema_source` walks `syn::File` items and
  handles `ItemStruct` only; `classify_type` maps `Option<T>` with an unknown
  `T` to `FieldType::OptionEnum(T)` but never sees the variants.
- `crates/ontogen-core/src/model.rs` —
  `FieldDef { name, field_type, role, serde_default, multiline_list, default_value }`;
  `FieldType::OptionEnum(String)`; `FieldRole::EnumField`.
- `crates/ontogen-core/src/ir.rs` — `SchemaOutput { entities }`.
- `src/clients/generators/admin.rs` — `classify_admin_field` maps `OptionEnum`
  to `'enum'` with no `enumValues`; `field_label` splits snake_case and
  capitalises the first word; `idType: 'string'` is a literal.
- `packages/admin-types/index.ts` — `AdminFieldDef.enumValues?: string[]`;
  `AdminEntityConfig.idType: 'string'`.
- `packages/nuxt_admin_layer/app/components/AdminFormField.vue` — already
  renders a `<select>` over `field.enumValues` for `type === 'enum'`.

## Proposed

- `parse_schema_source` also parses `ItemEnum` items into
  `EnumDef { name, variants: Vec<EnumVariant { name, serde_rename }> }`,
  honouring `#[serde(rename_all = "...")]` and per-variant `#[serde(rename)]`.
  `SchemaOutput` gains `enums: Vec<EnumDef>`.
- A field whose type is a parsed enum (`Option<Kind>` or `Kind`) resolves to
  `FieldType::OptionEnum(name)` / a new `FieldType::Enum(name)`; `FieldDef`
  gains `enum_ref: Option<String>`.
- `admin.rs` emits `enumValues: [...]` from the resolved enum's serialised
  variant names.
- Labels: `field_label` title-cases every word (`avg_hr_bpm` → `Avg Hr Bpm`) and
  `ClientsConfig` gains `label_overrides: HashMap<String, String>` keyed
  `entity.field` or `field` (`"avg_hr_bpm" → "Average HR (bpm)"`).
- `idType` is derived from the id field's `FieldType` (`'string'` or
  `'number'`); `admin-types` widens `idType` to `'string' | 'number'`.
- Doc comments are not parsed here — that is the
  [docs stage](./2026-09-07-docs-stage-data-model-reference-and-json-schema.md).

## Approach

1. In `crates/ontogen-core/src/model.rs`, add `EnumDef`, `EnumVariant`,
   `FieldType::Enum(String)` and `FieldDef.enum_ref`. Update every exhaustive
   match on `FieldType` (`src/persistence/seaorm/gen_entity.rs`,
   `src/persistence/dto.rs`, `src/persistence/markdown/gen_frontmatter.rs`,
   `src/store/gen_update.rs`, `src/clients/generators/admin.rs`): a
   non-optional enum is a `String` column.
2. In `src/schema/parse.rs`, collect `ItemEnum` items per file, then resolve
   field types against the collected enum names in a second pass so an entity
   can reference an enum declared in another schema file. Put `enums` on
   `SchemaOutput` in `crates/ontogen-core/src/ir.rs`.
3. In `src/clients/generators/admin.rs`, emit `enumValues`, the title-cased
   label with overrides, and the derived `idType`. Add `label_overrides` to
   `ClientsConfig` in `src/lib.rs` and to the internal client config.
4. In `packages/admin-types/index.ts`, widen `idType`.
5. Tests: `src/schema/parse.rs` parses an enum with `rename_all = "kebab-case"`
   and a struct using it; `src/clients/tests.rs` asserts
   `enumValues: ['peer-reviewed', ...]`, a label override, and
   `idType: 'number'` for an `i64` id. Snapshot the registry.

## Files to touch

`crates/ontogen-core/src/model.rs`, `crates/ontogen-core/src/ir.rs`,
`src/schema/parse.rs`, `src/schema/mod.rs`, `src/lib.rs`,
`src/persistence/seaorm/gen_entity.rs`, `src/persistence/dto.rs`,
`src/persistence/markdown/gen_frontmatter.rs`, `src/store/gen_update.rs`,
`src/clients/config.rs`, `src/clients/generators/admin.rs`,
`src/clients/tests.rs`, `packages/admin-types/index.ts`, `CHANGELOG.md`.

## Acceptance criteria

- [ ] AC-1: Parsing a schema file with `pub enum Quality { PeerReviewed, Community }`
      under `#[serde(rename_all = "kebab-case")]` and a struct field
      `quality: Option<Quality>` yields `SchemaOutput.enums` of length 1 and the
      field's `enum_ref == Some("Quality")`.
- [ ] AC-2: The generated `admin-registry.ts` for that schema contains
      `enumValues: ['peer-reviewed', 'community']` on the `quality` field.
- [ ] AC-3: With `label_overrides` containing `("avg_hr_bpm", "Average HR (bpm)")`,
      the registry emits that label; without it, the field is labelled
      `Avg Hr Bpm`.
- [ ] AC-4: An entity with `#[ontology(id)] pub id: i64` emits
      `idType: 'number'`; the existing string-id entities still emit
      `idType: 'string'`.

## Out of scope

- Carrying doc comments into `FieldDef` — the docs stage.
- Converting any consumer's string fields to Rust enums.
