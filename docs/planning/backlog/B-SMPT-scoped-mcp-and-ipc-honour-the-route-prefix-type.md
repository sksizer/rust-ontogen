---
type: backlog
schema_version: '2'
id: B-SMPT
tags:
- servers
- mcp
- ipc
- route-prefix
last_reviewed: '2026-10-04'
---

# Scoped MCP and IPC honour the route prefix type

A scoped HTTP router reads its prefix parameter as `RoutePrefix.params[0].rust_type` (`String`, `uuid::Uuid`, or any type the accessor takes). The MCP and IPC generators ignore that type.

- **MCP.** `mcp_handler_prefix` and `mcp_store_construction` in `src/servers/generators/mcp.rs` always parse the argument with `uuid::Uuid::parse_str` and pass `&uuid` to the accessor, and each scoped tool's input schema declares the argument as `"format": "uuid"`.
- **IPC.** `prefix_param_line`, `prefix_validation_line` and `store_construction_line` in `src/servers/generators/ipc.rs` always take the parameter as `Option<String>`, parse it with `uuid::Uuid::parse_str` and pass `&ontogen_uuid` to the accessor.

A prefix whose parameter is not a UUID therefore generates MCP and IPC code that does not compile, or a tool or command that refuses every valid scope. The TS IPC transport sends the scope as the prefix's `ts_type`, so the command's parameter type should follow the prefix too.

markdown-pilot shows the gap: its scoped HTTP router uses a `String` `project_id`, and `crates/markdown-pilot/build.rs` generates MCP unscoped only, with a comment saying why ("a scoped tool parses its scope argument as a UUID, and the pilot's one project is a name").

A related gap: a scoped MCP tool reads the scope from its arguments and reads an `*Input` or `*Query` struct from the arguments without it. A field of that struct named like the prefix parameter is never read. Generation refuses an op argument named like the prefix parameter, but cannot see struct fields without their types.

To close it:

- Read the scope argument (MCP) and take the scope parameter (IPC) as the prefix parameter's `rust_type`, still optional, and pass it to the accessor the way the HTTP generator does, borrowing a `String` as `&str`.
- Derive the MCP argument's schema from that type: `"format": "uuid"` only for `uuid::Uuid`.
- Report a scope argument of the wrong type as `Invalid parameter {name}: …`, as other typed arguments are.
- Generate scoped MCP in markdown-pilot (a `String` scope) and drive it from `tests/mcp_tools.rs`, then drop the comment in `build.rs`. Cover a `String` scope on IPC in the servers tests.
- Refuse a struct field named like the prefix parameter once the struct's fields are known to generation (see B-FQNC).
- Keep the existing `uuid::Uuid` case covered in the servers tests.
