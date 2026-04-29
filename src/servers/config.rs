//! Configuration for the codegen pipeline.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::servers::types::NamingConfig;

/// Top-level configuration for the server transport codegen pipeline.
#[derive(Debug, Clone)]
pub struct Config {
    /// Directory containing API source files (e.g., `src/api/v1`).
    pub api_dir: PathBuf,

    /// The state type name that service functions take as their first parameter
    /// (e.g., `"AppState"`).
    pub state_type: String,

    /// Import path for the service modules from the consuming crate
    /// (e.g., `"crate::api::v1"`).
    pub service_import_path: String,

    /// Import path for shared types (e.g., `"crate::types"`).
    pub types_import_path: String,

    /// Import path for the state type (e.g., `"crate::AppState"`).
    pub state_import: String,

    /// Naming configuration for pluralization, singularization, and labels.
    pub naming: NamingConfig,

    /// Which generators to run and their output paths.
    pub generators: Vec<GeneratorConfig>,

    /// Rust edition for `rustfmt` (e.g., `"2021"`).
    pub rustfmt_edition: String,

    /// SSE route overrides: map from event function name to custom route path
    /// (e.g., `"graph_updated"` → `"/api/events/graph"`).
    pub sse_route_overrides: HashMap<String, String>,

    /// Commands to skip in the TypeScript client (Tauri-only commands).
    pub ts_skip_commands: Vec<String>,

    /// Optional route prefix for project scoping.
    ///
    /// When set, generates project-scoped routes (e.g., `/api/projects/:project_id/nodes`)
    /// alongside the existing unscoped routes. The prefix params are extracted and used
    /// to validate the project context via the configured state accessor method.
    pub route_prefix: Option<RoutePrefix>,

    /// Optional store type for project-scoped data access.
    ///
    /// When set, service functions whose first parameter matches this type
    /// (e.g., `&Store`) are treated as entity-level functions that operate
    /// on a specific project's data. The generated handlers construct the
    /// store from the state using the appropriate accessor method.
    ///
    /// Functions matching `state_type` remain app-level and get unscoped routes.
    /// Functions matching `store_type` get scoped routes only.
    pub store_type: Option<String>,

    /// Import path for the store type (e.g., `"crate::store::Store"`).
    pub store_import: Option<String>,

    /// Schema entity definitions, used by the admin registry generator to emit
    /// per-field metadata (type, role, relation targets, display hints).
    /// When empty, the admin generator emits entity-level config only (no fields).
    pub schema_entities: Vec<ontogen_core::model::EntityDef>,

    /// Optional pagination support for list operations.
    ///
    /// When set, all `OpKind::List` handlers add `limit`/`offset` query params
    /// and wrap return values in `PaginatedResult<T>`.
    pub pagination: Option<PaginationConfig>,
}

/// Configuration for pagination support across all list endpoints.
#[derive(Debug, Clone)]
pub struct PaginationConfig {
    /// Default page size when `limit` is not specified.
    pub default_limit: u32,
    /// Maximum allowed page size. Requests above this are clamped.
    pub max_limit: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_dir: PathBuf::from("src/api/v1"),
            state_type: "AppState".to_string(),
            service_import_path: "crate::api::v1".to_string(),
            types_import_path: "crate::types".to_string(),
            state_import: "crate::AppState".to_string(),
            naming: NamingConfig::default(),
            generators: Vec::new(),
            rustfmt_edition: "2021".to_string(),
            sse_route_overrides: HashMap::new(),
            ts_skip_commands: Vec::new(),
            route_prefix: None,
            store_type: None,
            store_import: None,
            schema_entities: Vec::new(),
            pagination: None,
        }
    }
}

/// A path prefix inserted before entity routes, with extractable parameters.
///
/// For example, `"projects/:project_id"` produces routes like
/// `/api/projects/:project_id/nodes` and generates handlers that extract
/// `project_id` from the path.
#[derive(Debug, Clone)]
pub struct RoutePrefix {
    /// The path segment(s) to insert (e.g., `"projects/:project_id"`).
    pub segments: String,
    /// The state accessor method to call for validation
    /// (e.g., `"store_for"` → `state.store_for(&project_id)?`).
    pub state_accessor: String,
    /// Parameters extracted from the prefix segments.
    pub params: Vec<PrefixParam>,
}

/// A single parameter extracted from the route prefix.
#[derive(Debug, Clone)]
pub struct PrefixParam {
    /// Parameter name (e.g., `"project_id"`).
    pub name: String,
    /// Rust type (e.g., `"uuid::Uuid"`).
    pub rust_type: String,
    /// TypeScript type (e.g., `"string"`).
    pub ts_type: String,
}

/// Configuration for a specific generator.
#[derive(Debug, Clone)]
pub enum GeneratorConfig {
    /// Generate server-side handler code (Rust).
    Server(ServerGenerator),
    /// Generate client-side code (TypeScript).
    Client(ClientGenerator),
}

/// Server-side code generators (Rust).
#[derive(Debug, Clone)]
pub enum ServerGenerator {
    /// Generate Axum HTTP route handlers.
    HttpAxum {
        /// Output file path (e.g., `src/api/transport/http/generated.rs`).
        output: PathBuf,
    },
    /// Generate Tauri IPC command handlers.
    TauriIpc {
        /// Output file path (e.g., `src/api/transport/ipc/generated.rs`).
        output: PathBuf,
    },
    /// Generate MCP (Model Context Protocol) tool registry.
    Mcp {
        /// Output file path (e.g., `src/api/transport/mcp/generated.rs`).
        output: PathBuf,
    },
}

/// Client-side code generators (TypeScript).
#[derive(Debug, Clone)]
pub enum ClientGenerator {
    /// Generate unified TypeScript transport layer with both HTTP and IPC implementations.
    HttpTauriIpcSplit {
        /// Output file path (e.g., `../src-nuxt/app/transport/generated.ts`).
        output: PathBuf,
        /// Path to `bindings.ts` for type discovery.
        bindings_path: PathBuf,
    },
    /// Generate TypeScript HTTP-only client.
    HttpTs {
        /// Output file path (e.g., `../src-nuxt/app/types/httpCommands.ts`).
        output: PathBuf,
        /// Path to `bindings.ts` for type discovery.
        bindings_path: PathBuf,
    },
    /// Generate admin entity registry (TypeScript).
    AdminRegistry {
        /// Output file path (e.g., `../src-nuxt/layers/admin/generated/admin-registry.ts`).
        output: PathBuf,
    },
}
