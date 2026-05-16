---
status: closed/done
completion_note: "Shipped in d770838 on 2026-05-12."
---
# OF-002 - Singleton module URL pluralization

- **Severity:** Medium
- **Status:** Resolved (`d770838`, 2026-05-12)
- **Source:** [feedback.md OF-002](2026-05-12-pumice.md)
- **Related:** [OF-004](./OF-004-singleton-semantic.md) (shipped together)

## Resolution

Shipped in `d770838` on 2026-05-12, jointly with [OF-004](./OF-004-singleton-semantic.md). Modules can now declare themselves as singletons through either of two mechanisms, both feeding the same `ApiModule::is_singleton` IR bit:

1. **Source-side marker:** `// ontogen:singleton` or `//! ontogen:singleton` in the file's leading comment-and-attribute block (same placement rule as OF-012's `// ontogen:skip`).
2. **Config-side:** `NamingConfig::singleton_modules: HashSet<String>` in `build.rs`. A post-parse overlay ORs config entries onto the parsed IR.

The HTTP and TS-transport generators now route module URL segments through a new `NamingConfig::url_for_module(&ApiModule)` helper: singletons get the singular kebab-case form (`database` -> `database`, `auto_start` -> `auto-start`); entities keep the existing `url_plural` behaviour (`workout` -> `workouts`). The original `url_plural(&str)` is left untouched because internal naming consumers (e.g. `derive_action`) still depend on its plural semantics.

For `database` containing `get_path()`:

```
GET /api/database/path
```

Test coverage in `src/servers/tests.rs` (12 cases): 5 parser cases pin the source-marker grammar, 3 cases pin the config overlay (including idempotency when both declarations are present), 3 cases pin the `url_for_module` matrix, and 1 end-to-end test asserts a synthetic `database.rs` with the marker generates `/api/database/path` rather than `/api/databases/path`.

Site docs: `guides/api-layer.mdx` gains a "Singleton modules: `// ontogen:singleton`" subsection next to the OF-012 skip-marker section; `reference/configuration.mdx` documents the new `singleton_modules` field on `NamingConfig`.

`gen_api`'s `ApiOutput` IR (in `src/api/mod.rs`) consumes parser output through a separate flattening path that does not currently propagate `is_singleton`. That's fine for the present user (HTTP via `generate_transport`); if a future generator consumes `ApiOutput` and needs singleton awareness, the IR type and `merge_scanned_module` will need a parallel change. Noted but out of scope for this fix.

---

*The remainder of this document is preserved as a record of the original analysis.*

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
