---
status: closed/done
completion_note: "Shipped in 2804753 on 2026-05-13."
---
# OF-009 - Document or default-override cruet mass-noun singularization

- **Severity:** Low
- **Status:** Closed (docs-only)
- **Source:** [feedback.md OF-009](2026-05-12-pumice.md)

## Resolution (2026-05-13)

Took option (1) only -- documentation. The override mechanism (`NamingConfig::singular_overrides` / `plural_overrides`) already exists and works as documented; the friction was that users only discovered the issue *after* shipping awkward command names. Option (2) (curated default overrides) was held back per the task's own recommendation: only ship built-in overrides if the same landmines hit multiple downstreams, which hasn't happened.

### Verified cruet behaviour

Tested directly against `cruet 0.14` (the version pulled by ontogen's lockfile):

| Module name  | cruet singular | cruet plural | Misfire?                                                                            |
| ------------ | -------------- | ------------ | ----------------------------------------------------------------------------------- |
| `data`       | `datum`        | `data`       | Yes -- Latin singular; English treats `data` as mass.                               |
| `metadata`   | `metadatum`    | `metadata`   | Yes -- same pattern as `data`.                                                      |
| `settings`   | `setting`      | `settings`   | Yes -- stem-strips the `s`; `settings` is plural-tantum.                            |
| `media`      | `medium`       | `medias`     | Yes -- both directions wrong; `medium` is a different sense, `medias` isn't a word. |
| `information`| `information`  | `information`| No -- mass noun, correctly preserved.                                               |
| `news`       | `news`         | `news`       | No -- correctly preserved.                                                          |
| `evidence`   | `evidence`     | `evidence`   | No -- correctly preserved (this is the example case in existing docs).              |
| `series`     | `series`       | `series`     | No -- correctly preserved.                                                          |
| `schema`     | `schema`       | `schemas`    | No -- `schemas` is idiomatic English (vs. Greek-strict `schemata`).                 |

This tightened the original feedback table -- `information` is *not* actually a misfire (cruet preserves it), and `metadata` is a misfire (same Latin-plural-tantum shape as `data`).

### Where it's documented

- **In source.** Extended the `NamingConfig` doc-comment at `src/servers/types.rs` with a "Pitfall: mass nouns and Latin plural-tantums" section -- verified misfire table, the "already-correct" list, and a `singular_overrides` / `plural_overrides` snippet. `rustdoc` picks this up so `cargo doc` users see it too.
- **In site reference.** Added an `<Aside type="caution" title="Mass nouns and Latin plural-tantums">` in `site/src/content/docs/reference/configuration.mdx` directly below the existing `NamingConfig` example snippet, with the same verified table and the same override snippet. This is the natural reading location -- anyone scanning the `NamingConfig` field table hits the warning right after the existing `cruet`-uses-Rails-style line.

### Out of scope (deliberately)

- **No built-in overrides constant.** Per the original task's recommendation, ship docs first; only adopt curated default overrides if multiple downstreams keep hitting the same landmines. The override mechanism is already discoverable from the warning, and users typically need only 1-2 entries.
- **No build-script setup guide changes.** That guide is about pipeline wiring (which stages, what order); a per-noun pitfall isn't a wiring concern. The two homes above are where someone configuring naming would actually be looking.

## Original analysis (preserved for context)

### Problem

cruet applies Rails-style inflection that treats Latin pluralizations literally. Mass nouns and plural-tantums produce awkward command names by default:

| Module       | cruet singular | Emitted command           |
| ------------ | -------------- | ------------------------- |
| `data`       | `datum`        | `datumBackup()`           |
| `settings`   | `setting`      | (uneven; loses the "s")   |
| `information`| `information`  | (mass noun, varies)       |
| `media`      | `medium`       | `mediumUpload()`          |

The override path already works (`NamingConfig::singular_overrides` / `plural_overrides`). The friction is that users only discover the issue *after* shipping awkward command names.

### Location

- `src/servers/types.rs:220-225` (`NamingConfig::url_singular`).
- `src/servers/types.rs:206-214` (`NamingConfig::module_plural`).

### Proposed resolution

Two non-exclusive options:

1. **Documentation.** Add a "Mass nouns and plural-tantums" callout to the `NamingConfig` doc-comment and to the build-script setup guide. List the common landmines (`data`, `settings`, `information`, `media`, `news`) and show the override snippet.

2. **Curated default overrides.** Ship a small built-in override set covering the common cases above. Users can still override on top. Keep the set small and conservative - cruet's behaviour is otherwise generally desirable.

Recommendation: (1) for sure; (2) only if user feedback shows the same landmines hitting multiple downstreams.

### Effort

Very small. Pure documentation, plus an optional ~10-20 LOC default-overrides constant if (2) is adopted.

### Notes

- This is the lowest-value item in the backlog. The override mechanism already exists and works as documented. Worth fixing only because it's a sharp edge for new users.
