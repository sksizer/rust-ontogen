# Proposal: MCP-to-CLI Generator

## Status: Draft

## Problem

MCP tools are great for AI agents but awkward for humans. When debugging, scripting, or integrating with shell pipelines, users want a CLI -- not a JSON-RPC client. Without a generated CLI, you either use the UI or hand-craft MCP/HTTP calls.

## Goal

Add a **CLI client generator** to ontogen that consumes the same API metadata (IR or scanned API modules) and produces a standalone `clap`-based Rust CLI binary. Each MCP tool becomes a subcommand. The CLI connects to the MCP server over stdio or HTTP/SSE.

## Design

### Input

The generator reads from one of two sources (configurable):

1. **IR path** — `ApiOutput` from the codegen pipeline. Available at build time when running the full pipeline.
2. **Scan path** — parse API modules directly (same `servers::parse` scanner). Works standalone without running upstream generators.

Both paths produce the same internal representation: a list of modules, each with typed functions, params, return types, and operation classifications.

### Output

A single generated Rust file (e.g., `cli/generated.rs`) containing:

```rust
// Auto-generated CLI. DO NOT EDIT.
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "myapp", about = "CLI for the MyApp API")]
pub struct Cli {
    /// MCP server connection (stdio or url)
    #[arg(long, default_value = "stdio")]
    pub transport: String,

    /// Project ID (optional, for multi-tenant)
    #[arg(long)]
    pub project_id: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Node operations
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },
    // ... one variant per entity/module
}

#[derive(Subcommand)]
pub enum NodeAction {
    /// List all nodes
    List,
    /// Get a node by ID
    Get {
        #[arg(help = "Node ID")]
        id: String,
    },
    /// Create a node
    Create {
        /// JSON input (or reads from stdin)
        #[arg(long)]
        json: Option<String>,
    },
    /// Update a node
    Update {
        #[arg(help = "Node ID")]
        id: String,
        /// JSON input
        #[arg(long)]
        json: Option<String>,
    },
    /// Delete a node
    Delete {
        #[arg(help = "Node ID")]
        id: String,
    },
    // ... custom actions from scanned API fns
}
```

Plus an `execute()` function that dispatches each subcommand to the appropriate MCP tool call.

### MCP Client Layer

The generated CLI uses a thin MCP client that:

1. **stdio transport** — spawns the server binary as a child process, communicates via JSON-RPC over stdin/stdout
2. **HTTP/SSE transport** — connects to `http://localhost:4243` (or configured URL)
3. Serializes subcommand args → MCP `tools/call` request
4. Deserializes response → pretty-printed JSON (default) or raw JSON (`--raw`)

```
myapp node list
myapp node get NODE-001
myapp node create --json '{"id":"new-node","name":"New Node",...}'
myapp requirement list | jq '.[] | .id'
echo '{"id":"x","name":"y"}' | myapp node create --json -
```

### Architecture within ontogen

```
src/
└── clients/
        ├── mod.rs          # ClientGeneratorConfig enum (existing)
        ├── cli/
        │   ├── mod.rs      # generate() entry point
        │   ├── gen_clap.rs # Clap struct/enum generation
        │   └── gen_dispatch.rs  # MCP dispatch function generation
        └── ...             # existing ts_client, admin generators
```

New variant in `ClientGeneratorConfig`:

```rust
pub enum ClientGeneratorConfig {
    // ... existing variants ...
    /// Rust CLI wrapping MCP tools via clap subcommands.
    McpCli {
        output: PathBuf,
        binary_name: String,
    },
}
```

### Operation Mapping

| OpKind | CLI Pattern | MCP Tool |
|--------|-------------|----------|
| List | `<entity> list` | `list_{entities}` |
| GetById | `<entity> get <id>` | `get_{entity}` |
| Create | `<entity> create --json <data>` | `create_{entity}` |
| Update | `<entity> update <id> --json <data>` | `update_{entity}` |
| Delete | `<entity> delete <id>` | `delete_{entity}` |
| CustomGet | `<entity> <action> [args...]` | `{module}_{action}` |
| CustomPost | `<entity> <action> --json <data>` | `{module}_{action}` |
| EventStream | `<entity> watch` | SSE subscription |

### Output Formatting

- Default: pretty-printed JSON (colored if tty)
- `--raw`: compact JSON (for piping)
- `--format table`: tabular output for list operations (stretch goal)

## Phases

### Phase 1: CRUD subcommands
- Generate clap structs for all entities with standard CRUD actions
- MCP stdio transport client
- JSON input/output

### Phase 2: Custom actions
- Scan custom API functions → generate additional subcommands
- Typed arguments for simple params (string, int) instead of requiring `--json`

### Phase 3: DX polish
- HTTP transport option
- Shell completions generation (`myapp completions bash/zsh/fish`)
- `--format table` for list output
- Stdin pipe support for create/update JSON
- `--dry-run` to show the MCP request without sending

### Phase 4: Generic / library extraction
- Make the generator work for any MCP server, not just a single application
- Input: MCP server manifest / tool list JSON
- Output: standalone CLI crate

## Decisions

1. **Binary location** — generated as another binary in the consuming project. The binary name is a config option (e.g., `binary_name: "myapp-cli"`). The generator outputs a `main.rs` + `generated.rs` into a configured crate directory.
2. **MCP client** — generate a minimal inline JSON-RPC client. The protocol surface is small (`initialize`, `tools/call`, `tools/list`), so no external MCP crate dependency needed. Just `serde_json`, `tokio`, and transport I/O.
3. **Auth** — local only to start. No auth headers for stdio or HTTP transport. Can add later if needed.
