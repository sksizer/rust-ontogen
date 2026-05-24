---
type: task
schema_version: '3'
status: open/ready
created: '2026-05-23'
impact: medium
complexity: medium
tags:
- long-tail-walker
- ontogen-ts
- pumice-follow-up
related:
- 2026-05-20-ontogen-ts-entity-field-type-closure
autonomy: supervised
---
# ontogen-ts: walker should recurse through long-tail types' field references into the root set

## Goal

When ontogen-ts emits a long-tail type, any non-primitive, non-schema-known type idents referenced in that type's fields should also be added to the long-tail root set (and themselves walked recursively). Today the closure stops at the first level: a long-tail type `RestoreCandidate { manifest: Option<BackupManifest> }` is emitted, but `BackupManifest` is left as a bare TS reference with no definition, producing TS that fails to type-check at the consumer. This closes the residual emission gap that the just-shipped entity-field-type-closure (`[[2026-05-20-ontogen-ts-entity-field-type-closure]]`) didn't cover.

## Today

The long-tail root set in `src/clients/generators/ts_bindings.rs::long_tail` is derived from (a) API endpoint params/returns and (b) — per the just-shipped entity-field-type-closure work — schema-entity field types. Both feed into `ontogen_ts::emit`. The emitter walks the resulting root set and renders each type. Field references INSIDE those rendered types that point at non-primitive, non-schema-known idents are emitted as bare TS identifiers but never themselves added to the root set, so their bodies are never emitted.

| Location | Role today |
|---|---|
| `src/clients/generators/ts_bindings.rs` | `long_tail()` derives the root set from API + entity-field closures, then hands off to `ontogen_ts::emit`. No recursion through emitted types' field references. |
| `crates/ontogen-ts/src/` | `emit()` renders each root to TS but does not feed the closure of referenced types back into the root set. |
| `tests/` | `tests/ts_entity_field_type_closure.rs` covers the entity-field closure case (shipped 2026-05-20). No fixture for transitive long-tail-to-long-tail references. |

## Proposed

When `ontogen_ts::emit` (or the harness around it) renders a long-tail type, harvest the non-primitive, non-schema-known field-type idents in the same way `entity_field_type_names` does (per the closure work shipped 2026-05-20). Add new candidates to the root set and re-iterate until a fixed point is reached. Pool membership rules are unchanged — references not found in the pool surface as `UnresolvedReference` with the existing hard-error semantics.

End state: emitting `RestoreCandidate` also emits `BackupManifest`; emitting `ThemeSettings` also emits `ThemePreference`. Pumice can retire `append_ontogen_compat_stubs` from `build.rs` (filed in sksizer/pumice#225).

## Approach

1. **Identify the emit loop.** Read `src/clients/generators/ts_bindings.rs` and `crates/ontogen-ts/src/` to find where the root set is consumed and rendering happens. Confirm whether the fixed-point loop lives at the call-site (in `ts_bindings::long_tail` / `generate_clients`) or inside ontogen-ts itself. Co-locate the change wherever the existing root-set construction lives.
2. **Reuse the entity-field-type harvester.** The closure work added `is_simple_user_ident`, `field_type_user_ident`, and `entity_field_type_names` helpers. Apply the same harvester to each emitted long-tail type's fields (the rendered struct's `FieldType` references), excluding primitives and schema-known idents.
3. **Fixed-point iteration.** Repeat root-derivation → emit until the root set stops growing. Cycle in the type graph is handled by the visited-set already in the emitter; new candidates that are already in the root set don't add work.
4. **Verify against iron-log.** `cargo build` in `examples/iron-log/src-tauri/` must produce byte-identical generated TS (iron-log's long-tail closure is shallow — no transitive references that aren't already covered).
5. **Verify against Pumice.** Once landed and a new alpha tag is cut, sksizer/pumice#225 follow-up: remove `append_ontogen_compat_stubs` from `pumice/src-tauri/build.rs`, confirm `BackupManifest` and `ThemePreference` are now emitted by ontogen-ts, and confirm `pnpm typecheck` is clean without the stubs.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `src/clients/generators/ts_bindings.rs` | modify | extend the root-set derivation to fixed-point over transitive long-tail field-type references. |
| `crates/ontogen-ts/src/` | modify | if the fixed-point loop is cleaner inside ontogen-ts, expose the harvester there and iterate within `emit`. |
| `tests/` | modify | add a fixture: a long-tail type whose field references ANOTHER long-tail type defined only in the pool; assert both are emitted with full bodies. |

## Acceptance criteria

- [ ] AC-1: Integration test exercising a long-tail type with a field whose type is defined only in the pool (not in the schema-known surface AND not directly an API param/return) — the emitted bindings include both the parent type body and the referenced type body.
- [ ] AC-2: `cargo build` in `examples/iron-log/src-tauri/` succeeds with byte-identical generated TS — no behavioral regression.
- [ ] AC-3: Pumice's `append_ontogen_compat_stubs` (filed in sksizer/pumice#225) can be deleted, with `BackupManifest` and `ThemePreference` now emitted natively by ontogen-ts and `pnpm typecheck` clean.
- [ ] AC-4: `just full-check` passes on the rust-ontogen branch.

## Out of scope

- **Cross-crate `pub use` resolution in the resolver** — separate concern. For sibling-crate types, consumers still need `pool_extra_roots` (per OF-015 PR 7).
- **Re-derivation of entity-field roots** — covered by `[[2026-05-20-ontogen-ts-entity-field-type-closure]]`; this task only adds the transitive walk on top.
- **`UnresolvedReference` semantic changes** — keeps the existing hard-error behavior; if a transitively-walked candidate isn't in the pool, it errors the same way.

## Dependencies

- `[[2026-05-20-ontogen-ts-entity-field-type-closure]]` — already merged in alpha0.0.2 (rust-ontogen #72). This task assumes the entity-field harvester is in place and re-uses it for the transitive walk.

## Discovery context

- Surfaced by Pumice's bump to alpha0.0.2 (sksizer/pumice#225). Without this fix, Pumice carries `append_ontogen_compat_stubs` in `build.rs` to manually emit `BackupManifest` and `ThemePreference` because `RestoreCandidate.manifest` and `ThemeSettings.theme` reference them but ontogen-ts doesn't follow the references into the root set.
- The pre-#69 specta side-car emitted these correctly because specta walks the runtime-registered type universe (every type that hits a `derive(Type)` macro). ontogen-ts's AST-only approach has a different reachability boundary; this task closes that gap for the transitive-references case.
- Same shape of "Pumice surfaces an ontogen-ts gap" as the discovery thread that produced 2026-05-20-ontogen-ts-entity-field-type-closure: the AC-3 verification path for that work bumps a consumer and reveals what's missing from coverage.
