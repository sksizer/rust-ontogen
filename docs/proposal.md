# Proposal: Unified Layered Code Generation

## Status: Implementation (v3)

## Problem

The original codegen approach was split across two separate crates with an uncovered middle:

```
Schema ──► schema-codegen  ──► DB Entities, DTOs, Markdown I/O
                                         ↓ (hand-written gap)
                                      Store Layer (95% boilerplate)
                                         ↓ (hand-written gap)
API fns ──► service-codegen ──► HTTP, IPC, MCP, TypeScript
```

**Pain points:**
1. Store layer is ~95% identical boilerplate across 10 entities (CRUD, junction sync, relation population, event emission)
2. Two independent codegen crates parse separately — no shared entity metadata flows between them
3. Adding a new entity requires touching 5+ hand-written files beyond what's generated
4. No standard lifecycle hooks, change channels, or instrumentation points
5. The API layer (`src/api/v1/*.rs`) is also largely boilerplate forwarding to Store

## Core Principles

1. **Each generator is independently executable** — give it config and it runs; no mandatory upstream dependency
2. **Chainable outputs** — each generator returns a typed output struct that downstream generators *can* consume for richer generation, but don't *require*
3. **Multiple input sources** — the transport layer must still scan source files for non-entity APIs (graph, events, project, status, settings, path_opener, entity_counts)
4. **Generated and custom coexist** — generated code lives in `generated/` subdirectories alongside hand-written modules; the transport layer merges both

## Architecture: Independent Generators with Optional Chaining

```
                    ┌─────────────┐
  Schema/*.rs ────► │ parse_schema│──► SchemaOutput { entities }
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │gen_seaorm│ │ gen_dtos │ │gen_md_io │   Independent generators
        └────┬─────┘ └──────────┘ └──────────┘   (each runs alone)
             │
             ▼  SeaOrmOutput
        ┌──────────┐
  store/│ gen_store │──► StoreOutput { methods, scaffolds, channels }
  hooks/│          │◄── scaffolds hook stubs (once, never overwrites)
  *.rs  └────┬─────┘
             │ (optional)
        ┌────▼─────┐
  api/  │ gen_api  │──► ApiOutput { modules }
  v1/   │          │◄── scans v1/ for hand-written API modules
  *.rs  └────┬─────┘
             │
        ┌────▼───────┐
        │ gen_servers │──► ServersOutput { http_routes, ipc_commands, mcp_tools }
        └────┬───────┘
             │
        ┌────▼───────┐
        │ gen_clients │──► TypeScript, CLI, admin registry
        └────────────┘    (shape derived from which servers exist)
```

### Generator Function Signatures

Each generator is a standalone function. Upstream outputs are `Option` parameters — enrichment, not requirements. All merge layers normalize generated + scanned sources into the **same IR types** (see [Unified IR Types](#unified-ir-types)).

```rust
// ── Source discriminator (shared across all layers) ────────────────

/// Where did this method/module originate?
/// Used by downstream generators to emit correct import paths.
#[derive(Debug, Clone)]
pub enum Source {
    /// Generated from EntityDef by a codegen layer
    Generated { module_path: String },
    /// Scanned from a hand-written source file
    Scanned { module_path: String, file_path: PathBuf },
}

// ── Output structs: plain data, no side effects ────────────────────

pub struct SchemaOutput {
    pub entities: Vec<EntityDef>,
}

/// SeaORM-specific output. Only produced by gen_seaorm.
pub struct SeaOrmOutput {
    pub entity_tables: Vec<EntityTableMeta>,    // table names, column mappings
    pub junction_tables: Vec<JunctionMeta>,      // junction table metadata
    pub conversion_fns: Vec<ConversionMeta>,     // which from_model/to_active_model exist
}

/// Store layer output. Methods from both generated and scanned sources,
/// normalized into the same StoreMethodMeta type.
pub struct StoreOutput {
    pub methods: Vec<StoreMethodMeta>,           // generated + scanned, same type
    pub scaffolded_hooks: Vec<ScaffoldMeta>,      // scaffolded hook file paths + functions
    pub change_channels: Vec<ChannelMeta>,       // per-entity broadcast channels
}

/// API layer output. Modules from both generated and scanned sources,
/// normalized into the same ApiModule type.
pub struct ApiOutput {
    pub modules: Vec<ApiModule>,                 // generated + scanned, same type
}

/// Server transport output. Describes the concrete endpoints generated
/// so client generators can mirror them exactly.
pub struct ServersOutput {
    pub http_routes: Vec<HttpRouteMeta>,         // method, path, handler name
    pub ipc_commands: Vec<IpcCommandMeta>,        // command name, params (camelCase)
    pub mcp_tools: Vec<McpToolMeta>,             // tool name, JSON schema
}

// ── Generator functions ────────────────────────────────────────────

/// Parse schema files. Always the starting point when entity metadata is needed.
pub fn parse_schema(
    config: &SchemaConfig,
) -> Result<SchemaOutput, CodegenError>;

// ── Persistence generators (independent, produce building blocks) ──

/// Generate SeaORM entities, junction tables, and model conversions
/// (from_model, to_active_model).
pub fn gen_seaorm(
    entities: &[EntityDef],
    config: &SeaOrmConfig,
) -> Result<SeaOrmOutput, CodegenError>;

/// Generate markdown I/O: parser dispatch, writers, path helpers, and fs_ops.
/// Always generates the building blocks (render_*, write_*, *_path, parse).
/// Optionally generates store event wiring (FsSubscriber that listens for
/// EntityChange events and calls write_* functions with write-back loop guard).
pub fn gen_markdown_io(
    entities: &[EntityDef],
    config: &MarkdownIoConfig,
) -> Result<(), CodegenError>;    // no downstream IR needed

// ── Store generator (produces DTOs + CRUD + hooks) ─────────────────

/// Generate the store layer, including:
/// - Create/Update DTOs (CreateNodeInput, UpdateNodeInput)
/// - Update structs with apply() (NodeUpdate)
/// - From<Input> conversions (with wikilink stripping on relation fields)
/// - CRUD methods (list, get, create, update, delete)
/// - Scaffold hook files and populate_relations()
/// - Change channels (optional)
///
/// DTOs and conversions live here because the store is where they're consumed.
/// For standalone DTO generation without a store, use gen_dtos().
pub fn gen_store(
    entities: &[EntityDef],
    seaorm: Option<&SeaOrmOutput>,
    scan_dirs: &[PathBuf],        // e.g. ["src/store/custom"]
    config: &StoreConfig,
) -> Result<StoreOutput, CodegenError>;

/// Standalone DTO generation — for consumers who want Create/Update input
/// types without generating a full store layer. Same output as the DTO
/// portion of gen_store, just without CRUD/hooks/channels.
pub fn gen_dtos(
    entities: &[EntityDef],
    config: &DtoConfig,
) -> Result<(), CodegenError>;

/// Generate API layer.
/// Generates CRUD forwarding from entities, scans scan_dirs for custom modules.
/// Merges both into a single ApiOutput with uniform ApiFn types.
pub fn gen_api(
    entities: &[EntityDef],
    store: Option<&StoreOutput>,
    scan_dirs: &[PathBuf],        // e.g. ["src/api/v1"]
    config: &ApiConfig,
) -> Result<ApiOutput, CodegenError>;

/// Generate server-side transport handlers (HTTP, IPC, MCP).
/// Returns ServersOutput describing the concrete routes/commands/tools
/// that were generated — this is what client generators consume.
pub fn gen_servers(
    api: &ApiOutput,
    entities: Option<&[EntityDef]>,
    config: &ServersConfig,
) -> Result<ServersOutput, CodegenError>;

/// Generate client libraries (TypeScript, CLI, admin registry).
/// Built from ServersOutput — mirrors the actual server endpoints.
/// Each client type derives its shape from which servers exist:
/// - TypeScript: createHttpTransport() / createIpcTransport() / unified interface
/// - CLI: Rust clap subcommands that call HTTP endpoints
/// - Admin registry: TypeScript metadata for dynamic CRUD UI
pub fn gen_clients(
    servers: &ServersOutput,
    entities: Option<&[EntityDef]>,
    config: &ClientsConfig,
) -> Result<(), CodegenError>;
```

### Unified IR Types

All merge layers normalize generated and scanned sources into the **same struct types**. The `source` field is provenance metadata (for import paths), not a behavioral discriminator. Downstream consumers process all entries uniformly.

```rust
/// A store method — same type whether generated from schema or scanned from custom/.
/// `entity_name` is the grouping key that drives REST resource routing
/// (e.g. "Widget" → api/v1/widgets/ with standard REST verbs).
pub struct StoreMethodMeta {
    pub entity_name: String,           // "Node" or "Widget" (custom entity)
    pub name: String,                  // "create_node" or "bulk_reparent_nodes"
    pub kind: StoreMethodKind,         // Crud(Create) or Custom
    pub params: Vec<ParamMeta>,
    pub return_type: String,
    pub source: Source,                // Generated or Scanned — same struct either way
}

pub enum StoreMethodKind {
    Crud(CrudOp),                      // List, Get, Create, Update, Delete
    Custom,                            // anything scanned that doesn't match CRUD pattern
}

// Entity association for scanned methods:
// - Generated: entity_name is known from the EntityDef being iterated
// - Scanned CRUD: naming convention — create_widget → entity "Widget",
//   list_widgets → entity "Widget". Matches the generated naming pattern.
//   Override with #[ontology(entity = "Widget")] when convention doesn't fit.
// - Scanned Custom: entity_name from #[ontology(entity = "Node")] annotation
//   or "" if the method isn't entity-scoped (consumed via hand-written API only)

/// An API function — same type whether generated or scanned.
pub struct ApiFn {
    pub name: String,                  // "create" or "archive"
    pub params: Vec<ParamMeta>,
    pub return_type: String,
    pub source: Source,                // Generated or Scanned
    pub classified_op: OpKind,         // List, GetById, Create, ..., CustomPost
}

/// An API module — may contain functions from both sources, all as ApiFn.
pub struct ApiModule {
    pub name: String,                  // "node"
    pub fns: Vec<ApiFn>,              // mixed sources, same type
    pub state_type: StateKind,         // AppState or Store
}
```

### Chaining in build.rs

```rust
// ── Full pipeline: each output feeds the next ──────────────────────

fn main() {
    // ── Parse ──────────────────────────────────────────────────────
    let schema = parse_schema(&SchemaConfig {
        schema_dir: "src/schema".into(),
    })?;

    // ── Persistence (independent generators, no chaining needed) ───
    // ── Persistence (independent generators) ──────────────────────
    let seaorm = gen_seaorm(&schema.entities, &SeaOrmConfig {
        entity_output: "src/persistence/db/entities/generated".into(),
        conversion_output: "src/persistence/db/conversions/generated".into(),
    })?;

    // Markdown I/O: always generates parsers, writers, path helpers.
    // Optionally generates store event wiring (FsSubscriber + write-back guard).
    gen_markdown_io(&schema.entities, &MarkdownIoConfig {
        output_dir: "src/persistence/fs_markdown/generated".into(),
        store_wiring: Some(StoreWiringConfig {
            // Generates an FsSubscriber that listens for EntityChange events
            // and calls the appropriate write_*() function for each entity.
            // Includes write-back loop prevention via write_guard.
            subscriber_output: "src/persistence/fs_markdown/generated/subscriber.rs".into(),
        }),
    })?;

    // ── Store (generates DTOs + CRUD + hooks, merges scanned custom) ──
    let store = gen_store(
        &schema.entities,
        Some(&seaorm),
        &["src/store/custom"],       // scan for hand-written store methods
        &StoreConfig {
            output_dir: "src/store/generated".into(),
            hooks_dir: "src/store/hooks".into(),       // scaffold stubs here
            dto_output: "src/schema/dto".into(),       // DTOs generated here
            event_emission: true,
            change_channels: true,
            entity_overrides: vec![],
        },
    )?;

    // ── API (merges generated CRUD + scanned custom modules) ───────
    let api = gen_api(&schema.entities, Some(&store), &["src/api/v1"], &ApiConfig {
        generated_output: "src/api/v1/generated".into(),
        versions: vec!["v1".into()],
        exclude: vec![
            ("Contract".into(), "v1".into()),
            ("Evidence".into(), "v1".into()),
        ],
    })?;

    // ── Servers (HTTP, IPC, MCP — independent of each other) ─────
    let servers = gen_servers(&api, Some(&schema.entities), &ServersConfig {
        // Shared: project scoping concept.
        // Each server represents this differently (URL prefix, param, schema field).
        scoping: Some(ScopingConfig {
            param_name: "project_id",
            rust_type: "uuid::Uuid",
            ts_type: "string",
            state_accessor: "store_for",
        }),
        http: Some(HttpConfig {
            output: "src/api/transport/http/generated.rs".into(),
            route_prefix: "projects/:project_id",  // HTTP-specific: URL path prefix
        }),
        ipc: Some(IpcConfig {
            output: "src/api/transport/ipc/generated.rs".into(),
            // IPC: projectId as optional camelCase arg — derived from scoping
        }),
        mcp: Some(McpConfig {
            output: "src/api/transport/mcp/generated.rs".into(),
            // MCP: project_id injected into tool JSON schema — derived from scoping
        }),
    })?;

    // ── Clients (derived from server output) ──────────────────────
    // TypeScript sees ServersOutput has both http_routes and ipc_commands,
    // so it generates createHttpTransport() + createIpcTransport() +
    // a unified Transport interface with automatic switching.
    gen_clients(&servers, Some(&schema.entities), &ClientsConfig {
        typescript: Some(TypeScriptConfig {
            output: "src-nuxt/app/transport/generated.ts".into(),
            bindings_path: "src-nuxt/app/types/bindings.ts".into(),
        }),
        cli: Some(CliConfig {
            output: "src-cli/generated/commands.rs".into(),
            // Generates clap subcommands from HTTP routes
            // e.g. `ontological node list`, `ontological node create --name "Auth"`
        }),
        admin_registry: Some(AdminRegistryConfig {
            output: "src-nuxt/layers/admin/generated/admin-registry.ts".into(),
        }),
    })?;
}
```

```rust
// ── Standalone: transport-only (no schema, just scan) ──────────────

fn servers_only() {
    let api = gen_api(&[], None, &["src/api/v1"], &ApiConfig {
        generated_output: "src/api/v1/generated".into(),
        versions: vec!["v1".into()],
        exclude: vec![],
    })?;

    let servers = gen_servers(&api, None, &servers_config)?;
    gen_clients(&servers, None, &clients_config)?;
}
```

## Generated vs Custom: Parallel Directory Convention

The **same generated/custom split** applies at every layer that has custom logic. Generated code lives in `generated/` subdirectories; hand-written code lives alongside it. Each layer merges both sources for its downstream consumers.

```
src/
├── store/
│   ├── generated/              # ◄ Generated: CRUD methods, update structs,
│   │   ├── mod.rs              #   populate_relations, From<Input> impls, change channels
│   │   ├── node.rs             #   list/get/create/update/delete + imports hooks::node
│   │   ├── requirement.rs
│   │   ├── agent.rs            #   simple entities: hooks file is all no-ops
│   │   └── ...
│   ├── hooks/                  # ◄ Scaffolded once, never overwritten — fill in as needed
│   │   ├── mod.rs
│   │   ├── node.rs             #   before_create, after_create, before_update, etc.
│   │   ├── requirement.rs      #   (status transition validation)
│   │   └── ...
│   ├── custom/                 # ◄ Hand-written: custom store methods beyond CRUD
│   │   ├── mod.rs
│   │   └── node_custom.rs      #   e.g. bulk_reparent_nodes() — custom store methods
│   ├── mod.rs                  # re-exports generated + custom, Store struct
│   └── store.rs                # Store construction, helper methods (sync_junction, etc.)
│
├── api/
│   ├── v1/                     # Everything for v1 lives here
│   │   ├── generated/          # ◄ Generated: CRUD forwarding functions
│   │   │   ├── mod.rs
│   │   │   ├── node.rs         #   list, get_by_id, create, update, delete
│   │   │   ├── requirement.rs
│   │   │   └── ...
│   │   ├── graph.rs            # ◄ Hand-written: custom API modules
│   │   ├── events.rs           #   graph_updated, entity_changed (SSE)
│   │   ├── project.rs          #   switch_project, open_project, close_project, ...
│   │   ├── status.rs           #   get_status_transitions
│   │   ├── entity_counts.rs    #   get_entity_counts
│   │   ├── path_opener.rs      #   detect_installed_openers, open_path_with_app, ...
│   │   ├── settings.rs         #   get_app_info, get_dev_info
│   │   ├── node.rs             #   custom: archive_node() alongside generated CRUD
│   │   └── mod.rs              #   re-exports generated/ + custom modules
│   └── transport/
│       ├── http/generated.rs   # ◄ Generated from merged ApiOutput
│       ├── ipc/generated.rs
│       └── mcp/generated.rs
```

### The Pattern: Generated + Custom at Every Layer

The generated/custom split is the **same concept** at both store and API levels:

| Layer | Generated (`generated/`) | Hooks / Custom | How they connect |
|-------|--------------------------|-----------------|------------------|
| **Store** | CRUD methods, update structs, relation population | `hooks/` — scaffolded once, fill in validation logic; `custom/` — additional store methods (e.g. `bulk_reparent_nodes`) | Generated CRUD imports and calls `hooks::entity::before_create(...)` etc.; custom store methods are additional `impl Store` blocks |
| **API** | CRUD forwarding (list, get, create, update, delete) | Non-entity modules (graph, events) + extra entity functions (archive) | Transport merges both into one `ApiModule` per entity |

**Store-level custom methods** work just like API-level custom functions. If you need a `bulk_reparent_nodes()` that doesn't fit the CRUD pattern:

```rust
// Hand-written: src/store/custom/node_custom.rs
impl Store {
    /// Reparent multiple nodes in a single transaction.
    /// This is custom business logic that can't be generated from schema alone.
    pub async fn bulk_reparent_nodes(
        &self,
        node_ids: &[String],
        new_parent_id: &str,
    ) -> Result<Vec<Node>, AppError> {
        // Custom transactional logic...
        for id in node_ids {
            // This calls the GENERATED update method internally
            let updates = NodeUpdate {
                parent_id: Some(Some(new_parent_id.to_string())),
                ..Default::default()
            };
            self.update_node(id, updates).await?;
        }
        Ok(/* ... */)
    }
}
```

Then an API function in `api/v1/node.rs` can call this custom store method:

```rust
// Hand-written: src/api/v1/node.rs
pub async fn bulk_reparent(
    store: &Store,
    input: BulkReparentInput,
) -> Result<Vec<Node>, AppError> {
    store.bulk_reparent_nodes(&input.node_ids, &input.new_parent_id).await
}
```

And the transport generator picks it up via scan, merging it with the generated CRUD functions.

### How gen_api Merges Sources

```rust
pub fn gen_api(
    entities: &[EntityDef],
    store: Option<&StoreOutput>,
    scan_dirs: &[&str],
    config: &ApiConfig,
) -> Result<ApiOutput, CodegenError> {
    let mut modules: Vec<ApiModule> = Vec::new();

    // Source 1: Generate CRUD modules from entity metadata
    for entity in entities {
        if config.is_excluded(entity, "v1") { continue; }

        let generated_module = generate_crud_module(entity, store);
        write_file(&config.generated_output, &generated_module)?;
        modules.push(generated_module);
    }

    // Source 2: Scan hand-written API directories for custom modules
    for dir in scan_dirs {
        let scanned = scan_api_dir(dir, &config.state_type, config.store_type)?;
        for scanned_module in scanned {
            // If a scanned module has the same entity name as a generated one,
            // merge the custom functions INTO the generated module's ApiModule.
            // This lets you add `archive_node()` alongside generated CRUD.
            if let Some(existing) = modules.iter_mut().find(|m| m.name == scanned_module.name) {
                existing.merge_custom_functions(&scanned_module);
            } else {
                // Purely custom module (graph, events, project, etc.)
                modules.push(scanned_module);
            }
        }
    }

    Ok(ApiOutput { modules })
}
```

This means:
- **graph.rs, events.rs, project.rs, etc.** — scanned and included as-is
- **node.rs** — if it exists in both `v1/generated/` and `v1/` (hand-written), the custom functions are merged into the generated module's `ApiModule`, so the server generator sees everything
- **agent.rs** — if only generated exists, that's all transport sees

## Key Design Decisions

### 1. Independent Functions over Monolithic Pipeline

**Decision:** Each generator is a standalone `pub fn` with explicit inputs, not methods on a pipeline builder.

**Rationale:**
- You can run `gen_servers` + `gen_clients` alone against scanned source files — no schema parsing needed
- You can run `gen_seaorm` + `gen_store` without generating servers/clients (useful for a library crate that has no API)
- Testing is straightforward: construct inputs, call function, assert outputs
- The chain is explicit in build.rs — you see exactly what flows where

**Trade-off:** Slightly more verbose build.rs compared to a fluent builder. But the clarity of explicit data flow is worth it.

### 2. Optional Upstream Enrichment

Each generator works without upstream output, but produces **richer code** when it has it:

| Generator | Without upstream | With upstream |
|-----------|-----------------|---------------|
| `gen_store` | Infers junction tables from `#[ontology(relation)]` | Gets exact table/column names from `SeaOrmOutput` |
| `gen_api` | Generates CRUD assuming standard store method names | Knows exact method signatures from `StoreOutput` |
| `gen_servers` | Parses API source files with `syn` (current behavior) | Receives structured `ApiModule` metadata — no parsing needed |

The independent mode is the **fallback**; the chained mode is the **optimization**.

### 3. Store Generation with Scaffold Hooks

The generated store provides the full CRUD boilerplate. For lifecycle customization, it uses the **scaffold pattern**: a `hooks/` directory of plain functions that the generated CRUD imports and calls. Hook files are generated once as no-op stubs — you fill them in with your logic. They're never overwritten on subsequent builds.

**Two directories per entity:**

```
store/
├── generated/          # ◄ Overwritten every build
│   ├── node.rs         #   CRUD methods — imports and calls hooks/node.rs
│   ├── task.rs
│   └── ...
├── hooks/              # ◄ Scaffolded once, then yours to edit
│   ├── node.rs         #   before_create, after_create, etc. (starts as no-ops)
│   ├── task.rs         #   (all no-ops until you need custom logic)
│   └── mod.rs
```

**Scaffolded hooks file** (generated once, never overwritten):

```rust
// store/hooks/node.rs (SCAFFOLDED — fill in as needed)

use crate::schema::Node;
use crate::store::Store;
use crate::store::generated::node::NodeUpdate;
use crate::types::AppError;

/// Called before a node is inserted. Modify node or return Err to reject.
pub async fn before_create(_store: &Store, _node: &mut Node) -> Result<(), AppError> {
    Ok(())
}

/// Called after a node is inserted.
pub async fn after_create(_store: &Store, _node: &Node) -> Result<(), AppError> {
    Ok(())
}

/// Called before update. Receives current state and pending changes.
pub async fn before_update(
    _store: &Store,
    _current: &Node,
    _updates: &NodeUpdate,
) -> Result<(), AppError> {
    Ok(())
}

/// Called after successful update.
pub async fn after_update(_store: &Store, _node: &Node) -> Result<(), AppError> {
    Ok(())
}

/// Called before deletion.
pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

/// Called after successful deletion.
pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
```

**Generated CRUD** (regenerated every build, calls the hooks):

```rust
// store/generated/node.rs (REGENERATED — imports hooks)

use crate::store::hooks::node as hooks;

impl Store {
    pub async fn create_node(&self, mut node: Node) -> Result<Node, AppError> {
        hooks::before_create(self, &mut node).await?;

        let id = node.id.clone();
        let fulfills = node.fulfills.clone();
        let contains = node.contains.clone();

        let active = node.to_active_model();
        active.insert(self.db()).await?;

        // Generated from #[ontology(relation(many_to_many, target = "Requirement"))]
        self.sync_junction("node_fulfills", "node_id", "requirement_id", &id, &fulfills).await?;

        // Generated from #[ontology(relation(has_many, target = "Node", foreign_key = "parent_id"))]
        self.sync_has_many_parent("nodes", "parent_id", &id, &contains).await?;

        self.emit_change(ChangeOp::Created, EntityKind::Node, id.clone());
        let result = self.get_node(&id).await?;

        hooks::after_create(self, &result).await?;
        Ok(result)
    }

    // list, get, update, delete follow the same pattern...
}
```

**Adding custom logic** — just fill in the stub:

```rust
// store/hooks/node.rs (YOUR EDITS)

pub async fn before_update(
    store: &Store,
    current: &Node,
    updates: &NodeUpdate,
) -> Result<(), AppError> {
    // Custom containment validation — just code inline
    if let Some(ref contains) = updates.contains {
        validate_no_circular_containment(store, &current.id, contains).await?;
    }
    Ok(())
}
```

No traits, no registration, no wiring. The generated CRUD calls `hooks::before_update(...)` — you just write the function body.

### 4. Change Channels (Optional Per-Entity Broadcasts)

When `change_channels: true`, the store generates typed per-entity channels:

```rust
// Generated
impl Store {
    pub fn node_changes(&self) -> broadcast::Receiver<EntityChange<Node>> {
        self.channels.node.subscribe()
    }

    pub fn requirement_changes(&self) -> broadcast::Receiver<EntityChange<Requirement>> {
        self.channels.requirement.subscribe()
    }
}

pub struct EntityChange<T> {
    pub op: ChangeOp,
    pub entity: Option<T>,  // Some for Create/Update, None for Delete
    pub id: String,
}
```

This replaces the current generic `EntityChange` with typed channels, enabling subscribers to receive the full entity payload without re-fetching.

### 5. Instrumentation Hooks

When `instrumentation: true`, the generated store wraps operations with tracing spans:

```rust
pub async fn create_node(&self, mut node: Node) -> Result<Node, AppError> {
    let _span = tracing::info_span!("store.create", entity = "node", id = %node.id).entered();
    // ... generated body ...
}
```

Additional instrumentation can be added via scaffold hook functions without modifying generated code.

## Also Generated (Currently Hand-Written)

### Update Structs + Apply Methods

The `NodeUpdate` struct (with `Option<Option<T>>` for nullable field clearing) and its `apply()` method are fully mechanical. Generate them from entity metadata:

```rust
// Generated: part of store or dto layer
pub struct NodeUpdate {
    pub name: Option<String>,
    pub kind: Option<Option<NodeKind>>,     // outer: provided?, inner: clear?
    pub parent_id: Option<Option<String>>,
    pub contains: Option<Vec<String>>,
    pub fulfills: Option<Vec<String>>,
    // ... all fields from EntityDef
}

impl NodeUpdate {
    pub fn apply(&self, node: &mut Node) {
        if let Some(v) = &self.name { node.name.clone_from(v); }
        if let Some(v) = &self.kind { node.kind.clone_from(v); }
        // ... generated for each field
    }
}

impl From<UpdateNodeInput> for NodeUpdate { /* generated */ }
```

### From\<CreateInput\> Conversions

The `From<CreateNodeInput> for Node` impls (with wikilink stripping on relation fields) are boilerplate derivable from schema annotations:

```rust
// Generated
impl From<CreateNodeInput> for Node {
    fn from(input: CreateNodeInput) -> Self {
        Self {
            id: input.id,
            name: input.name,
            kind: input.kind,
            // Relation fields: auto-strip wikilinks (known from #[ontology(relation(...))])
            contains: strip_wikilinks_vec(input.contains),
            fulfills: strip_wikilinks_vec(input.fulfills),
            // Non-relation vecs: pass through
            tags: input.tags,
            // ... remaining fields
            ..Default::default()
        }
    }
}
```

### populate_relations()

Currently hand-written per entity. The schema already has all relation metadata:

```rust
// Generated as part of store layer
impl Store {
    pub(crate) async fn populate_node_relations(&self, node: &mut Node) -> Result<(), AppError> {
        // From #[ontology(relation(has_many, target = "Node", foreign_key = "parent_id"))]
        node.contains = self.load_has_many_ids("nodes", "parent_id", &node.id).await?;

        // From #[ontology(relation(many_to_many, target = "Requirement"))]
        node.fulfills = self.load_junction_ids("node_fulfills", "node_id", "requirement_id", &node.id).await?;

        Ok(())
    }
}
```

## Migration Path

Incremental, not a rewrite. Each phase produces working code.

**Phase 1: Consolidate crates** (low risk) — COMPLETE
- Created unified crate consolidating both prior schema-codegen and service-codegen crates
- Module hierarchy: `schema/`, `persistence/`, `servers/`, `clients/`
- Established IR types in `ir.rs` (`SchemaOutput`, `SeaOrmOutput`, `StoreOutput`, `ApiOutput`, `ServersOutput`)
- Top-level generator functions (`parse_schema`, `gen_seaorm`, `gen_dtos`, `gen_servers`, `gen_clients`)
- All 50 existing tests pass in new structure
- Original crates untouched — no behavioral change

**Phase 2: Generate Store layer** (medium risk) — COMPLETE
- Generates CRUD methods, Update structs, From<> impls, populate_relations for all entities
- Handles three complexity tiers: simple, junction (many_to_many), has_many
- 64 tests passing including integration tests against real schemas

**Phase 3: Generate API layer** (low risk) — COMPLETE
- Generates CRUD forwarding modules into `api/v1/generated/`
- Scans hand-written API directories for custom modules (graph, events, project, etc.)
- Merges generated + scanned into unified `ApiOutput` with correct `StateKind` and `OpKind`
- Added `doc` field to `ApiFnMeta` IR for MCP/OpenAPI documentation
- 73 tests passing (9 new API tests including scan+merge integration tests)

**Phase 4: Generate remaining boilerplate** (low risk) — ABSORBED INTO PHASE 2
- Update structs + apply methods → done in Phase 2
- From<Input> conversions → done in Phase 2
- populate_relations() → done in Phase 2

**Phase 5: Wire build.rs and swap hand-written code** — COMPLETE
- Replaced prior codegen build-dependencies with single `ontogen`
- `build.rs` now calls full pipeline: `parse_schema` → `gen_seaorm` + `gen_markdown_io` + `gen_dtos` → `gen_store` → `gen_api`
- Deleted 7 hand-written store entity modules (~1,487 lines of boilerplate):
  - Role, Agent, Node, Requirement, Specification, Task, WorkSession
- Deleted 7 hand-written API CRUD forwarding modules (~230 lines of boilerplate):
  - Role, Agent, Node, Requirement, Specification, Task, WorkSession
- Store uses `pub mod generated; pub mod hooks; pub use generated::*;` pattern
  - `store/generated/*.rs` — regenerated each build (gitignored)
  - `store/hooks/*.rs` — scaffolded once, never overwritten (committed)
- API uses same pattern: `pub mod generated; pub use generated::*;` in `v1.rs`
  - `api/v1/generated/*.rs` — regenerated each build (gitignored)
  - Custom modules (graph, events, project, etc.) remain hand-written alongside
- Transport scanner (`scan_api_dir`) enhanced to scan subdirectories, finding both
  hand-written `api/v1/*.rs` and generated `api/v1/generated/*.rs` files
- Added lifecycle hook callpoints to all generated CRUD: `before_create`, `after_create`, `before_update`, `after_update`, `before_delete`, `after_delete`
- Fixed wikilink stripping for all relation types:
  - `Vec<String>` (ManyToMany/HasMany) → `strip_wikilinks_vec`
  - `Option<String>` (BelongsTo) → `strip_wikilink_opt`
  - `String` (required BelongsTo) → `strip_wikilink`
- Fixed `FieldType::Other` to fully qualify with `crate::schema::` (e.g., `SpecificationStatus`)
- Contract and Evidence excluded from gen_store/gen_api (manual ActiveModel/JSON column handling)
- Test entity excluded pending `AppError::TestNotFound` / `EntityKind::Test` support
- 76 ontogen tests + all integration tests + 6 wikilink normalization tests pass

**Phase 5b: Enable all entities in codegen** — COMPLETE
- Added `AppError::TestNotFound`, `ContractNotFound`, `EvidenceNotFound` variants
- Added `EntityKind::Test` + `tests_changed` to `GraphDelta`
- Removed `#[ontology(skip)]` from Contract's `states`, `transitions`, `forbid`, `endpoints` fields
  - Plain `Vec<String>` fields are stored as JSON text columns and handled by existing codegen pipeline
- Removed all store/API exclusions — all 10 entities now generated by codegen
- Deleted hand-written `store/contract.rs` (225 lines), `store/evidence.rs` (118 lines)
- Deleted hand-written `api/v1/contract.rs` (34 lines), `api/v1/evidence.rs` (34 lines)
- Moved Contract/Evidence prefix validation warnings into `hooks/contract.rs` and `hooks/evidence.rs`
- Added `tests` table migration (`m20240110_000001_create_tests_table`)
- Added `EntityKind::Test` handling in `subscribers.rs` (fs write, file path, db delete)
- 142 tests passing (11 CRUD roundtrip tests including new Contract, Evidence, Test)

**Phase 6: Change channels and instrumentation** (additive) — REMAINING
- Add typed per-entity channels
- Add tracing instrumentation
- Both purely additive — no breaking changes

## What Stays Hand-Written

| What | Where | Why |
|------|-------|-----|
| Schema definitions | `src/schema/*.rs` | Source of truth — always authored by humans |
| Hook implementations | `src/store/hooks/*.rs` | Custom business logic (validation, containment, etc.) |
| Status machines | `schema/common.rs` | Domain logic, not boilerplate |
| Custom API modules | `src/api/v1/*.rs` | graph, events, project, status, path_opener, settings, entity_counts |
| Build configuration | `build.rs` | Pipeline setup and entity overrides |
| Migrations | `src/persistence/db/migrations/` | Schema evolution requires human judgment |
| Store helpers | `src/store/store.rs` | `sync_junction()`, `load_junction_ids()`, `emit_change()` — shared infrastructure |

## File Ownership: Current vs Proposed

| File | Current | Proposed |
|------|---------|----------|
| `schema/*.rs` | Hand-written | Hand-written (no change) |
| `schema/dto/*.rs` | Generated (schema-codegen, prior) | Generated (unified, persistence layer) |
| `persistence/db/entities/generated/*.rs` | Generated (schema-codegen, prior) | Generated (unified, persistence layer) |
| `persistence/db/conversions/generated/*.rs` | Generated (schema-codegen, prior) | Generated (unified, persistence layer) |
| `persistence/fs_markdown/generated/*.rs` | Generated (schema-codegen, prior) | Generated (unified, persistence layer) |
| `store/*.rs` | **Hand-written** | `store/generated/*.rs` **Generated** + `store/hooks/*.rs` hand-written |
| `api/v1/*.rs` | **Hand-written** (CRUD + custom) | `api/v1/generated/*.rs` **Generated** CRUD; `api/v1/*.rs` custom modules only |
| `api/transport/*/generated.rs` | Generated (service-codegen, prior) | Generated (unified, transport layer) |
| `src-nuxt/**/generated.ts` | Generated (service-codegen, prior) | Generated (unified, transport layer) |

## Resolved Decisions

1. **Crate structure**: Single crate. Simpler dependency graph, no cross-crate type sharing overhead.

2. **Output struct serialization**: Deferred to post-v1. Adding `#[derive(Serialize, Deserialize)]` to output structs is trivial and has no architectural impact.

3. **Generated mod.rs management**: Parent `mod.rs` is hand-written, uses `pub use generated::*` and `pub use custom::*` glob re-exports to flatten the namespace. Callers see a unified API without knowing which module a symbol came from. Name collisions between generated and custom code produce a compile error — catching conflicts at build time.

```rust
// src/store/mod.rs (hand-written, never overwritten)
mod generated;
pub use generated::*;    // flattens all generated CRUD, update structs, etc.

mod hooks;               // scaffold hooks (imported by generated code directly)

mod custom;
pub use custom::*;       // flattens custom store methods (e.g. bulk_reparent_nodes)
```
