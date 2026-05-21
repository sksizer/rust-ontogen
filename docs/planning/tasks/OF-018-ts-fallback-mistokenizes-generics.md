---
schema_version: '2'
status: planning/proposed
---
# OF-018 - TS bindings fallback emitter mis-tokenizes generic return types

- **Severity:** Low (cosmetic + invalid TS, but the obvious path through this code is the new OF-014 hybrid pipeline which doesn't fall back at all)
- **Source:** Pumice feedback, [`docs/feedback/2026-05-14-pumice.md`](../feedback/2026-05-14-pumice.md). Filed in the consumer's log as "OF-015" (their numbering); upstream uses OF-018 to avoid collision with the in-flight [OF-015](./OF-015-productionize-typescript-generation.md) TS-pipeline productionization.
- **Related:** [OF-006](./OF-006-ts-bindings-fallback-warning.md) (which made the fallback observable but didn't fix the tokenizer), [OF-014](./OF-014-redesign-ts-bindings-pipeline.md) (the new hybrid pipeline that obsoletes the fallback in the happy case), [OF-015](./OF-015-productionize-typescript-generation.md) (will decide the fallback path's ultimate fate — keep / strict-error / remove).

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
- `src/servers/generators/transport.rs:101-119` (the call site that does the actual collection).
- `src/servers/generators/ts_client.rs:` (parallel collection block in the legacy TS-client emitter — same bug shape).
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

Two viable directions; the choice depends on what OF-015 decides about the fallback path's long-term role.

### Direction A: AST-ify the TS collection pipeline

Walk `&syn::Type` on the way to TS the same way `collect_type_import` does on the way to Rust:

1. Replace `rust_type_to_ts(&str)` with `rust_type_to_ts_ast(&syn::Type) -> String` that pattern-matches on the AST and emits `T | null`, `T[]`, `Record<K, V>`, etc.
2. Replace `collect_ts_import(&str, ...)` with `collect_ts_import_ast(&syn::Type, ...)` that recurses on `Type::Path` segments, skips containers and primitives (mirroring `KNOWN_CONTAINERS` and `is_prelude_scalar`), and pushes leaf idents.
3. Update the call sites in `transport.rs:101-119` and `ts_client.rs` (the parallel block) to pass `f.return_type_ast` and `p.ty_ast` instead of the rendered strings.

This mirrors the OF-008/OF-010 migration on the Rust side — same shape of fix, same breaking-internal-API caveat. Tests follow the OF-008 `test_collect_type_import_matrix` template.

### Direction B: Mark obsolete; close as wontfix once OF-015 ships

[OF-014](./OF-014-redesign-ts-bindings-pipeline.md)'s hybrid pipeline (`ts_bindings.rs` for schema-known types + `ts_sidecar.rs` for the long tail) populates `bindings.ts` end-to-end so the fallback path is unreachable in the happy case. The consumer's hit was while running *without* the side-car. Once [OF-015](./OF-015-productionize-typescript-generation.md) ships and the side-car is the documented default, the fallback path's only role is "you misconfigured the pipeline" — and OF-015 is considering hard-erroring instead of warning at that point.

If OF-015 lands on "promote the OF-006 warning to a hard error" or "remove the fallback emitter entirely," this ticket closes naturally: the broken tokenizer goes away with the code path that exercises it.

### Recommendation

Hold this open until OF-015 makes its decision on the fallback's fate. If OF-015 keeps the fallback path as belt-and-braces, ship Direction A. If OF-015 removes or hard-errors the fallback, close as superseded.

## Effort

- Direction A: small-to-medium (similar shape to OF-008/10).
- Direction B: zero (closes naturally on OF-015 landing).

## Tests

(if Direction A is chosen)

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

- **Should the AST walker be shared between Rust-side and TS-side collection?** The "skip containers, recurse into args, collect leaf idents" loop is identical; the only thing that differs is which set of leaf idents we want (Rust skips `String`, TS also skips `String` → emits `string`). A shared helper parameterized by a "skip set" feels right but should wait until the second use case actually exists.
- **`Cow<'_, T>` / `Box<T>` in return types?** `KNOWN_CONTAINERS` already includes both for the Rust side. The TS side needs them in its skip-set too.
- **Tuple return types?** `(A, B)` doesn't have a clean TS rendering today (`rust_type_to_ts` doesn't handle them). Out of scope for this ticket; the consumer didn't hit it.
- **Dependency on OF-015's decision.** This is the load-bearing open question. Don't ship Direction A until OF-015 confirms the fallback path stays.

## Notes

- The consumer's verbatim entry under their "OF-015" includes the rendered TS that breaks `eslint`/`prettier`. Reproduced fresh on `168ff379`.
- The Pumice workaround (`NotificationPrefsMap` wrapper) is the same shape as OF-010's pre-fix workaround. Listed in the OF-014 spike outcome as "kept due to TS emitter bug — see OF-015" → that "OF-015" reference is to the consumer's numbering, i.e., this ticket.
- If we go with Direction A, this ticket bundles cleanly with any future "AST-ify the rest of the TS emitter" pass. Worth grepping for other places `rust_type_to_ts` is called against rendered strings — there are at least two (transport.rs and ts_client.rs); confirm the count before scoping.
