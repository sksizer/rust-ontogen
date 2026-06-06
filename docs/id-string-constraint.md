# Design note — the `id: String` cross-backend constraint

Status: checked in ahead of ADR 0001 implementation work, as required by that
ADR's Notes section ("The `id: String` constraint deserves its own checked-in
design note before implementation begins").

## The constraint

ADR 0001's cross-backend semantic contract pins item 1: **`id` is the primary
key, always a `String`**, on every entity that flows through `gen_store()`,
regardless of backend.

The markdown backend forces this: a record's id *is* its filename stem
(`<vault>/<entity-dir>/<id>.md`). Integer ids would produce meaningless
filenames (`7.md`), break slug-based workflows, and collide with the
wikilink citation format (`epic: "[[E0042]]"`), which is string-shaped by
construction. Rather than let backends disagree about key types — which would
leak into every generated API/server/client signature and break the
byte-identical-downstream invariant (contract item 5) — the contract narrows
both backends to the type the weaker backend can support.

## Today — audit of all in-tree schema definitions

| Schema source | Entities | Id field | Verdict |
|---|---|---|---|
| `examples/iron-log/src-tauri/src/schema/` | Workout, Exercise, WorkoutSet, Tag | `pub id: String` (all four, `#[ontology(id)]`) | compliant |
| `tests/fixtures/schema/` | Workout, Exercise, WorkoutSet, Tag | `pub id: String` (all four) | compliant |
| `src/snapshots.rs` fixture builders | Role, Comment, Article | `FieldDef::new("id", FieldType::String, FieldRole::Id)` | compliant |

**Result: the audit is clean.** No in-tree consumer or fixture uses `i64` or
`Uuid` primary keys. No migration pass is required before the first markdown
consumer ships; the ADR's "Consequences" worry about existing examples is
closed affirmatively (and should be recorded as such when the ADR is
accepted).

## What the constraint cascades into

- **`gen_seaorm` column inference**: string ids map to `TEXT` primary-key
  columns. Already the only shape exercised in-tree; no change.
- **Filename safety (markdown backend)**: because ids become path segments,
  the markdown runtime must validate them at the path-construction boundary —
  reject empty ids, path separators (`/`, `\`), `.`/`..`, and NUL — so a
  hostile or malformed id can never escape the vault root. This lands as
  `record_path(...) -> Result<PathBuf, Error>` validation in the runtime
  crate, not as a schema-layer rule, so it also protects ids that arrive at
  runtime via API input rather than from the schema.
- **Id derivation (`IdStrategy`)**: new records without a caller-supplied id
  derive one (slug-from-field, uuid). Derived ids are produced pre-validated
  (slugs are `[a-z0-9-]` by construction).
- **Schema-time enforcement (follow-up)**: `parse_schema` could reject
  non-`String` `#[ontology(id)]` fields eagerly for markdown-backend
  consumers. Deferred — runtime validation covers the safety property, and
  schema-time enforcement needs the backend choice to be visible at parse
  time, which it currently is not.

## Decision

1. The constraint is adopted as specified by ADR 0001; nothing in-tree needs
   migration.
2. Enforcement is at the markdown runtime's path boundary (validated
   `record_path`), plus documentation in the backend-choice guide.
3. SeaORM consumers keep the freedom they have today; the constraint binds
   only entities routed through a markdown-backed store, but new schemas are
   advised to use `String` ids unconditionally so the backend choice stays
   reversible.
