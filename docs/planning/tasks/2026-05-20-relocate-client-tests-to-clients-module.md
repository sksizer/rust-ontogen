---
type: task
schema_version: '3'
status: planning/needs-definition
created: '2026-05-20'
impact: low
complexity: small
tags:
- clients
- refactor
- tests
related:
- 2026-05-19-split-clients-from-servers
definition_gap: 'v2-to-v3 migration flagged ambiguous touchpoint row(s): row ``src/servers/tests.rs``
  carries removal/rename language (''remove'') — v3 kind cannot be inferred mechanically'
---
# Relocate client-side tests from src/servers/tests.rs to src/clients/tests.rs with shared fixtures

_Auto-generated from a /sdlc:task-work post-mortem. Review and
promote to `open/ready` before picking up._

## Goal

[[2026-05-19-split-clients-from-servers]] promoted client SDK generation to a
sibling `clients` module but deliberately kept the client-side tests in
`src/servers/tests.rs` (with a stub `src/clients/tests.rs`) to avoid
duplicating shared test fixtures (`make_crud_module`, `make_junction_module`,
etc.). That deviation from the spec's "Files to touch" was reasonable in
isolation, but the end state — client tests living under the servers test
module — undermines the split's mental-model payoff. Finish the relocation by
extracting the shared fixtures into a `#[cfg(test)]` module both test files
can import, then moving the client tests to `src/clients/tests.rs`.

## Today

Per the post-mortem of [[2026-05-19-split-clients-from-servers]]:

> Test relocation deferred. Spec's Files-to-touch said "relocate the
> client-side test cases to `src/clients/tests.rs` (new file)". Sub-agent
> created the file as a stub but kept tests in `src/servers/tests.rs` with
> new helpers; reasoning (avoid duplicating shared test fixtures) is sound
> but the deviation should have come back as an explicit ask rather than a
> unilateral call.

State on the `feat/2026-05-19-split-clients-from-servers` branch:
`src/clients/tests.rs` exists but is a stub; `src/servers/tests.rs` still
contains every client-side test plus the fixture builders both sides need.

## Proposed

Shared test fixtures live in a `#[cfg(test)]` module — e.g.
`src/test_support/mod.rs` (gated by `#[cfg(test)]`) or a child module
`src/servers/test_fixtures.rs` re-exported via `pub(crate)`. Both
`src/servers/tests.rs` and `src/clients/tests.rs` import from this shared
spot. Client-side tests (the ones exercising `HttpTs`, `HttpTauriIpcSplit`,
`AdminRegistry`, schema-known + long-tail TS emission) move to
`src/clients/tests.rs`. Server-side tests (Axum / Tauri IPC / MCP) stay in
`src/servers/tests.rs`. `cargo test` continues to pass with no behaviour
change in test coverage.

## Approach

1. Audit `src/servers/tests.rs` and partition test items into server-side vs
   client-side. Identify the fixture helpers (`make_crud_module`,
   `make_junction_module`, any sibling builders) that both sides need.
2. Extract the shared fixtures into a `#[cfg(test)]` module. Two reasonable
   homes: `src/test_support.rs` (`#[cfg(test)] mod test_support;` in `lib.rs`)
   or a `pub(crate) #[cfg(test)]` child of an existing module. Pick whichever
   matches the project's idioms.
3. Move client-side tests from `src/servers/tests.rs` to
   `src/clients/tests.rs`, updating imports to pull fixtures from the new
   shared module.
4. Run `just full-check` and confirm `cargo test` still passes with the
   same test count.

## Files to touch

| Location | Kind | Change |
|---|---|---|
| `src/servers/tests.rs` | modify | remove client-side test cases; update imports for |
| `src/clients/tests.rs` | modify | replace the stub with the relocated client-side |
| `src/test_support.rs` | new | extracted shared |
| `src/lib.rs` | new | register the new `#[cfg(test)] mod test_support;` if that |


## Acceptance criteria

- [ ] AC-1: `src/clients/tests.rs` contains the client-side test cases (TS
  bindings, transport, admin registry); `src/servers/tests.rs` contains only
  Rust transport tests (Axum, Tauri IPC, MCP).
- [ ] AC-2: Shared fixture builders are defined in exactly one place, not
  duplicated between the two test files.
- [ ] AC-3: `just full-check` passes; `cargo test` test count is unchanged
  from the post-split baseline (206 lib tests + 2 builder integration tests
  + 10 doctests, per the originating task's post-mortem AC-11 evidence).

## Out of scope

- Renaming or restructuring the snapshot files under `src/snapshots/`. Names
  refer to generated output, not module paths; the split already left them
  unchanged.
- Adding new test cases or coverage. Pure relocation.

## Dependencies

- [[2026-05-19-split-clients-from-servers]] must be merged first so this
  task operates on the post-split file layout.

## Discovery context

Spawned by /sdlc:task-work post-mortem of [[2026-05-19-split-clients-from-servers]] on 2026-05-20.
