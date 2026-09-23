import { fileURLToPath } from 'node:url'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

// Standalone vitest config for this layer: it has no Nuxt build context of its
// own (the layer is only ever built inside a consuming app), so tests import
// its composables/components/pages directly rather than through `@nuxt/test-utils`.
// `@vitejs/plugin-vue` gives real `<script setup>` SFC compilation (macros,
// styles, everything) with no custom loader; `tests/setup.ts` fills in the
// Vue runtime APIs (`ref`, `computed`, ...) that Nuxt's auto-import would
// otherwise inject, since these files reference them as free identifiers.
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'happy-dom',
    setupFiles: [fileURLToPath(new URL('./tests/setup.ts', import.meta.url))],
    include: ['tests/**/*.test.ts'],
  },
})
