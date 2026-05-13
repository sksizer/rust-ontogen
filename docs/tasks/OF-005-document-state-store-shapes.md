---
status: closed
resolution: fixed
resolution_date: 2026-05-12
resolution_commit: 919b74a
---
# OF-005 - Document accepted `state_type` / `store_type` first-param shapes

- **Severity:** Medium
- **Status:** Resolved (`919b74a`, 2026-05-12)
- **Source:** [feedback.md OF-005](2026-05-12-pumice.md)
- **Related:** [OF-001](./OF-001-parser-skip-diagnostic.md) (shipped together)

## Resolution

Shipped in `919b74a` on 2026-05-12 alongside OF-001.

The "this works / this doesn't" table now lives in
`site/src/content/docs/guides/api-layer.mdx` under
["Service functions: accepted signatures"](https://ontogen.dev/guides/api-layer/#service-functions-accepted-signatures),
followed by a "Build-time skip warnings" section that documents the exact
`cargo:warning=` wording from OF-001. The cookbook page
`cookbook/custom-api-endpoints.mdx` and the reference page
`reference/configuration.mdx` cross-link to that anchor.

Every row of the table is pinned to a runtime test in
`src/servers/tests.rs::test_of005_table_accepted_rows`,
`test_of005_table_rejected_rows`, and
`test_of005_table_store_substring_false_positive`, so the docs cannot drift
from behaviour silently.

The `&StoreContext` footgun (substring false-positive accept) is documented
in the table and pinned by a test. Adding an info-level warning for that case
was discussed during implementation and explicitly left out of scope to avoid
warning-channel noise on legitimate names like `StoreManager`. Reconsider as
a follow-up if a real user is bitten by it.

---

*The remainder of this document is preserved as a record of the original analysis.*

## Problem

The parser's substring-match rule (see OF-001) means a function taking `&Arc<PumiceState>` works but `State<'_, Arc<PumiceState>>` does not, and `State<'_, Mutex<AppState>>` is silently invisible. These constraints are not documented in user-facing docs - the only authoritative source is reading `parse.rs`.

## Location

- Behavior lives in `src/servers/parse.rs:107-110` (substring match on the normalized first-param type string).
- No user-facing documentation in the site (`site/src/content/docs/`) or the project README covers what first-param shapes are accepted.

## Proposed resolution

Add a "Service Functions: Accepted Signatures" section to the docs (good fit under `site/src/content/docs/guides/api-layer.mdx` or a new reference page). Include a "this works / this doesn't" table:

| First param                                     | Accepted | Why                                  |
| ----------------------------------------------- | -------- | ------------------------------------ |
| `&PumiceState`                                  | Yes      | Contains state_type substring        |
| `state: &PumiceState`                           | Yes      | Same                                 |
| `&Arc<PumiceState>`                             | Yes      | Contains state_type substring        |
| `State<'_, Arc<PumiceState>>` (Tauri)           | No       | Substring matches, but parser sees a different first-param shape - verify and document |
| `State<'_, Mutex<AppState>>`                    | No       | state_type not substring             |
| `&Store`                                        | Yes (if store_type configured) | Contains store_type substring |
| (no params)                                     | No       | See [OF-007](./OF-007-support-stateless-fns.md) |

Pair the docs page with the build-time diagnostic from [OF-001](./OF-001-parser-skip-diagnostic.md) so users hit the same explanation from two directions.

## Effort

Small. Mostly documentation. Verify each row by writing a unit test before publishing the table.

## Notes

- Once OF-001 ships, link the cargo warning at the docs page anchor.
- Worth adding the table to the build-script setup guide too, since that's where `state_type` is configured.
