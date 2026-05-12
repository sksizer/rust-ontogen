# OF-002 - Singleton module URL pluralization

- **Severity:** Medium
- **Status:** Open
- **Source:** [feedback.md OF-002](../feedback.md)
- **Related:** [OF-004](./OF-004-singleton-semantic.md)

## Problem

`NamingConfig::url_plural` always pluralizes the module name when building HTTP routes. For modules that represent a singleton (e.g., `database`, `autostart`, `vault`), the result reads as a non-existent collection: `/api/databases/path`, `/api/autostarts/enabled`, `/api/vaults/config`.

## Location

- `src/servers/types.rs:229` (`NamingConfig::url_plural`) - unconditionally calls `module_plural`.
- Consumed by HTTP route generation in `src/servers/generators/http.rs` and `src/servers/generators/transport.rs`.

## Current behavior

For an api module `database` containing `get_path()`:

```
GET /api/databases/path           ← current
GET /api/database/path            ← desired
```

## Proposed resolution

Introduce a "singleton module" marker. Two candidate shapes:

1. **Config-side:** `NamingConfig::singleton_modules: HashSet<String>`. `url_plural` short-circuits to the singular form when the module is in the set.
2. **Source-side:** a file-level marker, e.g., `//! @ontogen(singleton)` at the top of `database.rs`. Parser records the flag on `ApiModule`.

Recommendation: ship (1) first because it lives alongside the existing `*_overrides` maps in `NamingConfig` and requires no parser changes. (2) can follow if source-side declarations turn out to be more ergonomic in practice.

Treat this together with [OF-004](./OF-004-singleton-semantic.md) so the marker is general enough to feed admin / doc generators later.

## Effort

Medium. The naming-config addition is small; touch points across HTTP and transport generators are mechanical but need test coverage for both singleton and plural cases.

## Notes

- Cosmetic for Pumice today because IPC is the primary transport. Becomes user-visible when HTTP transport is exposed externally or documented in admin tooling.
- Decide whether singleton modules should also affect URL singular forms used elsewhere (e.g., the `:id` segment) - typically they would not, because singletons have no id.
