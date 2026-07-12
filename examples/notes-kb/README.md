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

## The two-crate boundary demo

`markdown-store` (this repo) owns the write/round-trip side;
[`markdown-vault`](../../../rust-markdown) (the sibling workspace this crate
will eventually migrate into) owns read-only extraction. Same files, two
complementary lenses:

```sh
# requires ../../../rust-markdown checked out
cargo run --features vault-tags -- tags
#architecture
#graphs
#ops
```

Those tags came out of the note *bodies* via markdown-vault's extractor,
over the exact files the generated store reads and writes. Edit a note in
Obsidian; both lenses see it.
