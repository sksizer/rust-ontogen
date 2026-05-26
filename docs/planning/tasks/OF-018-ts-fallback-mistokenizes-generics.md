---
schema_version: '3'
status: planning/proposed
impact: low
complexity: small
last_reviewed: '2026-05-26'
definition_gap: 'OF-015 closed/done (epic E0001 shipped 2026-05-15..05-20) kept FallbackRecord as a defensive backstop rather than deleting it (commit 15664f9), so Direction A applies. The buggy rust_type_to_ts / collect_ts_import in src/servers/types.rs:393,430 still feed both src/clients/generators/transport.rs and src/clients/generators/ts_client.rs. The Approach below mirrors the OF-008 / OF-010 AST migration on the TS side; AST forms (ApiFn.return_type_ast, Param.ty_ast) are already populated by src/servers/parse.rs and ready to consume.'
---
# OF-018 - TS bindings fallback emitter mis-tokenizes generic return types

- **Severity:** Low (cosmetic + invalid TS, but the obvious path through this code is the OF-015 `ontogen-ts` pipeline which populates `bindings.ts` end-to-end and doesn't exercise this fallback in the happy case)
- **Source:** Pumice feedback, [`docs/feedback/2026-05-14-pumice.md`](../feedback/2026-05-14-pumice.md). Filed in the consumer's log as "OF-015" (their numbering); upstream uses OF-018 to avoid collision with the (now-closed) [OF-015](./OF-015-productionize-typescript-generation.md) TS-pipeline productionization.
- **Related:** [OF-006](./OF-006-ts-bindings-fallback-warning.md) (which made the fallback observable but didn't fix the tokenizer), [OF-014](./OF-014-redesign-ts-bindings-pipeline.md) (the spike that the OF-015 epic replaced), [OF-015](./OF-015-productionize-typescript-generation.md) (closed/done; kept `FallbackRecord` as a defensive backstop, so this bug remains live on the backstop path), [OF-008](./OF-008-inner-type-strip-option.md) / [OF-010](./OF-010-collect-type-import-generics.md) (the analogous Rust-side AST migration we mirror here).

## Goal

Migrate the TS-side fallback collection pipeline from string-prefix parsing to a `syn::Type` AST walker so multi-arg generics (`HashMap<K, V>`, `Result<T, E>`, nested containers) no longer produce invalid TypeScript identifiers like `NotificationPrefs>` in the `Record<string, unknown>` placeholder emitter and OF-006 warning path.

## Today

| Location | Role today |
|---|---|
| `src/servers/types.rs:393-427` (`rust_type_to_ts`) | String-prefix dispatcher: handles `()`, `String`/`&str`/`str`, primitive integers + floats, `bool`, `&T`, `Option<T>`, `Vec<T>`, and the `relation::Model` / `seg::seg` qualified-path special case via string slicing. Anything else (notably `HashMap<K, V>`, `BTreeMap<K, V>`, tuples, user generics) falls through to `ty.to_string()` unchanged. |
| `src/servers/types.rs:430-445` (`collect_ts_import`) | Splits the rendered TS string on ` \| ` (for `T \| null` shapes), trims a trailing `[]`, skips primitive idents, and pushes the remainder. Has no awareness of `<` / `>` / `,`, so a multi-arg generic that fell through `rust_type_to_ts` is pushed verbatim or partially split, yielding tokens like `NotificationPrefs>`. |
| `src/servers/types.rs:111-212` (`collect_type_import`, `walk_path_args`) | The Rust-side counterpart, already AST-driven post-OF-008/OF-010. Walks `syn::Type` against `KNOWN_CONTAINERS` and `is_prelude_scalar`. This is the template the TS-side migration mirrors. |
| `src/clients/generators/transport.rs:110-115, 221, 237, 242, 267, 271, 453, 468, 472, 555, 565, 773, 790` | Call sites that feed rendered Rust type strings (`f.return_type`, `extract_input_type(&p.ty)`, `strip_ref(&pp.ty)`) into `rust_type_to_ts` / `collect_ts_import`. The first call cluster (110-115) is the `FallbackRecord` collection block; the rest are inline param/return renderings inside emitted TS code. |
| `src/clients/generators/ts_client.rs:51-56, 151, 170, 178, 221, 254` | Parallel collection block (51-56) plus inline renderings in the legacy TS-client emitter — same bug shape, same fix. |
| `src/clients/generators/mod.rs:55-75` (`FallbackRecord`) | Defensive-backstop record produced by the transport / ts_client emitters when a referenced TS type is missing from `bindings.ts`. Drained by `crate::clients::generate` into `cargo:warning=` lines. Kept by OF-015 (commit 15664f9) for the stale-`bindings.ts` / root-set-drift cases. |
| `src/servers/parse.rs:42-58, 105-117` (`ApiFn`, `Param`) | Already store `return_type_ast: syn::Type` and `ty_ast: syn::Type` alongside the rendered strings, populated by the parser. No upstream change is required to feed AST forms into the new walkers. |
| `src/servers/tests.rs:347` (`test_collect_type_import_matrix`) | The exemplar matrix the new TS-side tests should mirror. |
| `src/servers/tests.rs:518` (`test_rust_type_to_ts`) | Existing string-input tests that the new AST-based emitter needs equivalent (or replacement) coverage for. |

## Proposed

Replace `rust_type_to_ts(&str)` and `collect_ts_import(&str, ...)` with AST-driven siblings `rust_type_to_ts_ast(&syn::Type) -> String` and `collect_ts_import_ast(&syn::Type, &mut Vec<String>)` that pattern-match on `syn::Type` (mirroring `collect_type_import`'s shape), then update the `FallbackRecord` collection call sites in `transport.rs` and `ts_client.rs` to pass `f.return_type_ast` / `p.ty_ast` instead of rendered strings. The string-based functions stay as thin wrappers (parse-then-delegate) during the transition so the inline-rendering call sites elsewhere in `transport.rs` / `ts_client.rs` don't need to move in the same patch.

## Approach

1. Add `rust_type_to_ts_ast(ty: &syn::Type) -> String` in `src/servers/types.rs`, modelled on `collect_type_import`'s structure:
   - `Type::Reference` → peel and recurse.
   - `Type::Path` with a single segment matching a primitive (`String`, `&str` shape via the reference arm, `bool`, integers, floats) → emit the TS primitive.
   - `Type::Path` with head `Option` → recurse on the inner arg and append ` | null`.
   - `Type::Path` with head `Vec` → recurse on the inner arg and append `[]`.
   - `Type::Path` with head `HashMap` / `BTreeMap` → emit `Record<{K_ts}, {V_ts}>` after recursing on each arg.
   - Smart-pointer peel set (`Box`, `Rc`, `Arc`, `Cow`, `Pin`) → peel and recurse (matches the OF-015 design's transparency rule).
   - Qualified path `relation::Model` → `RelationModel`; other multi-segment paths → terminal ident.
   - Fallback for tuples / unrecognized shapes → emit `unknown` (mirrors the existing string-fallback's "leave it alone" semantics without producing invalid TS; logged as out-of-scope below).
2. Add `collect_ts_import_ast(ty: &syn::Type, imports: &mut Vec<String>)` next to it, mirroring `collect_type_import` exactly: walk `Type::Path`, skip `KNOWN_CONTAINERS` heads (recurse into args), skip `is_prelude_scalar` leaves, push remaining single-segment idents, recurse into qualified paths' final segment's generic args. Reuse `walk_path_args` if practical; otherwise add a TS-specific sibling.
3. Update the two `FallbackRecord` collection call sites to consume the AST:
   - `src/clients/generators/transport.rs:110-115`: replace `rust_type_to_ts(&f.return_type)` + `collect_ts_import(&ts_ret, &mut import_types)` with `collect_ts_import_ast(&f.return_type_ast, &mut import_types)`; same shape for the param loop using `p.ty_ast` (skip the `extract_input_type` string detour — the AST already carries the unwrapped form once `param_to_owned_type` / its sibling is applied, or we add a small AST `extract_input_type_ast` if needed).
   - `src/clients/generators/ts_client.rs:51-56`: same edit.
4. Leave `rust_type_to_ts(&str)` / `collect_ts_import(&str, ...)` in place as deprecated string-input shims that internally call `syn::parse_str::<syn::Type>` and delegate, so the inline-rendering call sites later in both files keep compiling. Mark them `#[deprecated(note = "...")]` or add a `// TODO(OF-018 follow-up)` comment pointing at a future pass that migrates the inline renderings too. The follow-up is explicitly out of scope (the bug doesn't reach those call sites in practice because the strings there come from `extract_input_type` / `strip_ref` on already-narrowed types).
5. Add a unit-test module mirroring `test_collect_type_import_matrix` and `test_rust_type_to_ts`, driven by `syn::parse_str` over the input column of each table in the [Tests](#tests) section below.
6. Add an end-to-end test mirroring `test_of013_unsized_dst_owned_form_in_ipc`: feed a service fn with `Result<HashMap<String, NotificationPrefs>, AppError>` through `parse::scan_api_dir`, run the transport generator against an empty `bindings.ts`, and assert every emitted placeholder type name is a syntactically valid TS identifier (matches `/^[A-Za-z_$][A-Za-z0-9_$]*$/`).
7. `just full-check` clean; commit one logical change per step where it tightens review.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `src/servers/types.rs` | modify | Add `rust_type_to_ts_ast` and `collect_ts_import_ast` (~lines 393-445 region). Convert the existing string-input functions into thin wrappers that parse via `syn::parse_str` and delegate. |
| `src/clients/generators/transport.rs` | modify | Update the `FallbackRecord` collection block (~lines 110-115) to call the AST walkers against `f.return_type_ast` / `p.ty_ast`. Inline-rendering sites (221, 237, 242, 267, 271, 453, 468, 472, 555, 565, 773, 790) stay on the string shims for now. |
| `src/clients/generators/ts_client.rs` | modify | Update the parallel `FallbackRecord` collection block (~lines 51-56). Same scope rule as transport.rs. |
| `src/servers/tests.rs` | modify | Add `test_collect_ts_import_ast_matrix` and `test_rust_type_to_ts_ast`, mirroring the existing OF-008/OF-010 templates at lines 347 and 518. Optionally backfill the existing `test_rust_type_to_ts` to also cover `HashMap` / nested generics via the new AST path. |
| `tests/` (an existing integration test file, location TBD during implementation) | modify | Add an end-to-end test that exercises the `FallbackRecord` collection path against a `Result<HashMap<…>, _>` return type and asserts emitted placeholder names are valid TS identifiers. Mirror the OF-013 IPC-shape test. |

## Acceptance criteria

- [ ] AC-1: `rust_type_to_ts_ast(&syn::Type)` exists in `src/servers/types.rs` and produces the expected TS rendering for the input matrix in the [Tests](#tests) section (`String`, `Option<String>`, `Vec<u8>`, `HashMap<String, NotificationPrefs>`, `Vec<HashMap<String, T>>`, `Option<Vec<&str>>`, `relation::Model`).
- [ ] AC-2: `collect_ts_import_ast(&syn::Type, &mut Vec<String>)` exists in `src/servers/types.rs` and produces the expected import sets for the input matrix (`HashMap<String, NotificationPrefs>` → `["NotificationPrefs"]`, `Vec<Option<UserType>>` → `["UserType"]`, `HashMap<UserKey, Vec<UserValue>>` → `["UserKey", "UserValue"]`, `Option<String>` → `[]`, `Vec<&str>` → `[]`).
- [ ] AC-3: The `FallbackRecord` collection call sites in `src/clients/generators/transport.rs` (~L110-115) and `src/clients/generators/ts_client.rs` (~L51-56) consume `f.return_type_ast` / `p.ty_ast` via the new AST walkers; rendered strings are not used for collection on this path.
- [ ] AC-4: End-to-end test: a service fn returning `Result<HashMap<String, NotificationPrefs>, AppError>` parsed through `parse::scan_api_dir` and run through the transport generator against an empty `bindings.ts` emits `FallbackRecord` entries whose `type_name`s all match `/^[A-Za-z_$][A-Za-z0-9_$]*$/` (no `<`, `>`, `,`, or whitespace). At minimum `NotificationPrefs` (without trailing `>`) appears.
- [ ] AC-5: `just full-check` is clean (rustfmt, clippy, cargo test, cargo doc).
- [ ] AC-6: The existing `test_rust_type_to_ts` and `test_collect_type_import_matrix` tests still pass — the string shims preserve current behavior for the inline-rendering call sites that remain on the string API.

## Out of scope

- Migrating the *inline rendering* call sites in `transport.rs` (L221, 237, 242, 267, 271, 453, 468, 472, 555, 565, 773, 790) and `ts_client.rs` (L151, 170, 178, 221, 254) to the AST walkers. These paths consume already-narrowed type strings produced by `extract_input_type` / `strip_ref` and don't exhibit the multi-arg-generic bug in practice. A follow-up ticket can do that pass once the AST API has settled.
- Tuple return types (`(A, B)`): the existing `rust_type_to_ts` doesn't handle them either; rendering them is its own design question.
- User-defined generics (`MyContainer<T>`): rejected in OF-015 phase 1 and tracked in [OF-021](./OF-021-user-defined-generics-in-ts-emitter.md). The new walker's "unknown head with args" arm should fall back gracefully (push nothing, render `unknown`) so this case stops *crashing the tokenizer* without claiming to support it.
- Sharing a single AST walker between the Rust-side `collect_type_import` and the new TS-side `collect_ts_import_ast`. They differ only in their "skip set" (TS additionally skips `String` because it emits as `string`); a parameterized helper would be cleaner but isn't justified by two call sites. Revisit if a third lands.
- Deleting `FallbackRecord` entirely. OF-015 considered this and decided to keep it as a defensive backstop (commit 15664f9). Re-litigating that decision is out of scope here.

## Dependencies

- none (OF-015 is closed/done; the FallbackRecord path it kept is the path this ticket fixes)

## Discovery context

Surfaced via Pumice's 2026-05-14 feedback log, where a `Result<HashMap<String, NotificationPrefs>, AppError>` return type produced the invalid TS placeholder `type NotificationPrefs> = Record<string, unknown>;` — a `>` character leaked into a TS identifier, breaking `eslint` / `prettier` / `tsc` immediately. Filed upstream as OF-018 (Pumice's local numbering called it "OF-015" before the upstream ticket existed). Held during OF-015's design and implementation because OF-015 explicitly listed "remove the fallback path entirely" as a viable resolution that would close OF-018 naturally. OF-015 closed/done on 2026-05-20 *kept* `FallbackRecord` as a defensive backstop for stale `bindings.ts` / root-set drift / hand-edit cases (commit 15664f9 rewrote the doc comment to describe that role), so the bug remains live on a narrower surface and Direction A — mirror the OF-008/OF-010 AST migration on the TS side — applies. Picked up now because the implementation-ready gate flagged stale file paths in the original task body; restructuring the task surfaced that the underlying technical content is intact and the work is ready to scope.

## Problem

The TS bindings fallback path (the placeholder `Record<string, unknown>` emitter, plus its sibling for the OF-006 warning) collects type names via string-based parsing on the rendered Rust return type:

```rust
// src/servers/generators/transport.rs:111-117
let ts_ret = rust_type_to_ts(&f.return_type);
collect_ts_import(&ts_ret, &mut import_types);
for p in &f.params {
    let ty = extract_input_type(&p.ty);
    let ts_ty = rust_type_to_ts(&ty);
    collect_ts_import(&ts_ty, &mut import_types);
}
```

`rust_type_to_ts` (`src/servers/types.rs:393`) handles `Option<T>` and `Vec<T>` by string-prefix match but does *not* recognize `HashMap<K, V>` (or any other multi-arg generic). Anything that doesn't match the small prefix table falls through to the catch-all branch at `:419-426`, which returns the unmodified type string (or splits on `::` for `relation::Model`).

Then `collect_ts_import` (`src/servers/types.rs:430`) splits the result only on `|` (for `T | null` shapes) and trims a trailing `[]` — it has no understanding of `<>` brackets, so it pushes the *entire* rendered substring as an "import."

The composite effect: a return type like `Result<HashMap<String, NotificationPrefs>, AppError>` produces:

1. The parser/AST already strips `Result<…>`, leaving `f.return_type` ≈ `HashMap<String, NotificationPrefs>` (in rendered string form).
2. `rust_type_to_ts` doesn't match `Vec<` / `Option<` / primitive / `&` / `::` — falls through unchanged.
3. `collect_ts_import` splits on `|` (no split), trims `[]` (no-op), pushes the whole string.
4. Wait — actually it pushes `HashMap<String, NotificationPrefs>` as one entry, but then `partition` against `exported_types` (`transport.rs:124`) splits on commas implicitly because the fallback emitter at `:162-167` formats each missing type as `type {t} = ...;` — let's check the verbatim symptom.

Per the consumer's repro, the emitted placeholder is:

```ts
// TODO: Type 'NotificationPrefs>' not yet exported from bindings.ts
type NotificationPrefs> = Record<string, unknown>;
```

So `NotificationPrefs>` (trailing `>` captured) is what landed in `import_types`. That suggests the tokenization splits on `,` somewhere — likely the rendered type was tokenized into `["HashMap<String", " NotificationPrefs>"]` and the second token went through `collect_ts_import`'s "strip-`[]`-trim" path which doesn't touch the trailing `>`. The exact split site isn't critical — the bug is that the entire pipeline operates on strings instead of the AST.

Either way: invalid TS identifier emitted as a type alias, breaks `eslint`/`prettier`/`tsc` immediately.

## Location

- `src/servers/types.rs:393-427` (`rust_type_to_ts`) — handles `Vec<`/`Option<`/primitives via string prefix; falls through for everything else.
- `src/servers/types.rs:430-445` (`collect_ts_import`) — splits on `|` and `[]` only; no awareness of `<>`.
- `src/clients/generators/transport.rs:110-115` (the `FallbackRecord` collection call site, relocated from `src/servers/generators/` under commit e678999).
- `src/clients/generators/ts_client.rs:51-56` (parallel collection block in the legacy TS-client emitter — same bug shape).
- Compare with the Rust-side counterpart `collect_type_import` at `src/servers/types.rs:111`, which walks `syn::Type` correctly post-OF-008/10. The TS side never received the same migration.

## Current behavior

Reproducer:

```rust
pub async fn get_prefs(state: &Store) -> Result<HashMap<String, NotificationPrefs>, AppError> { ... }
```

Generated `transport.ts`:

```ts
// TODO: Type 'NotificationPrefs>' not yet exported from bindings.ts
type NotificationPrefs> = Record<string, unknown>;
```

— invalid TypeScript. The consumer's workaround is the same OF-010-era pattern: wrap the map in a named struct (`NotificationPrefsMap { entries: HashMap<…> }`) so the return type collapses to a single ident and avoids the tokenizer entirely.

## Proposed resolution

OF-015 closed/done 2026-05-20 and kept `FallbackRecord` as a defensive backstop (commit 15664f9), so this ticket ships as the AST-ification migration originally outlined as Direction A in the design pass: replace the string-based `rust_type_to_ts` / `collect_ts_import` pipeline with `syn::Type`-driven siblings, then update the two `FallbackRecord` collection call sites to consume the AST. Mirrors the OF-008 / OF-010 migration that landed on the Rust side. Concrete steps live in the [Approach](#approach) section above.

## Effort

Small-to-medium (similar shape to OF-008 / OF-010). The AST forms (`ApiFn.return_type_ast`, `Param.ty_ast`) are already populated by `src/servers/parse.rs`, so no upstream parser work is required.

## Tests

Unit tests on `rust_type_to_ts_ast`:

| Input (parsed via `syn::parse_str`) | Expected TS rendering |
|---|---|
| `String` | `string` |
| `Option<String>` | `string \| null` |
| `Vec<u8>` | `number[]` |
| `HashMap<String, NotificationPrefs>` | `Record<string, NotificationPrefs>` |
| `Result<HashMap<String, NotificationPrefs>, AppError>` | (Result stripped upstream; same as above) |
| `Vec<HashMap<String, T>>` | `Record<string, T>[]` |
| `Option<Vec<&str>>` | `string[] \| null` |
| `relation::Model` | `RelationModel` |

Unit tests on `collect_ts_import_ast`:

| Input | Expected idents collected |
|---|---|
| `HashMap<String, NotificationPrefs>` | `["NotificationPrefs"]` |
| `Vec<Option<UserType>>` | `["UserType"]` |
| `HashMap<UserKey, Vec<UserValue>>` | `["UserKey", "UserValue"]` |
| `Option<String>` | `[]` (primitives skipped) |
| `Vec<&str>` | `[]` |

End-to-end mirroring `test_of013_unsized_dst_owned_form_in_ipc`: parse a service fn through `parse::scan_api_dir`, run `transport::generate` against an empty `bindings.ts`, assert every emitted placeholder type name is a valid TS identifier (no `<`, `>`, `,`, or whitespace).

## Open questions

- **Should the AST walker be shared between Rust-side and TS-side collection?** The "skip containers, recurse into args, collect leaf idents" loop is identical; the only thing that differs is which set of leaf idents we want (Rust skips `String`, TS also skips `String` → emits `string`). A shared helper parameterized by a "skip set" feels right but should wait until the second use case actually exists. (Captured in [Out of scope](#out-of-scope).)
- **`Cow<'_, T>` / `Box<T>` in return types?** `KNOWN_CONTAINERS` already includes both for the Rust side. The TS side needs them in its skip-set too. Confirmed via the OF-015 smart-pointer-transparency decision; the new walker should peel `{Box, Rc, Arc, Cow, Pin}` before classification.
- **Tuple return types?** `(A, B)` doesn't have a clean TS rendering today (`rust_type_to_ts` doesn't handle them). Out of scope for this ticket; the consumer didn't hit it.

## Notes

- The consumer's verbatim entry under their "OF-015" includes the rendered TS that breaks `eslint`/`prettier`. Reproduced fresh on `168ff379`.
- The Pumice workaround (`NotificationPrefsMap` wrapper) is the same shape as OF-010's pre-fix workaround. Listed in the OF-014 spike outcome as "kept due to TS emitter bug — see OF-015" → that "OF-015" reference is to the consumer's numbering, i.e., this ticket.
- This ticket bundles cleanly with any future "AST-ify the rest of the TS emitter" pass. There are ~14 inline-rendering call sites across `src/clients/generators/transport.rs` (lines 221, 237, 242, 267, 271, 453, 468, 472, 555, 565, 773, 790) and `src/clients/generators/ts_client.rs` (lines 151, 170, 178, 221, 254) that still consume rendered strings; the [Out of scope](#out-of-scope) entry captures the follow-up.
