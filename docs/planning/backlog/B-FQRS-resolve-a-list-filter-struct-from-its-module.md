---
type: backlog
schema_version: '2'
id: B-FQRS
tags:
- servers
- api
- filters
last_reviewed: '2026-10-04'
---

# Resolve a list's filter struct from the module that declares it

The generated HTTP and MCP handlers name every type an API function takes through `types_import_path`, so a hand-written list's `*Query` filter struct must be reachable there. A struct declared beside its `list` in `src/api/v1/task.rs` fails to build (`unresolved import crate::schema::ListTasksQuery`) until the schema module re-exports it (`pub use crate::api::v1::task::ListTasksQuery;`). The tasks-tracker example and markdown-pilot carry that re-export, and the cookbook's "Filtered lists" recipe tells consumers to add it.

The scanner already has what it needs to do better: it parses the file that declares the `list` and that file's `use` items, as it does to resolve an op's error type (`resolve_through_uses` in `src/servers/parse.rs`).

To retire the re-export:

- When a list's filter struct is declared in the scanned API file, import it from that module (`{service_import_path}::{module}::ListTasksQuery`) in the generated HTTP and MCP files; when the file `use`s it from elsewhere, import it through that path.
- Keep `types_import_path` for types the file neither declares nor imports.
- Drop the re-exports from tasks-tracker and markdown-pilot, and the re-export step from the cookbook recipe and the wire contract's §7.3.
- Cover a struct declared in the API file, one imported by a `use`, and one reached through `types_import_path` in the servers tests.
