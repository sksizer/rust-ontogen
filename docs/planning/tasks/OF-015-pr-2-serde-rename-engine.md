---
type: task
schema_version: '1'
status: closed/done
created: 2026-05-19
last_reviewed: 2026-05-19
impact: high
complexity: medium
tags: [ontogen-ts, ts-pipeline]
related: [OF-015, OF-015-pr-1]
completion_note: "Shipped in #61 (merge f85e8f0, 2026-05-19). Commits d7a6def + ff463fe + e38a9e1 — serde rename engine (rename_all + rename + skip) covering AC-4."
---
# OF-015 PR 2 — Serde rename engine (rename_all + rename + skip)

## Goal

Implement the serde rename family inside `ontogen-ts`: `#[serde(rename = "...")]` on fields and variants, `#[serde(rename_all = "...")]` on containers (all 8 modes), `#[serde(skip)]` on fields, and field-level-wins-over-container precedence. Roll our own case-transform engine — do NOT depend on `heck` (its acronym rules diverge from serde's). Property-test against `serde_json::to_string` to verify wire-name equality. Satisfies AC-4 of [OF-015](./OF-015-productionize-typescript-generation.md).

## Today

After PR 1 lands, `crates/ontogen-ts/src/emit.rs::emit_struct` produces TS field names from the Rust ident verbatim — no rename application. The `EmitConfig` struct exposes a `case_default: Option<RenameAll>` field but nothing reads it. `crates/ontogen-ts/src/types.rs::RenameAll` has the enum variants but no case-transform implementation. Serde attribute parsing on `syn::Attribute` lists is not yet wired up anywhere in the crate.

## Approach

Three commits inside the worktree:

1. **Case-transform engine** in `crates/ontogen-ts/src/rename.rs` (new).
   - `pub(crate) fn split_words(ident: &str) -> Vec<&str>` — the hard part. Handle:
     - Snake/kebab boundaries: `parse_url_v2` → `["parse", "url", "v2"]`
     - PascalCase / camelCase boundaries: `HTMLParser` → `["HTML", "Parser"]`, `parseHTML` → `["parse", "HTML"]`
     - Acronym runs preserve consecutive uppercase: `parseURLv2` → `["parse", "URL", "v2"]`
     - Digit transitions handled per serde's behavior — keep digits attached to the preceding word fragment unless preceded by a separator.
   - `pub(crate) fn apply(words: &[&str], mode: RenameAll) -> String` — one match arm per mode covering all 8: `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE`.
   - Unit tests: per-mode tables of (input ident → expected output) covering primitive cases + acronym + digit edge cases.

2. **Serde attribute extraction** in `crates/ontogen-ts/src/attr.rs` (new) or extending `rename.rs`.
   - `pub(crate) struct SerdeContainerAttrs { rename_all: Option<RenameAll>, ... }`
   - `pub(crate) struct SerdeFieldAttrs { rename: Option<String>, skip: bool, ... }`
   - Parse from `&[syn::Attribute]` — walk meta items, look for `serde(...)` with `rename`, `rename_all`, `skip`. Reject `serde(rename(serialize = "...", deserialize = "..."))` (split-rename) with `UnsupportedSerdeAttr { type_path, attr: "split-rename" }` plus a hint.
   - Reject malformed `rename_all` values with `UnsupportedSerdeAttr`.
   - Unit tests parse fixture `syn::ItemStruct` / `syn::ItemEnum` via `parse_quote!` and assert extracted attrs.

3. **Wire renames into emission** by modifying `crates/ontogen-ts/src/emit.rs`:
   - In `emit_struct`: read container attrs → for each field, read field attrs → compute wire name with precedence (field-level `rename` wins over container `rename_all`). Skip fields with `serde(skip)`.
   - In `emit_enum`: same precedence for variant idents.
   - Add `crates/ontogen-ts/tests/rename_roundtrip.rs` — property tests:
     - Define ~20 fixture struct + enum types with various serde rename combos.
     - For each fixture: round-trip a value through `serde_json::to_string`, parse keys/discriminants, assert they match the TS field/variant names ontogen-ts emits.
     - Cover acronym + digit edge cases (`HTMLParser`, `parse_url_v2`, etc.) explicitly.
   - Add `serde` + `serde_json` as `[dev-dependencies]` in `crates/ontogen-ts/Cargo.toml` for the property tests.

Each commit builds clean and `just full-check` passes.

## Files to touch

- `crates/ontogen-ts/src/rename.rs` (new) — case-transform engine.
- `crates/ontogen-ts/src/attr.rs` (new) — serde attribute extraction.
- `crates/ontogen-ts/src/emit.rs` (modify) — apply renames in struct/enum emission.
- `crates/ontogen-ts/src/lib.rs` (modify) — register the new modules.
- `crates/ontogen-ts/src/types.rs` (modify) — `RenameAll` may need extra derive(s) or impls.
- `crates/ontogen-ts/Cargo.toml` (modify) — add `serde` + `serde_json` to `[dev-dependencies]`.
- `crates/ontogen-ts/tests/rename_roundtrip.rs` (new) — property tests against `serde_json::to_string`.

## Acceptance criteria

These are AC-4 from OF-015 — restated here for per-PR scope:

- [ ] AC-4.1: `#[serde(rename = "wireName")]` on fields and enum variants substitutes the wire name in emitted TS.
- [ ] AC-4.2: `#[serde(rename_all = "...")]` on containers covers all 8 serde modes (`lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `SCREAMING-KEBAB-CASE`).
- [ ] AC-4.3: Field-level `rename` wins over container `rename_all` (precedence preserved).
- [ ] AC-4.4: `#[serde(skip)]` drops the field from TS emission.
- [ ] AC-4.5: Split-rename (`#[serde(rename(serialize=..., deserialize=...))]`) raises `UnsupportedSerdeAttr` with hint pointing at symmetric form or `#[ontogen::ts_opaque]`.
- [ ] AC-4.6: At least 20 fixture round-trips through `serde_json::to_string` confirm wire-name equality against ontogen-ts's emitted TS field/variant names. Acronym + digit edge cases included (`HTMLParser → htmlParser`, `parse_url_v2 → parseUrlV2` for camelCase; analogous for other modes).
- [ ] AC-4.7: Case-transform engine is in-crate (not depending on `heck`); unit tests assert per-mode outputs for at least 8 input idents covering acronym + digit boundaries.

## Out of scope

- **Shape-changing serde attrs** (`tag`, `content`, `untagged`, `flatten`) — phase 2 / OF-015 phase 2. PR 2 rejects with `UnsupportedSerdeAttr` if encountered.
- **Cross-field renames** (`#[serde(rename_all_fields)]` on enums) — not in OF-015 phase 1 scope.
- **Type collection** — PR 3.
- **External-types lookup** — PR 3.

## Dependencies

- [[OF-015-pr-1-scaffold-and-emission]] must land first (provides the `EmitConfig`, `RenameAll`, struct/enum emission entry points this PR builds on).
