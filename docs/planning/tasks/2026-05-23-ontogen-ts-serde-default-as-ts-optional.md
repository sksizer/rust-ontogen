---
type: task
schema_version: '3'
status: closed/done
created: '2026-05-23'
impact: medium
complexity: medium
tags:
- ontogen-ts
- pumice-follow-up
- serde-shape
related: []
autonomy: supervised
last_reviewed: '2026-05-24'
completion_note: |
  Shipped via #75 (merge 5001844, 2026-05-24). PR was opened on
  feat/ontogen-ts-serde-default-optional rather than the
  task/<basename> convention, so /sdlc:task-close-out's auto-detect
  didn't catch it during /sdlc:orchestrate tick #1 — closed out
  manually here.
---
# ontogen-ts: render fields with #[serde(default)] as TS-optional (?) to match the wire contract

## Goal

Rust struct fields annotated with `#[serde(default)]` should emit as TS-optional (`field?: T`) instead of required (`field: T`). The Rust deserializer accepts partial JSON for any `#[serde(default)]` field — it substitutes `Default::default()` when the field is absent — so the TS shape ontogen-ts emits today is stricter than the wire contract actually requires. Specta (the predecessor emitter) recognized this attribute and emitted with `?`; ontogen-ts's bump to alpha0.0.2 regressed that behavior for every Pumice-style consumer.

## Today

ontogen-ts's AST walker reads each `syn::Field` and emits a TS field declaration. It already handles a few field-shape mappings (`Option<T>` → `string | null`; the schema-known surface partition; the external-types table). It does NOT inspect the field's serde attribute list for `#[serde(default)]` or its `#[serde(default = "path::to::fn")]` variant; the resulting TS field is always required.

| Location | Role today |
|---|---|
| `crates/ontogen-ts/src/` | The field-emit code path walks `syn::Field` and renders a TS declaration. Serde attribute parsing exists for `#[serde(rename = "...")]` (per the alpha0.0.2 rename-engine work) and the container `rename_all`, but not for `#[serde(default)]`. |
| `tests/` | Unit fixtures cover `Option<T>`, primitive mapping, external-types, name collisions, but no fixture for `#[serde(default)]`-marked fields. |

A concrete consumer hit by this today: Pumice's `ProfileInput` (see `crates/pumice/src/...` and the symptom in sksizer/pumice#225) has three `#[serde(default)]` fields (`auto_start_breaks`, `auto_start_focus`, `default_tags`). Pre-alpha0.0.2 (specta) they emitted as optional; post-alpha0.0.2 (ontogen-ts) they emit as required, which forced Pumice to either fill in defaults at every call site or sync its hand-written copy to the new stricter shape (Pumice chose the latter — see the `ProfileInput.notesTemplate` widening in sksizer/pumice#225).

## Proposed

When the field walker encounters `#[serde(default)]` (both the bare attribute form `#[serde(default)]` and the path form `#[serde(default = "module::fn")]`), emit the TS field with a trailing `?` on the field name. Combined with the existing `Option<T>` → `string | null` mapping, a field that is both optional-and-nullable emits as `field?: T | null`.

End state: `ProfileInput { name: String, #[serde(default)] auto_start_breaks: bool }` emits as `type ProfileInput = { name: string; autoStartBreaks?: boolean; }` instead of the current `autoStartBreaks: boolean`. Pumice (and any future consumer with `#[serde(default)]` shapes) gets a TS contract that matches the wire — clients can omit the field, the Rust side substitutes the default, and TS doesn't complain.

## Approach

1. **Locate the field-emit code.** In `crates/ontogen-ts/src/`, find the function that renders a `syn::Field` into a TS declaration. The serde rename-engine (added in alpha0.0.2) likely lives nearby and parses `#[serde(rename = "...")]` — extend that attribute-parsing pass to also recognize `#[serde(default)]`.
2. **Recognize both attribute forms.** Bare `#[serde(default)]` and path-form `#[serde(default = "path")]` both signal the same thing for ontogen-ts's purposes (the deserializer substitutes a default if absent). Treat both as "emit with `?`."
3. **Compose with Option<T>.** A field that is `Option<T>` AND `#[serde(default)]` is BOTH optional-and-nullable: emit as `field?: T | null`. The existing `Option<T>` → `string | null` mapping is unchanged; the `?` is a separate per-field flag.
4. **Test fixtures.**
   - Field with `#[serde(default)]` on a non-Option type → `field?: T`.
   - Field with `#[serde(default)]` on `Option<T>` → `field?: T | null`.
   - Field with `Option<T>` alone (no default) → `field: T | null` (no `?`).
   - Field with no annotation → `field: T` (no `?`).
   - Path form `#[serde(default = "path::to::fn")]` → same as bare default.
5. **Verify against iron-log.** No `#[serde(default)]` in iron-log's API shapes today; expect byte-identical generated TS.
6. **Verify against Pumice.** Once landed and a new alpha tag is cut, sksizer/pumice#225 follow-up: revert the appropriate hand-written `ProfileInput` field annotations back to `?` and confirm the new generated shape matches.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `crates/ontogen-ts/src/` | modify | extend the field-attribute parser to recognize `#[serde(default)]` and `#[serde(default = "path")]`; thread the resulting flag into the field renderer so it emits a trailing `?` on the field name. |
| `crates/ontogen-ts/tests/` | new | fixtures for each of the four shapes in Approach step 4. |

## Acceptance criteria

- [ ] AC-1: Unit test: a struct field annotated `#[serde(default)]` on a non-Option type emits as `field?: T` in the generated TS.
- [ ] AC-2: Unit test: `#[serde(default)]` combined with `Option<T>` emits as `field?: T | null`.
- [ ] AC-3: Unit test: the path form `#[serde(default = "module::fn")]` emits identically to the bare `#[serde(default)]` form.
- [ ] AC-4: `cargo build` in `examples/iron-log/src-tauri/` succeeds with byte-identical generated TS (no `#[serde(default)]` in iron-log today).
- [ ] AC-5: `just full-check` passes on the rust-ontogen branch.

## Out of scope

- **Container-level serde attributes** like `#[serde(deny_unknown_fields)]`, struct-level `#[serde(default)]`, `#[serde(tag = "...")]`, etc. — separate concerns.
- **Other field-level serde attributes** (`#[serde(skip_serializing_if = "...")]`, `#[serde(with = "...")]`, etc.) — separate tickets if real consumers need them.
- **Reverting Pumice's `ProfileInput.notesTemplate` widening** — lives in Pumice (sksizer/pumice#225). Note that `notesTemplate` is `Option<String>` WITHOUT `#[serde(default)]` upstream, so this task's fix would NOT revert that specific field; the widening there is structurally correct. The fix would matter for the other three Pumice `ProfileInput` fields (`auto_start_breaks`, `auto_start_focus`, `default_tags`) which ARE `#[serde(default)]`-marked.

## Dependencies

- none. This is a pure additive feature in ontogen-ts's field-attribute parser.

## Discovery context

- Surfaced by Pumice's bump to alpha0.0.2 (sksizer/pumice#225). Specta emitted `ProfileInput.autoStartBreaks?: boolean` (optional) for the `#[serde(default)] auto_start_breaks: bool` field; ontogen-ts emits the stricter `autoStartBreaks: boolean`. The hand-written Pumice copy was loose enough (with `?`) to typecheck against either shape, but the post-bump generated shape forced Pumice to sync the hand-written copy in lock-step (a different field, `notesTemplate`, hit a related TS2345 incompatibility).
- This is one of two ontogen-ts gaps the Pumice bump surfaced; the other is the transitive long-tail walk, filed as `[[2026-05-23-ontogen-ts-transitive-walk-long-tail-field-types]]`.
