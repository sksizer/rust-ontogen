---
type: backlog
schema_version: '2'
id: B-HTRP
tags:
- clients
- http
- route-prefix
last_reviewed: '2026-10-04'
---

# Support `route_prefix` in the `HttpTs` client

The `HttpTs` client (`httpCommands`) ignores `route_prefix`: its methods take no `projectId` parameter and build no scoped path. Against a server generated with a route prefix every call misses, because the server only serves the scoped routes. Only the `HttpTauriIpcSplit` transport supports prefixes today.

To support it:

- Add a trailing optional prefix parameter to each `httpCommands` method, the same shape the split transport uses (`projectId?: string`), and build the path with the same `scopedPath` helper.
- Junction ops need no special case: scoped routes have their unscoped shape under the prefix, so a junction method calls the same path through `scopedPath`, as the split transport does. In a module named after an entity that path is the relationship endpoint (`…/tasks/{id}/relationships/labels`); in any other module it is `{base}/{parent_id}/{segment}`.
- Add a client test against a scoped example server covering a list, a get, a create, a scoped relationship add, and a scoped junction add in a module with no entity.

Until then, an app that needs a route prefix generates the `HttpTauriIpcSplit` transport.
