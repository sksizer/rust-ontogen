import type { Meta, StoryObj } from '@storybook/vue3'
import { defineComponent, h } from 'vue'

const WelcomePage = defineComponent({
  name: 'WelcomePage',
  setup() {
    return () =>
      h(
        'div',
        {
          style:
            'max-width: 600px; margin: 2rem auto; font-family: system-ui, sans-serif; color: #333;',
        },
        [
          h('h1', { style: 'font-size: 2rem; margin-bottom: 0.5rem;' }, 'Tauri + Nuxt Template'),
          h(
            'p',
            { style: 'color: #666; font-size: 1.1rem; margin-bottom: 2rem;' },
            'A desktop application template built with Tauri 2, Nuxt, and TypeScript.',
          ),
          h('h2', { style: 'font-size: 1.3rem; margin-bottom: 0.5rem;' }, 'Writing Stories'),
          h('p', { style: 'color: #666; line-height: 1.6;' }, [
            'Stories live alongside components in ',
            h(
              'code',
              { style: 'background: #f0f0f0; padding: 2px 6px; border-radius: 4px;' },
              'app/components/',
            ),
            '. Each component has a co-located ',
            h(
              'code',
              { style: 'background: #f0f0f0; padding: 2px 6px; border-radius: 4px;' },
              '.stories.ts',
            ),
            ' file using the CSF3 format.',
          ]),
          h('ul', { style: 'color: #666; line-height: 2;' }, [
            h('li', {}, 'Use args for prop customization via Storybook controls'),
            h('li', {}, 'Components work without Tauri — pass mock functions as props'),
            h('li', {}, 'See the Components section in the sidebar for examples'),
          ]),
          h(
            'h2',
            { style: 'font-size: 1.3rem; margin-top: 1.5rem; margin-bottom: 0.5rem;' },
            'Learn More',
          ),
          h('ul', { style: 'color: #666; line-height: 2; list-style: none; padding: 0;' }, [
            h('li', {}, [
              h(
                'a',
                {
                  href: 'https://storybook.js.org/docs',
                  target: '_blank',
                  style: 'color: #667eea;',
                },
                'Storybook Documentation',
              ),
            ]),
            h('li', {}, [
              h(
                'a',
                { href: 'https://v2.tauri.app', target: '_blank', style: 'color: #667eea;' },
                'Tauri v2 Documentation',
              ),
            ]),
            h('li', {}, [
              h(
                'a',
                { href: 'https://nuxt.com/docs', target: '_blank', style: 'color: #667eea;' },
                'Nuxt Documentation',
              ),
            ]),
          ]),
        ],
      )
  },
})

const meta = {
  title: 'Welcome',
  component: WelcomePage,
  parameters: {
    backgrounds: { default: 'light' },
    controls: { disable: true },
    actions: { disable: true },
  },
} satisfies Meta<typeof WelcomePage>

export default meta
type Story = StoryObj<typeof meta>

export const Default: Story = {}
