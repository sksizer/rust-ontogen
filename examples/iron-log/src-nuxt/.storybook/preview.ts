import type { Preview } from '@storybook/vue3'
import '../app/assets/css/main.css'

const preview: Preview = {
  parameters: {
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/i,
      },
    },
    viewport: {
      viewports: {
        tauriSmall: { name: 'Tauri Small', styles: { width: '800px', height: '600px' } },
        tauriMedium: { name: 'Tauri Medium', styles: { width: '1024px', height: '768px' } },
        tauriLarge: { name: 'Tauri Large', styles: { width: '1440px', height: '900px' } },
      },
    },
    backgrounds: {
      default: 'app',
      values: [
        { name: 'app', value: 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)' },
        { name: 'light', value: '#ffffff' },
        { name: 'dark', value: '#1a1a2e' },
      ],
    },
  },
}

export default preview
