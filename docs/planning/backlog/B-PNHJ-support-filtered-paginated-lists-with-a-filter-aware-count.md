---
type: backlog
schema_version: '2'
id: B-PNHJ
tags:
- pagination
- servers
- api
last_reviewed: '2026-09-22'
---

# Support filtered + paginated lists with a filter-aware count

A paginated `list` that also takes a query struct or a scoped filter (e.g. `skill_id: &str`) is currently rejected by `check_paginated_lists` (PR #159 review fix, commit 94d21a0): `count(store)` takes no filter, so the total would be the whole table and a client paginator would show empty pages.

To lift the rejection:

- Generate a filter-aware `count(store, <same filter params as list>)` for paginated entities (API generator, both store backends).
- Have the HTTP, IPC and MCP page handlers forward the query/plain filter params to both `list` and `count`.
- Remove the 'may take nothing but limit/offset' check once the total is correct.

The client side already anticipates this shape: the TS transport signature is `(query?, limit?, offset?)` and the admin registry emits `listHasQuery` so the Nuxt admin layer knows where the page goes.
