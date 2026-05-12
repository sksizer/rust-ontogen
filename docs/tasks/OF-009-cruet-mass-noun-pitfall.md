# OF-009 - Document or default-override cruet mass-noun singularization

- **Severity:** Low
- **Status:** Open
- **Source:** [feedback.md OF-009](../feedback.md)

## Problem

cruet applies Rails-style inflection that treats Latin pluralizations literally. Mass nouns and plural-tantums produce awkward command names by default:

| Module       | cruet singular | Emitted command           |
| ------------ | -------------- | ------------------------- |
| `data`       | `datum`        | `datumBackup()`           |
| `settings`   | `setting`      | (uneven; loses the "s")   |
| `information`| `information`  | (mass noun, varies)       |
| `media`      | `medium`       | `mediumUpload()`          |

The override path already works (`NamingConfig::singular_overrides` / `plural_overrides`). The friction is that users only discover the issue *after* shipping awkward command names.

## Location

- `src/servers/types.rs:220-225` (`NamingConfig::url_singular`).
- `src/servers/types.rs:206-214` (`NamingConfig::module_plural`).

## Proposed resolution

Two non-exclusive options:

1. **Documentation.** Add a "Mass nouns and plural-tantums" callout to the `NamingConfig` doc-comment and to the build-script setup guide. List the common landmines (`data`, `settings`, `information`, `media`, `news`) and show the override snippet.

2. **Curated default overrides.** Ship a small built-in override set covering the common cases above. Users can still override on top. Keep the set small and conservative - cruet's behaviour is otherwise generally desirable.

Recommendation: (1) for sure; (2) only if user feedback shows the same landmines hitting multiple downstreams.

## Effort

Very small. Pure documentation, plus an optional ~10-20 LOC default-overrides constant if (2) is adopted.

## Notes

- This is the lowest-value item in the backlog. The override mechanism already exists and works as documented. Worth fixing only because it's a sharp edge for new users.
