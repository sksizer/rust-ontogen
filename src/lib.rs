#![forbid(unsafe_code)]
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
//!             └── gen_servers → ServersOutput  (also emits TypeScript clients)
//! ```
//!
//! Each generator is a standalone function. Upstream outputs are `Option` parameters —
//! enrichment, not requirements. Generators can run independently or be chained.

pub mod api;
pub mod persistence;
pub mod schema;
pub mod servers;
pub mod store;

#[cfg(test)]
mod snapshots;

// Re-export ontogen-core as the canonical source for shared types.
// Internal modules should import from `ontogen_core` directly.
// External consumers can use `ontogen::` for everything.
pub use ontogen_core::ir;
pub use ontogen_core::model;
pub use ontogen_core::naming;
pub use ontogen_core::utils;

// Re-export the derive macro so users only need `ontogen` in their Cargo.toml.
pub use ontogen_macros::OntologyEntity;

// Re-export key types for ergonomic use in build.rs
pub use ontogen_core::CodegenError;
pub use ontogen_core::ir::*;
pub use ontogen_core::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};
pub use ontogen_core::naming::{pluralize, to_pascal_case, to_snake_case};
pub use ontogen_core::utils::{
    clean_generated_dir, emit_rerun_directives, emit_rerun_directives_excluding, rustfmt, write_and_format,
    write_and_format_ts, write_if_changed,
};

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
    /// Import path for the schema module in generated code (e.g., `"crate::schema"`).
    /// Defaults to `"crate::schema"`.
    pub schema_module_path: String,
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
    /// Import path for the schema module in generated code (e.g., `"crate::schema"`).
    /// Defaults to `"crate::schema"`.
    pub schema_module_path: String,
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
    /// Which client generators to run (TypeScript transports, admin registry, etc.).
    pub client_generators: Vec<servers::config::ClientGenerator>,
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
    /// Optional pagination for list operations.
    pub pagination: Option<servers::PaginationConfig>,
}

/// Configuration for installing the admin layer into a Nuxt app.
pub struct AdminLayerConfig {
    /// Path to the Nuxt app's `nuxt.config.ts`.
    pub nuxt_config: PathBuf,
    /// Relative path from the nuxt.config.ts to the admin layer package
    /// (e.g., `"../crates/ontogen/packages/nuxt_admin_layer"`).
    pub layer_path: String,
}

/// Install the ontogen admin layer into a Nuxt app.
///
/// Checks if the `extends` field in `nuxt.config.ts` already includes the
/// admin layer path. If not, adds it. This is idempotent — safe to call
/// from `build.rs` on every build.
pub fn install_admin_layer(config: &AdminLayerConfig) -> Result<(), CodegenError> {
    let content = std::fs::read_to_string(&config.nuxt_config)
        .map_err(|e| CodegenError::Client(format!("Failed to read {}: {e}", config.nuxt_config.display())))?;

    // Already installed — nothing to do
    if content.contains(&config.layer_path) {
        return Ok(());
    }

    let new_content = if content.contains("extends:") || content.contains("extends :") {
        // extends exists but doesn't contain our layer — append to the array
        // Match `extends: [...]` and insert our path
        if let Some(bracket_pos) = content.find("extends:").and_then(|i| content[i..].find('[').map(|j| i + j)) {
            let mut result = String::with_capacity(content.len() + config.layer_path.len() + 10);
            result.push_str(&content[..bracket_pos + 1]);
            result.push_str(&format!("'{}', ", config.layer_path));
            result.push_str(&content[bracket_pos + 1..]);
            result
        } else {
            // extends exists but isn't an array — don't touch it, warn instead
            println!(
                "cargo:warning=ontogen: nuxt.config.ts has `extends` but not as an array — add '{}' manually",
                config.layer_path
            );
            return Ok(());
        }
    } else {
        // No extends field — add one after defineNuxtConfig({
        let insert_marker = "defineNuxtConfig({";
        if let Some(pos) = content.find(insert_marker) {
            let insert_at = pos + insert_marker.len();
            let mut result = String::with_capacity(content.len() + config.layer_path.len() + 30);
            result.push_str(&content[..insert_at]);
            result.push_str(&format!("\n  extends: ['{}'],", config.layer_path));
            result.push_str(&content[insert_at..]);
            result
        } else {
            println!(
                "cargo:warning=ontogen: could not find defineNuxtConfig({{ in nuxt.config.ts — add extends manually"
            );
            return Ok(());
        }
    };

    utils::write_if_changed(&config.nuxt_config, new_content.as_bytes())
        .map_err(|e| CodegenError::Client(format!("Failed to write {}: {e}", config.nuxt_config.display())))
}
