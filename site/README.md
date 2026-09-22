# Ontogen documentation site

The Ontogen docs, built with [Astro](https://astro.build) and
[Starlight](https://starlight.astro.build). Deployed from `main` via Cloudflare
Pages.

## Running it

```sh
npm install
npm run dev      # http://localhost:4321
npm run build    # production build into ./dist
npm run preview  # serve ./dist locally
```

## Layout

```
site/
├── astro.config.mjs          # site config -- the sidebar lives here
├── public/                   # static assets served as-is
└── src/
    ├── assets/               # logos, images referenced from content
    ├── styles/custom.css     # theme overrides
    └── content/docs/         # every page, one .mdx per route
        ├── index.mdx         # landing page
        ├── getting-started/
        ├── concepts/
        ├── guides/
        ├── cookbook/
        ├── reference/
        └── examples/
```

A file at `src/content/docs/guides/store-layer.mdx` is served at
`/guides/store-layer/`. Adding a page means creating the file **and** adding it
to the `sidebar` array in `astro.config.mjs` -- Starlight won't pick it up
automatically.

## Writing

- Internal links are absolute, trailing-slashed: `[Store Layer](/guides/store-layer/)`.
- Prose uses `--`, not em dashes, matching the rest of the repository.
- Rust snippets that show a config struct must list **every** field. These are
  plain structs with public fields and no `#[non_exhaustive]`, so an omitted
  field is a compile error for anyone who copies the block. When a snippet is
  deliberately partial, mark the gap with `// ... other fields`.
- The four `build.rs` files under [`examples/`](../examples/) are the source of
  truth for what a working pipeline looks like. When the API changes, reconcile
  the docs against those rather than against another doc page.
