---
type: backlog
schema_version: '2'
id: B-SMPT
tags:
- servers
- mcp
- route-prefix
last_reviewed: '2026-10-04'
---

# Scoped MCP tools honour the route prefix's parameter type

A scoped HTTP router reads its prefix parameter as `RoutePrefix.params[0].rust_type` (`String`, `uuid::Uuid`, or any type the accessor takes). The MCP generator ignores that type. `mcp_handler_prefix` and `mcp_store_construction` in `src/servers/generators/mcp.rs` always parse the argument with `uuid::Uuid::parse_str` and pass `&uuid` to the accessor, and each scoped tool's input schema declares the argument as `"format": "uuid"`. A prefix whose parameter is not a UUID therefore generates MCP code that does not compile, or a tool that refuses every valid scope.

markdown-pilot shows the gap: its scoped HTTP router uses a `String` `project_id`, and `crates/markdown-pilot/build.rs` generates MCP unscoped only, with a comment saying why ("a scoped tool parses its scope argument as a UUID, and the pilot's one project is a name").

To close it:

- Read the scope argument as the prefix parameter's `rust_type` (deserialize it from the argument value), and pass it to the accessor the way the HTTP generator does, borrowing a `String` as `&str`.
- Derive the argument's schema from that type: `"format": "uuid"` only for `uuid::Uuid`.
- Report a scope argument of the wrong type as `Invalid parameter {name}: …`, as other typed arguments are.
- Generate scoped MCP in markdown-pilot (a `String` scope) and drive it from `tests/mcp_tools.rs`, then drop the comment in `build.rs`.
- Keep the existing `uuid::Uuid` case covered in the servers tests.
