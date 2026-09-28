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
  // The generated transport fixture (tests/fixtures) imports Tauri's API,
  // which this package does not install; the stubs stand in for it.
  resolve: {
    alias: {
      '@tauri-apps/api/core': fileURLToPath(new URL('./tests/stubs/tauri-core.ts', import.meta.url)),
      '@tauri-apps/api/event': fileURLToPath(new URL('./tests/stubs/tauri-event.ts', import.meta.url)),
    },
  },
  test: {
    environment: 'happy-dom',
    setupFiles: [fileURLToPath(new URL('./tests/setup.ts', import.meta.url))],
    include: ['tests/**/*.test.ts'],
  },
})
