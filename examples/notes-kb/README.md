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

## An OKF bundle

The vault is an [OKF 0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
bundle you can navigate without the app: `data/vault/index.md` and
`data/vault/notes/index.md` are committed, generated indexes, and every write
through the API keeps them current. `build.rs` turns on both OKF knobs
(`okf.index` and `okf.generated_by`), so a note the API creates or changes also
gains a `generated: { by: notes-kb/<version>, at: <UTC instant> }` stamp. The
seed notes carry none until their first real write.

The root-workspace test `tests/okf_conformance.rs` rebuilds the seed indexes
in a temp copy and fails if the committed files differ. After changing the seed
notes, re-bless them from the repo root:

```sh
OKF_BLESS_SEED_INDEXES=1 cargo test --test okf_conformance
```

See [the markdown backend guide](../../site/src/content/docs/guides/markdown-backend.mdx)
for the rules.
