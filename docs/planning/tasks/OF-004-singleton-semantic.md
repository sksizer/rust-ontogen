---
schema_version: '0'
status: closed/done
completion_note: "Shipped in d770838 on 2026-05-12."
---
# OF-004 - First-class singleton-module semantic for downstream generators

- **Severity:** Low (today), Medium (once admin UI is generated)
- **Status:** Resolved (`d770838`, 2026-05-12)
- **Source:** [feedback.md OF-004](2026-05-12-pumice.md)
- **Related:** [OF-002](./OF-002-singleton-url-pluralization.md) (shipped together)

## Resolution

Shipped in `d770838` on 2026-05-12, folded into the OF-002 change. `ApiModule` gains a `pub is_singleton: bool` field that downstream generators (`http.rs`, `transport.rs`, future `admin.rs`, future `doc-gen`) read directly instead of re-deriving the classification from naming rules.

Two declaration paths feed the bit:

- **Source-side:** `// ontogen:singleton` / `//! ontogen:singleton` marker - parsed during `parse_api_module` via a shared `has_top_level_marker(source, name)` helper (factored out of OF-012's `has_skip_marker`).
- **Config-side:** `NamingConfig::singleton_modules: HashSet<String>` - applied as a post-parse overlay (`apply_singleton_overlay`) that ORs config entries onto the parsed IR before any generator runs.

The bit currently drives one user-visible behaviour: HTTP / TS-transport URL routing through `NamingConfig::url_for_module(&ApiModule)` (singletons get singular kebab-case, entities get pluralized kebab-case). Future generators can branch on `module.is_singleton` for richer behaviour (e.g. admin layer exposing standalone "single config screen" routes instead of list-detail pairs).

Boolean rather than `ModuleKind` enum: this was a deliberate YAGNI call. The boolean satisfies OF-002's URL fix and OF-004's IR-signal requirement. If more module kinds emerge (junction, event-stream, etc.) the field can be widened later; today's bool is consistent with the existing `first_param_is_store: bool` precedent on `ApiFn`.

Test coverage is documented in [OF-002's Resolution section](./OF-002-singleton-url-pluralization.md#resolution). The end-to-end HTTP test exercises the IR path from source marker through `is_singleton` to the generated route string.

**Known gap (out of scope here):** `gen_api`'s `ApiOutput` flattening in `src/api/mod.rs` consumes parser output via a separate IR type (`ontogen_core::ir::ApiModule`) that does not currently carry `is_singleton`. The bit is dropped at that boundary. If/when a downstream consumer of `ApiOutput` needs singleton awareness, the parallel IR type and `merge_scanned_module` will need to thread the field through. Noted, deferred.

---

*The remainder of this document is preserved as a record of the original analysis.*

## Problem

Beyond URL pluralization (OF-002), there is no way for a module to declare itself as a singleton. Any future generator that wants to branch on "list view + detail page" (entity) vs "single config screen" (singleton) has no signal to read.

The admin registry emitter (`ClientGenerator::AdminRegistry`) currently includes only entities. If singleton modules become first-class, the admin layer could expose them as standalone screens; today it cannot, because there's nothing to inspect.

## Location

- Same marker introduced for [OF-002](./OF-002-singleton-url-pluralization.md), generalised so all downstream generators (HTTP, admin, future doc-gen) can read it.

## Current behavior

Singletons (autostart, database, vault) and entities (workout, exercise) are indistinguishable to the codegen pipeline. Downstream emitters must guess or hard-code, neither of which scales.

## Proposed resolution

When implementing [OF-002](./OF-002-singleton-url-pluralization.md), surface the singleton flag on `ApiModule` (whether the marker is sourced from `NamingConfig::singleton_modules` or a source-side directive). Downstream generators (`http.rs`, `transport.rs`, `admin.rs`, `mcp.rs`) read the flag on the IR rather than re-deriving from naming rules.

## Effort

Folded into [OF-002](./OF-002-singleton-url-pluralization.md). No additional implementation cost beyond making the flag part of the IR rather than a `NamingConfig`-only concern.

## Notes

- Defer admin-side use until admin generation actually needs it; for now, propagating the flag through the IR is enough.
