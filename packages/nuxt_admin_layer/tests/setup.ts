/**
 * vitest setupFile (vitest.config.ts): stands in for the auto-imports Nuxt's
 * build pipeline would otherwise inject into every `<script setup>` block in
 * this layer (`ref`, `computed`, ... from `vue`; the `definePageMeta` macro).
 *
 * This layer has no Nuxt app of its own to build inside — it is only ever
 * built as part of a consuming app (see nuxt.config.ts) — so tests import its
 * composables/components/pages directly and need these on `globalThis`
 * themselves. App-specific auto-imports (`useRoute`, `useTransport`,
 * `useAdminRegistry`, `useAdminEntity`, ...) are stubbed per test file
 * instead, since their shape varies per test.
 */
import { computed, isRef, onMounted, ref, watch } from 'vue'

import { useAdminDetailLink } from '../app/composables/useAdminDetailLink'
import { pageArgs } from '../app/utils/pageArgs'

Object.assign(globalThis, {
  ref,
  computed,
  watch,
  isRef,
  onMounted,
  // Layer-internal helpers Nuxt auto-imports from `app/utils` and
  // `app/composables`. Both are pure — `pageArgs` is a plain function and
  // `useAdminDetailLink` injects with a null-returning default — so unlike
  // the app-specific composables below they need no per-test shape.
  pageArgs,
  useAdminDetailLink,
  definePageMeta() {
    // Nuxt's pages module strips this at build time and reads the object for
    // route metadata; standalone, it is a no-op.
  },
})
