# notes-kb — the vault as a graph

An Obsidian-vault-shaped knowledge base: notes whose frontmatter `links` are
wikilinks to other notes — one syntax that is simultaneously a foreign key,
a graph edge, and an Obsidian link.

```sh
cargo run            # http://127.0.0.1:3003 — the graph IS the index page
```

The index page is a deliberately framework-free SVG graph fed by the
generated HTTP API (click a node for the note body). The generated TypeScript
client lives in `generated-ts/` for a real frontend to consume — a full Nuxt
app over it is left as the natural next step, shaped by your own component
conventions rather than generated boilerplate.
