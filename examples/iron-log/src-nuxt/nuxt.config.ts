export default defineNuxtConfig({
  compatibilityDate: '2024-11-01',
  extends: ['@ontogen/admin-layer'],
  ssr: false,
  devServer: {
    port: 1420,
    host: 'localhost',
  },
  modules: [
    '@nuxt/icon',
    '@nuxt/ui',
    '@pinia/nuxt',
  ],
})
