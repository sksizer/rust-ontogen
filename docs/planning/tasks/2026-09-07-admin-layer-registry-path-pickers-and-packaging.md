---
type: task
schema_version: '3'
status: in-progress
created: '2026-09-07'
last_reviewed: '2026-09-20'
impact: medium
complexity: medium
tags:
- admin-layer
- packaging
- dx
related:
- 2026-09-07-schema-enums-labels-and-id-type.md
relevance_note: Part (a) shipped via #157; the rest is in review as #158. Spec ported from the dev monorepo, where it was authored as T-43C1.
---
# Admin layer: registry path option, relation pickers, enum selects, detail-link hook, packable tgz

## Goal

`@ontogen/admin-layer` and `@ontogen/admin-types` are close to what a host
application needs from a generated admin browser, with five gaps: the registry
path is hard-coded, a relation field is a bare id text box, an enum select
needs the values the schema-enums work emits, a detail page has no hook for a
host to add its own link, and the layer's dependency on `admin-types` is
`workspace:*`, which bun cannot resolve from a git install of a subdirectory.

## Today

- `packages/nuxt_admin_layer/modules/admin.ts` — sets the `#admin-registry`
  alias to `<rootDir>/app/admin/generated/admin-registry`; no option.
- `packages/nuxt_admin_layer/app/components/AdminFormField.vue` —
  `type === 'relation'` renders a text input with placeholder `<relationTo> ID`;
  `type === 'enum'` renders a `<select>` over `field.enumValues`.
- `packages/nuxt_admin_layer/app/pages/admin/[entity]/[id]/index.vue` — the
  detail page; links only to the list and the edit page, plus relation links
  through `resolveRelationRoute`.
- `packages/nuxt_admin_layer/app/composables/useAdminEntity.ts` — calls
  `transport[config.listMethod]` for lists; the pattern a relation picker
  reuses for the target entity.
- `packages/nuxt_admin_layer/package.json` —
  `peerDependencies["@ontogen/admin-types"] = "*"`,
  `devDependencies["@ontogen/admin-types"] = "workspace:*"`; `files` already
  lists what a pack ships.
- `packages/admin-types/package.json` — version `0.1.0`, `main: ./index.ts`.
- `justfile` — no pack recipe; `pnpm-workspace.yaml` lists `packages/*`.

## Proposed

Five changes, one per letter, splittable into separate PRs:

- (a) `modules/admin.ts` reads `nuxt.options.ontogenAdmin?.registry` (a path
  relative to `rootDir`) and falls back to today's default.
- (b) `AdminFormField.vue` renders a relation field as a picker: it resolves
  `field.relationTo` through `useAdminRegistry()`, calls the target's
  `listMethod` on `useTransport()` (page 1, `defaultLimit` when paginated), and
  offers rows as `id — <first prominent field>` with type-to-filter over the
  loaded rows. The raw id stays editable behind a toggle.
- (c) The enum `<select>` is used only when `field.enumValues` is non-empty;
  otherwise the field falls back to a text input, so a registry generated
  before the schema-enums work still renders.
- (d) `[id]/index.vue` injects `ADMIN_DETAIL_LINK` (an
  `InjectionKey<(entity: string, id: string) => { to: string; label: string } | null>`),
  exported from the layer, and renders the returned link in the header when a
  host provides one.
- (e) `@ontogen/admin-types` is pinned by version (`^0.1.0` peer, `0.1.0` dev)
  so a packed layer resolves it from the registry or a sibling tgz. A `justfile`
  recipe `pack-admin` runs `bun pm pack` in each package into `dist/` and prints
  the tgz paths.

## Approach

1. (a) In `packages/nuxt_admin_layer/modules/admin.ts`, add the option with
   `defineNuxtModule`'s `options` and read `nuxt.options.ontogenAdmin.registry`;
   document it in `nuxt.config.ts`'s header comment.
2. (b) Add `packages/nuxt_admin_layer/app/components/AdminRelationPicker.vue`
   and use it from `AdminFormField.vue` for `type === 'relation'`. It takes
   `relationTo` and `modelValue`, lists through the target entity's
   `listMethod`, filters client-side, and emits the chosen id.
3. (c) Guard the enum branch on `field.enumValues?.length`.
4. (d) Export `ADMIN_DETAIL_LINK` from
   `packages/nuxt_admin_layer/app/composables/useAdminDetailLink.ts`; inject it
   in `[id]/index.vue`.
5. (e) Pin `@ontogen/admin-types` in
   `packages/nuxt_admin_layer/package.json`; add the `pack-admin` recipe to
   `justfile`; bump both packages to `0.2.0`.
6. Run the pack; inspect `package.json` inside the layer tgz and confirm no
   `workspace:` protocol survives. If one does, rewrite it in the recipe before
   packing.

## Files to touch

`packages/nuxt_admin_layer/modules/admin.ts`,
`packages/nuxt_admin_layer/nuxt.config.ts`,
`packages/nuxt_admin_layer/app/components/AdminFormField.vue`,
`packages/nuxt_admin_layer/app/components/AdminRelationPicker.vue` (new),
`packages/nuxt_admin_layer/app/composables/useAdminDetailLink.ts` (new),
`packages/nuxt_admin_layer/app/pages/admin/[entity]/[id]/index.vue`,
`packages/nuxt_admin_layer/package.json`, `packages/admin-types/package.json`,
`justfile`, `CHANGELOG.md`.

## Acceptance criteria

- [ ] AC-1: A consuming `nuxt.config.ts` with
      `ontogenAdmin: { registry: 'app/admin/generated/admin-registry' }` and one
      without both build; the alias resolves to the configured path in the first
      case and to the default in the second.
- [ ] AC-2: A relation field in the edit form lists the target entity's rows
      through its `listMethod`, narrows as the user types, and writes the chosen
      id to the form model; the id stays editable behind a toggle.
- [ ] AC-3: A registry entry with `enumValues: []` renders a text input; one
      with values renders a `<select>` with those values.
- [ ] AC-4: A host that provides `ADMIN_DETAIL_LINK` sees its link in the detail
      page header; a host that does not sees no extra element.
- [ ] AC-5: `just pack-admin` produces two tgz files and
      `tar -xOf <layer tgz> package/package.json` contains no `workspace:`
      string.

## Out of scope

- Anything a host does with the layer: extending it, the registry output path,
  layout overrides.

## Dependencies

- [Enum variants, field labels and the id type](./2026-09-07-schema-enums-labels-and-id-type.md)
  — the enum select only has values once the registry emits `enumValues`; (c)
  is written to work without it.
