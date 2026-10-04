---
type: backlog
schema_version: '2'
id: B-FQNC
tags:
- servers
- clients
- filters
last_reviewed: '2026-10-04'
---

# Classify a list's filter struct by its type, not its name

`Param::is_filter_struct` in `src/servers/parse.rs` treats any parameter whose type name ends in `Query` as a list's `*Query` filter struct, whose fields are the `filter[…]` members (wire contract §7.3). The test is the name alone. A list parameter of another kind whose name happens to end in `Query`, such as a unit enum `SearchQuery` or a newtype `RawQuery(String)`, is classified as a filter struct. On HTTP its spec then accepts no member: `filter_fields::<T>()` finds no fields, so every `filter[…]` key is refused as unknown, and the value can never be sent. A bare filter it should have been, `filter[search_query]`, is never offered.

The clients stage already resolves a filter struct through its type pool (`Config.required_query_structs`, from the scanned TS bindings), and the servers stage scans the schema and API files.

To close it:

- Classify a parameter as a filter struct only when its type resolves, through the scanned type pool or the API file that declares it, to a struct with named fields. Keep the `Query` suffix as a convention for documentation, not as the test.
- Treat any other type, whatever its name, as a bare filter, subject to the existing bare-filter checks (one value must carry it).
- Make the servers and clients stages agree, so the TS transport sends the same members the server reads.
- Cover a unit enum and a newtype named `…Query` in the servers and clients tests, and a struct not named `…Query` if the classification admits it.
- Related: B-FQRS, which resolves the same struct's import path from its module.
