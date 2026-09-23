---
type: task
schema_version: '3'
status: backlog
created: '2026-09-07'
last_reviewed: '2026-09-20'
impact: low
complexity: large
tags:
- schema
- persistence
- seaorm
related:
- 2026-09-07-docs-stage-data-model-reference-and-json-schema.md
relevance_note: No implementation started. Spec ported from the dev monorepo, where it was authored as T-SKHA.
---
# Bytes, timestamp, date and JSON columns, defaults, and declared indexes

## Goal

The field vocabulary is `String`, `Option<String>`, `Option<Enum>`,
`Vec<String>`, `Vec<Struct>`, integers, floats and `bool`. A realistic domain
model needs more: a binary column for encoded sensor streams (today stored
base64 in a `String`, a third larger), typed timestamps and dates, a JSON
column, column defaults, and declared indexes and unique constraints — all of
which consumers hand-write in every migration today.

The schema should state these once and the generators should carry them into
SeaORM, DTOs, TS and the admin registry.

## Today

- `crates/ontogen-core/src/model.rs` — `FieldType` has no bytes, datetime, date
  or json; `FieldDef.default_value` is a markdown-rendering hint, not a column
  default.
- `src/schema/parse.rs` — `classify_type` maps unknown types to
  `FieldType::Other`; `parse_field_ontology_attrs` knows `id`, `body`,
  `enum_field`, `relation`, `skip`.
- `src/persistence/seaorm/gen_entity.rs` — `field_db_type` maps to
  `String`/`i32`/`f64`/`bool`; no `Vec<u8>`, no `DateTimeUtc`, no `Json`.
- `src/persistence/seaorm/gen_conversion.rs`, `src/persistence/dto.rs`,
  `src/store/gen_update.rs`, `src/persistence/markdown/gen_frontmatter.rs` —
  every exhaustive `FieldType` match.
- `crates/ontogen-ts/src/emit.rs` — the Rust-to-TS type map (`u8..f64 → number`);
  no bytes mapping.
- `src/clients/generators/admin.rs` — `classify_admin_field` for the registry
  field types.
- `packages/admin-types/index.ts` — the `FieldType` union for the admin UI.

## Proposed

**Types.** `Vec<u8>` → `FieldType::Bytes` (SeaORM `Vec<u8>`, column BLOB, TS
`string` base64 over the wire with a serde `base64` adapter in the DTO, admin
`'bytes'` read-only size display). `#[ontology(timestamp)] String` and
`#[ontology(date)] String` → `FieldType::Timestamp`/`Date` (column stays TEXT
ISO8601; DTO and TS stay `string`; the admin registry gets
`'timestamp'`/`'date'` so the layer can render a date input).
`#[ontology(json)] serde_json::Value` → `FieldType::Json` (column TEXT, DTO
`serde_json::Value`, TS `unknown`).

**Defaults.** `#[ontology(default = "strength")]` (string literal, number or
bool) → SeaORM `#[sea_orm(default_value = ...)]`, DTO `#[serde(default = ...)]`,
and a `ColumnMeta.default` for migrations.

**Indexes.** `#[ontology(index)]` and `#[ontology(unique)]` on a field;
`#[ontology(entity, index("workout_id", "channel"), unique("system", "external_id", "subject_kind"))]`
on the struct. Emitted as `EntityTableMeta.indexes: Vec<IndexMeta { columns, unique }>`
and as a generated `pub fn indexes() -> &'static [IndexDef]` per entity module,
so a migration can loop over them instead of hand-writing SQL.

## Approach

1. In `crates/ontogen-core/src/model.rs`, add
   `FieldType::{Bytes, OptionBytes, Timestamp, OptionTimestamp, Date, OptionDate, Json, OptionJson}`,
   `FieldDef.column_default: Option<Literal>`, `FieldDef.index: Option<IndexKind>`
   (`Plain | Unique`), and `EntityDef.indexes: Vec<IndexDef>`. Add `IndexMeta`
   to `crates/ontogen-core/src/ir.rs`.
2. In `src/schema/parse.rs`, parse the new attributes in
   `parse_field_ontology_attrs` and `parse_struct_ontology_attrs`; classify
   `Vec<u8>` and `serde_json::Value` in `classify_type`; reject
   `timestamp`/`date` on a non-`String` field with an error naming the field.
3. Extend every exhaustive match: `src/persistence/seaorm/gen_entity.rs`
   (`field_db_type`, `generate_model_column` with `default_value`, the
   `indexes()` fn), `gen_conversion.rs`, `src/persistence/dto.rs` (base64
   adapter for bytes), `src/store/gen_update.rs`,
   `src/persistence/markdown/gen_frontmatter.rs` (bytes render as base64),
   `src/clients/generators/admin.rs`, `crates/ontogen-ts/src/emit.rs`.
4. Widen `packages/admin-types/index.ts` `FieldType` with
   `'bytes' | 'timestamp' | 'date' | 'json'`; render them in
   `packages/nuxt_admin_layer/app/components/AdminFormField.vue` and
   `app/utils/formatFieldValue.ts` (bytes read-only with a byte count,
   timestamp/date as `<input type="datetime-local">`/`date`, json as a
   textarea).
5. Tests: parse fixtures for each attribute; insta snapshots in
   `src/snapshots.rs` for an entity with a bytes column, a default and a
   composite unique index; `tests/ts_entity_field_type_closure.rs` covers the TS
   mapping.

## Files to touch

`crates/ontogen-core/src/model.rs`, `crates/ontogen-core/src/ir.rs`,
`src/schema/parse.rs`, `src/persistence/seaorm/gen_entity.rs`,
`src/persistence/seaorm/gen_conversion.rs`, `src/persistence/dto.rs`,
`src/store/gen_update.rs`, `src/persistence/markdown/gen_frontmatter.rs`,
`src/clients/generators/admin.rs`, `crates/ontogen-ts/src/emit.rs`,
`packages/admin-types/index.ts`,
`packages/nuxt_admin_layer/app/components/AdminFormField.vue`,
`packages/nuxt_admin_layer/app/utils/formatFieldValue.ts`, `src/snapshots.rs`,
`tests/ts_entity_field_type_closure.rs`, `CHANGELOG.md`.

## Acceptance criteria

- [ ] AC-1: An entity with `pub data: Vec<u8>` generates a SeaORM model column
      `pub data: Vec<u8>`, a DTO that serialises it as base64, and a TS binding
      typed `string`.
- [ ] AC-2: `#[ontology(default = "strength")] pub activity_kind_id: String`
      generates `#[sea_orm(default_value = "strength")]` on the model column and
      `ColumnMeta.default == Some("strength")`.
- [ ] AC-3: `#[ontology(entity, unique("system", "external_id", "subject_kind"))]`
      generates an `indexes()` fn whose one entry has those three columns and
      `unique: true`.
- [ ] AC-4: `#[ontology(timestamp)]` on a non-`String` field fails
      `parse_schema` with an error naming the field; on a `String` field the
      registry emits `type: 'timestamp'`.

## Out of scope

- A migration generator; `indexes()` is data for a hand-written migration to
  loop over.
- Decimal columns.
