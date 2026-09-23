---
type: task
schema_version: '3'
status: backlog
created: '2026-08-04'
last_reviewed: '2026-09-20'
impact: medium
complexity: medium
tags:
- servers
- clients
- dx
relevance_note: No implementation started. Ported from the dev monorepo, where it was captured as backlog item B-O4GF.
---
# Make route shapes explicit, or fail loudly on ambiguous op names

## Goal

Route shape is derived from function-name prefixes and arity, and a wrong name
reshapes routes silently rather than failing the build. Consumers compensate
with long defensive doc headers on every API module explaining the naming rules
they must not break.

## Today

The classifier's implicit rules, as observed by a consumer building a vault
openers surface:

- `add_`/`remove_` with exactly two params, and `list_` with one param,
  silently become junction routes.
- Only `get_`/`list_`/`count_`/`exists_`/`find_`/`is_`/`has_` route as GET.
- POST params partition by `Option`-ness between query and body.
- The module-name echo is stripped: `get_vault_openers` in module `openers`
  emits `GET /api/openers/vault`, not `GET /api/openers/vault-openers`.

None of these fail the build when the author meant something else — they just
produce a different route.

A second, related emitter ergonomics problem: the generated TS type pool is
emitted in alphabetical order, so adding any wire type reshuffles `types.ts`
wholesale. Small API changes produce large, noisy generated diffs.

## Proposed

Explicit route and verb declaration — an attribute or a registration builder —
so the shape is stated rather than inferred. At minimum, a build-time error on
ambiguous or junction-prone shapes instead of silent reshaping: when a name
matches more than one rule, or matches a junction rule by arity alone, name the
function and the two candidate shapes and stop.

`#[ontogen::http::get]` (shipped in `4a2704c`) is the first piece of the
explicit path; this task is about making the implicit path safe or replacing it.

Alongside, give the TS type pool a stable emission order (insertion order, or
per-module grouping) so generated diffs stay proportional to the change.

## Files to touch

`src/servers/classify.rs`, `src/servers/parse.rs`,
`src/clients/generators/ts_bindings.rs`, `src/servers/tests.rs`,
`src/clients/tests.rs`, the site docs' API-layer guide, `CHANGELOG.md`.

## Acceptance criteria

- [ ] AC-1: A function whose name matches two classification rules fails the
      build with an error naming the function and both candidate shapes.
- [ ] AC-2: A function that would become a junction route by arity alone
      requires an explicit declaration, or fails by name.
- [ ] AC-3: Adding one wire type to a schema changes `types.ts` by that type
      alone; the rest of the file is unmoved.

## Out of scope

- Error-status mapping — see the
  [consumer-controlled HTTP errors epic](../epics/http-error-mapping.md).
