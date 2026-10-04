---
type: backlog
schema_version: '2'
id: B-HTRP
tags:
- clients
- http
- route-prefix
last_reviewed: '2026-10-03'
---

# Support `route_prefix` in the `HttpTs` client

The `HttpTs` client (`httpCommands`) ignores `route_prefix`: its methods take no `projectId` parameter and build no scoped path. Against a server generated with a route prefix every call misses, because the server only serves the scoped routes. Only the `HttpTauriIpcSplit` transport supports prefixes today.

To support it:

- Add a trailing optional prefix parameter to each `httpCommands` method, the same shape the split transport uses (`projectId?: string`), and build the path with the same `scopedPath` helper.
- For scoped junction ops, call the action-style route with both arguments in `meta.args` when a prefix value is given, as the split transport does.
- Add a client test against a scoped example server covering a list, a get, a create, and a scoped junction add.

Until then, an app that needs a route prefix generates the `HttpTauriIpcSplit` transport.
