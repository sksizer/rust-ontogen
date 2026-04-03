import type { StorybookConfig } from '@storybook/vue3-vite'
import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'

const config: StorybookConfig = {
  stories: ['../app/**/*.stories.@(ts|tsx)', '../stories/**/*.stories.@(ts|tsx)'],
  addons: ['@storybook/addon-a11y'],
  framework: {
    name: '@storybook/vue3-vite',
    options: {},
  },
  viteFinal: async (config) => {
    config.plugins = config.plugins || []
    config.plugins.push(tailwindcss())
    config.plugins.push(vue())
    // Avoid Vite 7 failing to resolve Nuxt's .nuxt/tsconfig.json extends
    config.esbuild = {
      ...config.esbuild,
      tsconfigRaw: '{}',
    }
    return config
  },
}

export default config
