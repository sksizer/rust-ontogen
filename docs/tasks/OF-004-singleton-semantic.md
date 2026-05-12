# OF-004 - First-class singleton-module semantic for downstream generators

- **Severity:** Low (today), Medium (once admin UI is generated)
- **Status:** Open
- **Source:** [feedback.md OF-004](../feedback.md)
- **Related:** [OF-002](./OF-002-singleton-url-pluralization.md)

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
