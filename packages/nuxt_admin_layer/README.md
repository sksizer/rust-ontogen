# @osdlc/ontogen-admin-layer

A Nuxt layer that renders a CRUD admin UI (list, detail, create and edit pages) from the admin registry that [ontogen](https://github.com/sksizer/rust-ontogen) generates.

This package is released together with the `ontogen` crate and shares its version. Install it and the registry types at the version that matches your `ontogen` dependency:

```bash
pnpm add @osdlc/ontogen-admin-layer@0.10 @osdlc/ontogen-admin-types@0.10
```

Then extend it in `nuxt.config.ts`:

```typescript
export default defineNuxtConfig({
  extends: ['@osdlc/ontogen-admin-layer'],
})
```

The layer expects:

- the generated registry at `app/admin/generated/admin-registry.ts`, or at the path set in `ontogenAdmin.registry`;
- a `useTransport()` composable in your app that returns the generated transport.

See the [client generation guide](https://github.com/sksizer/rust-ontogen/blob/main/site/src/content/docs/guides/client-generation.mdx#the-nuxt-admin-layer) for details.
