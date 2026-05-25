---
type: task
schema_version: '3'
status: in-progress
created: '2026-05-24'
last_reviewed: '2026-05-25'
impact: low
complexity: small
tags:
- test-ergonomics
- apifn
related:
- 2026-05-24-ontogen-classifier-add-post-attribute-opt-in
readiness_verified_at: '2026-05-25T00:46:13Z'
---
# rust-ontogen: derive Default on ApiFn so new fields don't require updating every test literal

_Auto-generated from a /sdlc:task-work post-mortem. Review and
promote to `open/ready` before picking up._

## Goal

Adding a new field to `ApiFn` in `src/servers/parse.rs` requires updating ~18 struct-literal constructors across `src/servers/tests.rs` (plus any future test fixtures). With `#[derive(Default)]` on `ApiFn` and idiomatic `..Default::default()` in test literals, new fields would be backward-compatible at the call site, and a PR adding a single parser-side flag would no longer touch every fixture. Surfaced by `[[2026-05-24-ontogen-classifier-add-post-attribute-opt-in]]` where adding one `force_post: bool` field required 18 mechanical edits.

## Today

`ApiFn` is the parsed-function metadata struct used across the codebase. Construction sites are spread between `parse.rs` (the real parser path) and `tests.rs` (synthetic fixtures), with the test fixtures using inline struct literals that name every field.

| Location | Role today |
|---|---|
| `src/servers/parse.rs#ApiFn` | The parsed function metadata struct; today every field must be named explicitly at every construction site. |
| `src/servers/tests.rs` | ~17 `ApiFn { ... }` struct-literal fixtures in the unit test suite, each naming every field by hand. |

## Proposed

`ApiFn` derives `Default`. Test fixtures use `..Default::default()` so a new field added to the struct only requires test edits where the new field's value matters; the other fixtures pick up the default automatically. The real parser path in `parse.rs` continues to populate every field explicitly (no semantic change).

## Approach

1. Add `#[derive(Default)]` to `ApiFn` (and `Param` if its `ty_ast: syn::Type` field allows — `syn::Type` should impl Default via `Type::Verbatim(TokenStream::new())` or similar). If `syn::Type` lacks a usable default, document the blocker and stop; we keep the manual constructors and close this task as wontdo.
2. Convert each `ApiFn { ... }` test literal in `src/servers/tests.rs` to `ApiFn { name: ..., /* per-test overrides */, ..Default::default() }`. Each fixture only names the fields it cares about for the test's assertion.
3. Keep `parse.rs`'s real construction site fully explicit — that path is the source of truth for what every field means, and it should remain unambiguous to readers.
4. Run `just full-check` to confirm no test regressed.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `src/servers/parse.rs#ApiFn` | modify | add `#[derive(Default)]`. |
| `src/servers/parse.rs#Param` | modify | add `#[derive(Default)]` if `syn::Type` cooperates. |
| `src/servers/tests.rs` | modify | rewrite `ApiFn { ... }` fixtures to use `..Default::default()`. |

## Acceptance criteria

- [ ] AC-1: `ApiFn` derives `Default` and the existing test suite compiles unchanged (or with mechanical `..Default::default()` insertions, no behavioural diffs).
- [ ] AC-2: Adding a hypothetical new `bool` field to `ApiFn` (try it locally and revert) only requires updating the test fixtures that exercise that field, not every fixture.
- [ ] AC-3: `just full-check` passes.

## Out of scope

- Changing field types on `ApiFn` to make defaults possible (e.g. wrapping `syn::Type` in `Option`). The task assumes `Default` is achievable as-is; if not, close as wontdo.
- Doing the same refactor for unrelated structs in the codebase. Scope is `ApiFn` + `Param`.

## Dependencies

- none

## Discovery context

Spawned by /sdlc:task-work post-mortem of [[2026-05-24-ontogen-classifier-add-post-attribute-opt-in]] on 2026-05-24.

### Dedup search (spawn-from-post-mortem)

Bullet: Adding a field to ApiFn required updating 18 struct-literal call sites. Derive Default on ApiFn and use ..Default::default() in test literals would make field additions backward-compatible.
Keywords searched: backward-compatible, struct-literal, additions, required, updating, literals, default, adding
Excluded: 2026-05-24-ontogen-classifier-add-post-attribute-opt-in
Top candidates (score / status / headline):
  - 42 / closed/done / 2026-05-23-ontogen-ts-serde-default-as-ts-optional — ontogen-ts: render fields with #[serde(default)] as TS-optional (?) to match the wire contract
  - 18 / open/ready / 2026-05-24-ontogen-classifier-reverse-zero-param-default-to-post — ontogen: reverse the zero-user-param classifier default from CustomGet to CustomPost, opt-in to GET via known-read prefixes
  - 15 / closed/done / OF-019-document-side-car-tauri-watcher — OF-019 - Document the OF-014 side-car's three consumer-side gotchas
  - 14 / open/ready / 2026-05-24-ontogen-ts-configurable-string-literal-quote-style — ontogen-ts: make string-literal quote style configurable via EmitConfig (currently always single-quoted)
  - 14 / closed/done / OF-015-productionize-typescript-generation — OF-015 - Replace the TS bindings side-car with `ontogen-ts`
Decision: SPAWNED
