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
    |-- gen_seaorm      -> SeaOrmOutput
    |-- gen_markdown_io -> ()
    |-- gen_dtos        -> ()
    `-- gen_store       -> StoreOutput
        `-- gen_api     -> ApiOutput
            `-- gen_servers -> ServersOutput
                `-- gen_clients -> ()
```

Each generator is a standalone function. You can run the full pipeline or pick individual stages. Upstream outputs are
optional enrichment, not hard requirements.

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

    let _store = gen_store(&schema.entities, Some(&seaorm), &StoreConfig {
        output_dir: "src/store/generated".into(),
        hooks_dir: Some("src/store/hooks".into()),
    }).unwrap();

    // ... continue with gen_api, gen_servers, gen_clients as needed
}
```

One `cargo build` generates your SeaORM entities, CRUD store methods, lifecycle hook stubs, API forwarding functions,
and transport handlers. Add a new entity to your schema and rebuild -- everything updates automatically.

## Key Features

- **Layered pipeline** with typed intermediate representations between each stage
- **Independent generators** that can run alone or be chained for richer output
- **SeaORM persistence** including entity models, junction tables, and model conversions
- **Store generation** with CRUD methods, update structs, and relation population
- **Lifecycle hooks** scaffolded once per entity, never overwritten -- you own the hook files
- **API layer** that merges generated CRUD with hand-written custom endpoints
- **Server transports** for Axum HTTP, Tauri IPC, and MCP (Model Context Protocol)
- **Client generation** for TypeScript and admin registries
- **Markdown I/O** with parser dispatch, writers, and filesystem operations

## Documentation

- [Walkthrough](docs/walkthrough.md) -- end-to-end pipeline tutorial with concrete examples
- [Architecture Proposal](docs/proposal.md) -- design rationale and decision log
- [CLI Generator Proposal](docs/cli-generator.md) -- planned MCP-to-CLI client generator

## Project Status

Ontogen is functional and in active development. Phases 1-5 of the pipeline are complete (schema parsing through client
generation). See [docs/tasks.md](docs/tasks.md) for the current backlog.

## License

This project is licensed under MIT.
