// TODO: review — rewritten to re-export from ontogen-core, config types kept here
#![allow(
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::uninlined_format_args,
    clippy::doc_markdown,
    clippy::match_wildcard_for_single_variants,
    clippy::manual_let_else,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls
)]

//! Ontogen — build-time code generator for ontology-driven applications.
//!
//! Generates code from schema definitions through a layered pipeline:
//!
//! ```text
//! parse_schema → SchemaOutput
//!     ├── gen_seaorm      → SeaOrmOutput
//!     ├── gen_markdown_io → ()
//!     ├── gen_dtos        → ()
//!     └── gen_store       → StoreOutput
//!         └── gen_api     → ApiOutput
//!             └── gen_servers → ServersOutput
//!                 └── gen_clients → ()
//! ```
//!
//! Each generator is a standalone function. Upstream outputs are `Option` parameters —
//! enrichment, not requirements. Generators can run independently or be chained.

pub mod api;
pub mod clients;
pub mod persistence;
pub mod schema;
pub mod servers;
pub mod store;

// Re-export ontogen-core as the canonical source for shared types.
// Internal modules should import from `ontogen_core` directly.
// External consumers can use `ontogen::` for everything.
pub use ontogen_core::ir;
pub use ontogen_core::model;
pub use ontogen_core::naming;
pub use ontogen_core::utils;

// Re-export key types for ergonomic use in build.rs
pub use ontogen_core::CodegenError;
pub use ontogen_core::ir::*;
pub use ontogen_core::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};
pub use ontogen_core::naming::{pluralize, to_pascal_case, to_snake_case};
pub use ontogen_core::utils::{clean_generated_dir, emit_rerun_directives, prettier, rustfmt};

use std::path::PathBuf;

// ── Top-level generator functions ───────────────────────────────────
//
// These are the public API. Each wraps the corresponding module's logic
// and returns a typed output struct for downstream consumption.

/// Parse schema files. Always the starting point when entity metadata is needed.
pub fn parse_schema(config: &SchemaConfig) -> Result<SchemaOutput, CodegenError> {
    emit_rerun_directives(&config.schema_dir);
    let entities = schema::parse::parse_schema_dir(&config.schema_dir).map_err(CodegenError::Schema)?;
    Ok(SchemaOutput { entities })
}

/// Generate SeaORM entities, junction tables, and model conversions.
pub fn gen_seaorm(entities: &[EntityDef], config: &SeaOrmConfig) -> Result<SeaOrmOutput, CodegenError> {
    persistence::seaorm::generate(entities, config)
}

/// Generate markdown I/O: parser dispatch, writers, path helpers, and fs_ops.
pub fn gen_markdown_io(entities: &[EntityDef], config: &MarkdownIoConfig) -> Result<(), CodegenError> {
    persistence::markdown::generate(entities, config)
}

/// Generate Create/Update DTOs as standalone types.
/// Also invoked internally by `gen_store`, but available independently
/// for consumers who want input types without a full store.
pub fn gen_dtos(entities: &[EntityDef], config: &DtoConfig) -> Result<(), CodegenError> {
    persistence::dto::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)
}

/// Generate store layer: CRUD methods, Update structs, From impls, and
/// populate_relations helpers.
///
/// When `seaorm` is `Some`, uses structured metadata for exact table/column names.
/// When `None`, infers junction table names from entity/field naming conventions.
pub fn gen_store(
    entities: &[EntityDef],
    seaorm: Option<&SeaOrmOutput>,
    config: &StoreConfig,
) -> Result<StoreOutput, CodegenError> {
    store::generate(entities, seaorm, config)
}

/// Generate API layer: CRUD forwarding functions that delegate to Store methods.
///
/// Generates per-entity modules in `config.output_dir` and returns `ApiOutput`
/// metadata for downstream transport generators.
pub fn gen_api(entities: &[EntityDef], config: &ApiConfig) -> Result<ApiOutput, CodegenError> {
    api::generate(entities, config)
}

/// Generate server transport handlers from API metadata.
///
/// When `api` is `Some`, uses structured metadata for exact routing.
/// When `None`, falls back to scanning source files with `syn`.
pub fn gen_servers(
    api: Option<&ApiOutput>,
    scan_dirs: &[PathBuf],
    config: &ServersConfig,
) -> Result<ServersOutput, CodegenError> {
    servers::generate(api, scan_dirs, config)
}

/// Generate client libraries from server transport metadata.
pub fn gen_clients(
    servers: &ServersOutput,
    api: Option<&ApiOutput>,
    config: &ClientsConfig,
) -> Result<(), CodegenError> {
    clients::generate(servers, api, config)
}

// ── Configuration types ─────────────────────────────────────────────

/// Configuration for schema parsing.
pub struct SchemaConfig {
    /// Path to the schema source directory (e.g., `src/schema/`).
    pub schema_dir: PathBuf,
}

/// Configuration for SeaORM persistence generation.
pub struct SeaOrmConfig {
    /// Output path for generated SeaORM entity code.
    pub entity_output: PathBuf,
    /// Output path for generated DB conversion code.
    pub conversion_output: PathBuf,
    /// Entity names to skip in conversion generation.
    pub skip_conversions: Vec<String>,
}

/// Configuration for markdown I/O generation.
pub struct MarkdownIoConfig {
    /// Output directory for generated writer, parser dispatch, and fs_ops.
    pub output_dir: PathBuf,
}

/// Configuration for standalone DTO generation.
pub struct DtoConfig {
    /// Output path for generated Create/Update input types.
    pub output_dir: PathBuf,
}

/// Configuration for store layer generation.
pub struct StoreConfig {
    /// Output directory for generated store modules (e.g., `src/store/generated/`).
    pub output_dir: PathBuf,
    /// Directory for scaffolded hook files (e.g., `src/store/hooks/`).
    /// When `Some`, hook files are scaffolded once per entity (never overwritten).
    /// When `None`, hook scaffolding is skipped (generated CRUD still calls hooks —
    /// the consuming crate must provide its own hook modules).
    pub hooks_dir: Option<PathBuf>,
}

/// Configuration for API layer generation.
pub struct ApiConfig {
    /// Output directory for generated API modules (e.g., `src/api/v1/generated/`).
    pub output_dir: PathBuf,
    /// Entity names to exclude from API generation.
    pub exclude: Vec<String>,
    /// Directories to scan for hand-written API modules (e.g., `["src/api/v1"]`).
    /// Scanned modules are merged with generated CRUD modules into a unified `ApiOutput`.
    /// When empty, only generated CRUD modules are included.
    pub scan_dirs: Vec<PathBuf>,
    /// The AppState type name for scanning (e.g., `"AppState"`).
    pub state_type: String,
    /// The Store type name for scanning (e.g., `"Store"`).
    pub store_type: Option<String>,
}

/// Configuration for server transport generation.
pub struct ServersConfig {
    /// Directory to scan for API source files (when not using ApiOutput).
    pub api_dir: PathBuf,
    /// The AppState type name for route handlers.
    pub state_type: String,
    /// Import path for the service module.
    pub service_import_path: String,
    /// Import path for schema types.
    pub types_import_path: String,
    /// Import path for the state type.
    pub state_import: String,
    /// Naming overrides for plural/singular entity names.
    pub naming: servers::NamingConfig,
    /// Which server generators to run.
    pub generators: Vec<servers::ServerGeneratorConfig>,
    /// Rustfmt edition for formatting generated Rust.
    pub rustfmt_edition: String,
    /// SSE route overrides.
    pub sse_route_overrides: std::collections::HashMap<String, String>,
    /// IPC commands to skip in TypeScript generation.
    pub ts_skip_commands: Vec<String>,
    /// Optional route prefix (e.g., `/projects/:project_id`).
    pub route_prefix: Option<servers::RoutePrefix>,
    /// Store type for entity-scoped functions (e.g., `"Store"`).
    pub store_type: Option<String>,
    /// Import path for the Store type.
    pub store_import: Option<String>,
}

/// Configuration for client generation.
pub struct ClientsConfig {
    /// Which client generators to run.
    pub generators: Vec<clients::ClientGeneratorConfig>,
}
