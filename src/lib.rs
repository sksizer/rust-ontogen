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
pub mod ir;
pub mod persistence;
pub mod schema;
pub mod servers;
pub mod store;

// Re-export key types for ergonomic use in build.rs
pub use ir::*;
pub use schema::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};

use std::path::{Path, PathBuf};

/// Run `rustfmt` on a generated Rust file.
/// Silently ignores failures (e.g., if rustfmt is not installed).
pub fn rustfmt(path: &Path) {
    let _ = std::process::Command::new("rustfmt").arg("--edition").arg("2024").arg(path).status();
}

/// Run `prettier` on generated TypeScript files.
/// Silently ignores failures.
pub fn prettier(paths: &[&Path]) {
    if paths.is_empty() {
        return;
    }
    let mut cmd = std::process::Command::new("npx");
    cmd.arg("prettier").arg("--write");
    for p in paths {
        cmd.arg(p);
    }
    let _ = cmd.status();
}

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
///
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

// ── Error type ──────────────────────────────────────────────────────

/// Codegen error with layer context.
#[derive(Debug)]
pub enum CodegenError {
    Schema(String),
    Persistence(String),
    Store(String),
    Api(String),
    Server(String),
    Client(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema(e) => write!(f, "schema codegen error: {e}"),
            Self::Persistence(e) => write!(f, "persistence codegen error: {e}"),
            Self::Store(e) => write!(f, "store codegen error: {e}"),
            Self::Api(e) => write!(f, "api codegen error: {e}"),
            Self::Server(e) => write!(f, "server codegen error: {e}"),
            Self::Client(e) => write!(f, "client codegen error: {e}"),
        }
    }
}

impl std::error::Error for CodegenError {}

// ── Helpers ─────────────────────────────────────────────────────────

/// Remove `.rs` files from `dir` that are not in `expected`.
///
/// Call this at the start of each generator to clean up files left behind
/// by entity renames or deletions.  `expected` should contain bare filenames
/// like `"node.rs"`, `"mod.rs"`, etc.  Files whose names are not in the set
/// are deleted.  Non-`.rs` files and subdirectories are left alone.
pub fn clean_generated_dir(dir: &Path, expected: &std::collections::HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            let name = entry.file_name().to_string_lossy().to_string();
            if !expected.contains(&name) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// Emit `cargo:rerun-if-changed` directives for all `.rs` files in a directory.
fn emit_rerun_directives(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
