export default defineNuxtConfig({
  telemetry: false,
  compatibilityDate: '2024-11-01',
  extends: ['@ontogen/admin-layer'],
  css: [
    '~/assets/css/no-bounce.css',
    '~/assets/css/main.css',
  ],
  devtools: { enabled: true },
  ssr: false,
  devServer: {
    port: parseInt(process.env.TAURI_DEV_PORT || '1420'),
    host: 'localhost',
  },
  modules: [
    '@nuxt/eslint',
    '@nuxt/fonts',
    '@nuxt/icon',
    '@nuxt/scripts',
    '@nuxt/test-utils',
    '@nuxt/ui',
    '@pinia/nuxt',
  ],
})
