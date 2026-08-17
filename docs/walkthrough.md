# Ontogen: Detailed Walkthrough

**This walkthrough has been retired.** It was written during the design phase,
before the pipeline's API settled, and documented a shape that was never built --
`StoreWiringConfig`, `StoreConfig::dto_output` / `event_emission` /
`change_channels`, a four-argument `gen_store`, a `ClientsConfig` assembled from
`TypeScriptConfig` and `AdminRegistryConfig`. None of those symbols exist.

Rather than maintain a second full tutorial in parallel with the documentation
site -- which is what let this one drift unnoticed in the first place -- each
stage now has a single home. The site is built from [`site/`](../site/), so it
moves with the code.

| Stage | Where it lives now |
|---|---|
| Schema definition | `guides/defining-entities`, `guides/schema-annotations` |
| `parse_schema` | `reference/public-api` |
| `gen_seaorm` | `guides/persistence-seaorm` |
| `gen_markdown_io` | `guides/markdown-backend`, `guides/markdown-io` |
| `gen_store` | `guides/store-layer`, `guides/lifecycle-hooks` |
| `gen_api` | `guides/api-layer`, `cookbook/custom-api-endpoints` |
| `gen_servers` | `guides/server-transports` |
| `gen_clients` | `guides/client-generation`, `guides/typescript-bindings` |
| The full `build.rs` | `guides/build-script-setup` |
| Every config type | `reference/configuration` |
| The IRs between stages | `reference/intermediate-representations`, `concepts/pipeline` |

Those paths are relative to [`site/src/content/docs/`](../site/src/content/docs/).
To read them rendered:

```sh
cd site && npm install && npm run dev
```

For a worked end-to-end example you can build and run, see
[`examples/`](../examples/) -- `iron-log` for the SeaORM path; `iron-log-md`,
`tasks-tracker`, and `notes-kb` for the markdown store backend.

For the design rationale behind the pipeline -- why the stages are shaped this
way, what was considered and rejected -- see [`proposal.md`](proposal.md), which
remains accurate as a decision record.
