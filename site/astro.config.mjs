// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	integrations: [
		starlight({
			title: 'Ontogen',
			tagline: 'Build-time code generation for ontology-driven Rust applications',
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/sksizer/rust-ontogen' },
			],
			logo: {
				light: './src/assets/logo-light.svg',
				dark: './src/assets/logo-dark.svg',
				replacesTitle: false,
			},
			customCss: ['./src/styles/custom.css'],
			editLink: {
				baseUrl: 'https://github.com/sksizer/rust-ontogen/edit/main/site/',
			},
			head: [
				{
					tag: 'meta',
					attrs: {
						property: 'og:description',
						content: 'Define your entities once. Ontogen generates your persistence layer, CRUD store, API endpoints, server transports, and client libraries at build time.',
					},
				},
			],
			sidebar: [
				{
					label: 'Getting Started',
					items: [
						{ label: 'Installation', slug: 'getting-started/installation' },
						{ label: 'Quick Start', slug: 'getting-started/quick-start' },
						{ label: 'Your First Entity', slug: 'getting-started/your-first-entity' },
					],
				},
				{
					label: 'Concepts',
					items: [
						{ label: 'Architecture', slug: 'concepts/architecture' },
						{ label: 'The Pipeline', slug: 'concepts/pipeline' },
						{ label: 'Design Philosophy', slug: 'concepts/design-philosophy' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Defining Entities', slug: 'guides/defining-entities' },
						{ label: 'Schema Annotations', slug: 'guides/schema-annotations' },
						{ label: 'Field Types & Roles', slug: 'guides/field-types-and-roles' },
						{ label: 'Relationships', slug: 'guides/relationships' },
						{ label: 'Persistence (SeaORM)', slug: 'guides/persistence-seaorm' },
						{ label: 'Store Layer', slug: 'guides/store-layer' },
						{ label: 'Lifecycle Hooks', slug: 'guides/lifecycle-hooks' },
						{ label: 'API Layer', slug: 'guides/api-layer' },
						{ label: 'Server Transports', slug: 'guides/server-transports' },
						{ label: 'Client Generation', slug: 'guides/client-generation' },
						{ label: 'Markdown I/O', slug: 'guides/markdown-io' },
						{ label: 'Build Script Setup', slug: 'guides/build-script-setup' },
					],
				},
				{
					label: 'Cookbook',
					items: [
						{ label: 'Adding a New Entity', slug: 'cookbook/adding-a-new-entity' },
						{ label: 'Custom API Endpoints', slug: 'cookbook/custom-api-endpoints' },
						{ label: 'Tauri Integration', slug: 'cookbook/tauri-integration' },
						{ label: 'MCP Integration', slug: 'cookbook/mcp-integration' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Public API', slug: 'reference/public-api' },
						{ label: 'Configuration', slug: 'reference/configuration' },
						{ label: 'Annotations', slug: 'reference/annotations' },
						{ label: 'Intermediate Representations', slug: 'reference/intermediate-representations' },
						{ label: 'Field Types', slug: 'reference/field-types' },
					],
				},
				{
					label: 'Examples',
					items: [
						{ label: 'Iron Log', slug: 'examples/iron-log' },
					],
				},
			],
		}),
	],
});
