---
type: backlog
schema_version: '2'
id: B-PNHJ
tags:
- pagination
- servers
- api
last_reviewed: '2026-10-04'
---

# Support filtered + paginated lists with a filter-aware count

> **Closed (done).** Lifted by https://github.com/sksizer/rust-ontogen/pull/172 (a paginated list may take filter parameters, and `count` takes the same filter) and by E0004 phase 2 (https://github.com/sksizer/rust-ontogen/pull/202): a hand-written `list` or `count` replaces the generated one in a module named after an entity, and a filtered list is served as JSON:API with `meta.total` from that count.
>
> What differs from the bullets below:
>
> - The filter-aware `count` is hand-written beside the hand-written `list`. The store has no filter, so `gen_api` cannot generate it. A paginated module whose hand-written `list` has no `count` is a build error.
> - Forwarding the filter to `list` and `count` on IPC and MCP was done by #172. Phase 2 added HTTP and the TS clients (`filter[…]` on the wire), not those two transports.
> - The rejection in `check_paginated_lists` was lifted by #172, replaced by the requirement above.

Original request:

A paginated `list` that also takes a query struct or a scoped filter (e.g. `skill_id: &str`) is currently rejected by `check_paginated_lists` (PR #159 review fix, commit 94d21a0): `count(store)` takes no filter, so the total would be the whole table and a client paginator would show empty pages.

To lift the rejection:

- Generate a filter-aware `count(store, <same filter params as list>)` for paginated entities (API generator, both store backends).
- Have the HTTP, IPC and MCP page handlers forward the query/plain filter params to both `list` and `count`.
- Remove the 'may take nothing but limit/offset' check once the total is correct.

The client side already anticipates this shape: the TS transport signature is `(query?, limit?, offset?)` and the admin registry emits `listHasQuery` so the Nuxt admin layer knows where the page goes.
