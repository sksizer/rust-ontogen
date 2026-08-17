# Ontogen [![CI][ci-badge]][ci] [![License: MIT][license-badge]][license]

[ci]: https://github.com/sksizer/rust-ontogen/actions
[ci-badge]: https://github.com/sksizer/rust-ontogen/actions/workflows/ci.yml/badge.svg
[license]: https://opensource.org/licenses/MIT
[license-badge]: https://img.shields.io/badge/License-MIT-blue.svg

A build-time code generator for ontology-driven Rust applications. Define your entities with annotated structs and
Ontogen generates the full stack: persistence layer, CRUD store with lifecycle hooks, API forwarding, server transports
(HTTP/IPC/MCP), and client libraries.

## How It Works

Ontogen runs as a library in your `build.rs`. It parses `#[ontology(...)]` annotations on your structs and generates
code through a pipeline of independent generators, each producing typed intermediate representations that downstream
generators can optionally consume:

```text
parse_schema -> SchemaOutput
    |-- gen_seaorm      -> SeaOrmOutput       ---.
    |-- gen_markdown_io -> MarkdownIoOutput   ---+--> StoreConfig.backend
    |-- gen_dtos        -> ()                    |
    `-- gen_store       -> StoreOutput        <--'
        `-- gen_api     -> ApiOutput
            |-- gen_servers -> ServersOutput  (Rust transports: Axum / Tauri IPC / MCP)
            `-- gen_clients -> ()             (TS bindings, HTTP / IPC clients, admin registry)
```

Each generator is a standalone function. You can run the full pipeline or pick individual stages. Upstream outputs are
optional enrichment, not hard requirements -- with one exception: `gen_store` needs a `Backend` to know what to emit
CRUD against, and `Backend::Markdown` carries the `MarkdownIoOutput` that `gen_markdown_io` returned.

## Quick Example

Define an entity:

```rust
#[derive(OntologyEntity)]
#[ontology(entity, table = "tasks", directory = "tasks")]
pub struct Task {
    #[ontology(id)]
    pub id: String,
    pub name: String,
    pub description: Option<String>,

    #[ontology(relation(belongs_to, target = "Agent"))]
    pub assignee_id: Option<String>,

    #[ontology(relation(many_to_many, target = "Requirement"))]
    pub fulfills: Vec<String>,
}
```

Wire it in `build.rs`:

```rust
use ontogen::*;

fn main() {
    let schema = parse_schema(&SchemaConfig {
        schema_dir: "src/schema".into(),
    }).unwrap();

    let seaorm = gen_seaorm(&schema.entities, &SeaOrmConfig {
        entity_output: "src/persistence/entities/generated".into(),
        conversion_output: "src/persistence/conversions/generated".into(),
        skip_conversions: vec![],
    }).unwrap();

    let _store = gen_store(&schema.entities, &StoreConfig {
        output_dir: "src/store/generated".into(),
        hooks_dir: Some("src/store/hooks".into()),
        schema_module_path: DEFAULT_SCHEMA_MODULE_PATH.into(),
        backend: Backend::Seaorm(Some(seaorm)),
        wikilink_policy: None,
    }).unwrap();

    // ... continue with gen_api, gen_servers, gen_clients as needed
}
```

Or drive the same stages through the `Pipeline` builder, which threads each output into the next and applies the
defaults for you:

```rust
ontogen::Pipeline::new("src/schema")
    .seaorm("src/persistence/entities/generated", "src/persistence/conversions/generated")
    .store("src/store/generated", Some::<std::path::PathBuf>("src/store/hooks".into()))
    .api("src/api/v1/generated", "AppState")
    .build()
    .expect("ontogen pipeline failed");
```

One `cargo build` generates your SeaORM entities, CRUD store methods, lifecycle hook stubs, API forwarding functions,
and transport handlers. Add a new entity to your schema and rebuild -- everything updates automatically.

## Key Features

- **Layered pipeline** with typed intermediate representations between each stage
- **Independent generators** that can run alone or be chained for richer output
- **Two store backends**, chosen with one config field: SeaORM/SQL, or a vault of markdown files
  ([ADR 0001](docs/architecture/0001-markdown-as-store-backend.md)). Everything above the store is byte-identical
  between them.
- **SeaORM persistence** including entity models, junction tables, and model conversions
- **Markdown persistence** where records are `.md` files with YAML frontmatter -- editable in any editor, diffable in
  git, navigable in Obsidian -- with hand edits preserved across generated writes
- **Store generation** with CRUD methods, update structs, and relation population
- **Lifecycle hooks** scaffolded once per entity, never overwritten -- you own the hook files
- **API layer** that merges generated CRUD with hand-written custom endpoints
- **Server transports** for Axum HTTP, Tauri IPC, and MCP (Model Context Protocol)
- **Client generation** for TypeScript and admin registries, with a build-time AST walker that emits the full
  reachable type closure -- no side-car binary, no extra compilation

## Example Projects

Four runnable examples live under [`examples/`](examples/):

| Example | Backend | What it shows |
|---|---|---|
| [`iron-log`](examples/iron-log/) | SeaORM | The full pipeline end to end: 4 entities to a Tauri + Nuxt app with a generated TypeScript client |
| [`iron-log-md`](examples/iron-log-md/) | Markdown | iron-log's exact schema on the markdown backend -- `diff -r` the generated `api/v1` trees to watch the byte-identical invariant hold |
| [`tasks-tracker`](examples/tasks-tracker/) | Markdown | A planning vault over HTTP **and** the generated MCP tool registry |
| [`notes-kb`](examples/notes-kb/) | Markdown | Wikilinked notes served as a graph |

```bash
cd examples/iron-log/src-tauri
cargo build
```

That generates 36 files across all layers -- 23 Rust modules under `generated/`, 5 DTOs, 5 hook stubs, and 3
TypeScript files -- from 4 schema entity files.

## Documentation

The full documentation site is built from [`site/`](site/) -- start with Getting Started, then the guides.

In-repo:

- [Walkthrough](docs/walkthrough.md) -- end-to-end pipeline tutorial with concrete examples
- [ADR 0001](docs/architecture/0001-markdown-as-store-backend.md) -- markdown as a first-class store backend
- [Architecture Proposal](docs/proposal.md) -- design rationale and decision log
- [CLI Generator Proposal](docs/cli-generator.md) -- planned MCP-to-CLI client generator

## Project Status

Ontogen is functional and in active development. The full pipeline ships -- schema parsing through client generation,
on both store backends. See [docs/planning/tasks/README.md](docs/planning/tasks/README.md) for the current backlog and
[docs/roadmap.md](docs/roadmap.md) for the capability tiers.

## License

This project is licensed under MIT.
