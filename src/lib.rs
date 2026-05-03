#![forbid(unsafe_code)]

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

pub mod admin;
pub mod api;
pub mod persistence;
pub mod pipeline;
pub mod schema;
pub mod servers;
pub mod store;

pub use pipeline::Pipeline;

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

/// Parse schema files into structured entity metadata.
///
/// Always the starting point when entity metadata is needed — every other
/// generator consumes the resulting [`SchemaOutput::entities`]. Also emits
/// `cargo:rerun-if-changed` directives for the schema directory so the build
/// script re-runs whenever any schema file is touched.
///
/// # Errors
///
/// Returns [`CodegenError::Schema`] if the directory cannot be read or any
/// schema file fails to parse.
///
/// # Example
///
/// ```ignore
/// use ontogen::{parse_schema, SchemaConfig};
/// use std::path::PathBuf;
///
/// let schema = parse_schema(&SchemaConfig {
///     schema_dir: PathBuf::from("src/schema"),
/// })?;
///
/// println!("parsed {} entities", schema.entities.len());
/// # Ok::<(), ontogen::CodegenError>(())
/// ```
pub fn parse_schema(config: &SchemaConfig) -> Result<SchemaOutput, CodegenError> {
    emit_rerun_directives(&config.schema_dir);
    let entities = schema::parse::parse_schema_dir(&config.schema_dir).map_err(CodegenError::Schema)?;
    Ok(SchemaOutput { entities })
}

/// Generate SeaORM entities, junction tables, and model conversions from parsed schema.
///
/// Emits one entity module per [`EntityDef`] plus junction tables for many-to-many
/// relations. The returned [`SeaOrmOutput`] captures concrete table and column
/// names so [`gen_store`] can produce exact join code rather than inferring it.
///
/// # Errors
///
/// Returns [`CodegenError::Persistence`] on I/O or formatting failure.
///
/// # Example
///
/// ```ignore
/// use ontogen::{gen_seaorm, parse_schema, SchemaConfig, SeaOrmConfig};
/// use std::path::PathBuf;
///
/// let schema = parse_schema(&SchemaConfig {
///     schema_dir: PathBuf::from("src/schema"),
/// })?;
///
/// let seaorm = gen_seaorm(&schema.entities, &SeaOrmConfig {
///     entity_output: PathBuf::from("src/persistence/db/entities"),
///     conversion_output: PathBuf::from("src/persistence/db/conversions"),
///     skip_conversions: vec![],
/// })?;
/// # Ok::<(), ontogen::CodegenError>(())
/// ```
pub fn gen_seaorm(entities: &[EntityDef], config: &SeaOrmConfig) -> Result<SeaOrmOutput, CodegenError> {
    persistence::seaorm::generate(entities, config)
}

/// Generate markdown I/O helpers: parser dispatch, writers, path helpers, and `fs_ops`.
///
/// Use this when entities round-trip through markdown files on disk. The generated
/// code provides per-entity read and write functions plus a generic dispatcher.
///
/// # Errors
///
/// Returns [`CodegenError::Persistence`] on I/O or formatting failure.
///
/// # Example
///
/// ```ignore
/// use ontogen::{gen_markdown_io, parse_schema, MarkdownIoConfig, SchemaConfig};
/// use std::path::PathBuf;
///
/// let schema = parse_schema(&SchemaConfig {
///     schema_dir: PathBuf::from("src/schema"),
/// })?;
///
/// gen_markdown_io(&schema.entities, &MarkdownIoConfig {
///     output_dir: PathBuf::from("src/persistence/markdown/generated"),
/// })?;
/// # Ok::<(), ontogen::CodegenError>(())
/// ```
pub fn gen_markdown_io(entities: &[EntityDef], config: &MarkdownIoConfig) -> Result<(), CodegenError> {
    persistence::markdown::generate(entities, config)
}

/// Generate `Create` / `Update` input DTOs as standalone types.
///
/// Invoked internally by [`gen_store`], but exposed here for consumers who
/// want strongly typed input structs without pulling in the full store layer
/// (e.g. for an HTTP-only crate that posts payloads to a remote service).
///
/// # Errors
///
/// Returns [`CodegenError::Persistence`] on I/O or formatting failure.
///
/// # Example
///
/// ```ignore
/// use ontogen::{gen_dtos, parse_schema, DtoConfig, SchemaConfig};
/// use std::path::PathBuf;
///
/// let schema = parse_schema(&SchemaConfig {
///     schema_dir: PathBuf::from("src/schema"),
/// })?;
///
/// gen_dtos(&schema.entities, &DtoConfig {
///     output_dir: PathBuf::from("src/dtos/generated"),
/// })?;
/// # Ok::<(), ontogen::CodegenError>(())
/// ```
pub fn gen_dtos(entities: &[EntityDef], config: &DtoConfig) -> Result<(), CodegenError> {
    persistence::dto::generate(entities, &config.output_dir).map_err(CodegenError::Persistence)
}

/// Generate the store layer: CRUD methods, `Update` structs, `From` impls,
/// and `populate_relations` helpers.
///
/// The `seaorm` parameter is **reserved for future enrichment** (e.g., using
/// exact column names from [`SeaOrmOutput`] instead of deriving them by
/// convention) and currently has no effect on generated output. Pass `None`
/// today; passing `Some(_)` is forward-compatible but produces identical code.
///
/// # Errors
///
/// Returns [`CodegenError`] variants for schema, persistence, or I/O failures.
///
/// # Example
///
/// ```ignore
/// use ontogen::{gen_seaorm, gen_store, parse_schema, SchemaConfig, SeaOrmConfig, StoreConfig};
/// use std::path::PathBuf;
///
/// let schema = parse_schema(&SchemaConfig {
///     schema_dir: PathBuf::from("src/schema"),
/// })?;
///
/// let seaorm = gen_seaorm(&schema.entities, &SeaOrmConfig {
///     entity_output: PathBuf::from("src/persistence/db/entities"),
///     conversion_output: PathBuf::from("src/persistence/db/conversions"),
///     skip_conversions: vec![],
/// })?;
///
/// let store = gen_store(&schema.entities, Some(&seaorm), &StoreConfig {
///     output_dir: PathBuf::from("src/store/generated"),
///     hooks_dir: Some(PathBuf::from("src/store/hooks")),
///     schema_module_path: "crate::schema".into(),
/// })?;
/// # Ok::<(), ontogen::CodegenError>(())
/// ```
pub fn gen_store(
    entities: &[EntityDef],
    seaorm: Option<&SeaOrmOutput>,
    config: &StoreConfig,
) -> Result<StoreOutput, CodegenError> {
    store::generate(entities, seaorm, config)
}

/// Generate the API layer: CRUD forwarding functions that delegate to store methods.
///
/// Emits one module per entity in `config.output_dir` and merges any hand-written
/// modules under `config.scan_dirs` into the returned [`ApiOutput`], which feeds
/// [`gen_servers`] for transport-layer codegen.
///
/// # Errors
///
/// Returns [`CodegenError::Api`] on I/O, parsing, or formatting failure.
///
/// # Example
///
/// ```ignore
/// use ontogen::{gen_api, parse_schema, ApiConfig, SchemaConfig};
/// use std::path::PathBuf;
///
/// let schema = parse_schema(&SchemaConfig {
///     schema_dir: PathBuf::from("src/schema"),
/// })?;
///
/// let api = gen_api(&schema.entities, &ApiConfig {
///     output_dir: PathBuf::from("src/api/v1/generated"),
///     exclude: vec![],
///     scan_dirs: vec![PathBuf::from("src/api/v1")],
///     state_type: "AppState".into(),
///     store_type: Some("Store".into()),
///     schema_module_path: "crate::schema".into(),
/// })?;
/// # Ok::<(), ontogen::CodegenError>(())
/// ```
pub fn gen_api(entities: &[EntityDef], config: &ApiConfig) -> Result<ApiOutput, CodegenError> {
    api::generate(entities, config)
}

/// Generate server transport handlers (Axum, Tauri, etc.) and TypeScript clients
/// from API metadata.
///
/// Currently, this function always scans `config.api_dir` with `syn`,
/// regardless of `api` and `scan_dirs`. Both parameters are **reserved for
/// future enrichment** — `api` for using structured metadata directly without
/// re-parsing, and `scan_dirs` for additional scan locations beyond
/// `config.api_dir`. They have no effect today; pass `None` and `&[]`
/// respectively. Both signatures are kept stable for forward compatibility.
///
/// The set of transports emitted is controlled by
/// [`ServersConfig::generators`] and [`ServersConfig::client_generators`].
///
/// # Errors
///
/// Returns [`CodegenError`] variants for parsing, I/O, or formatting failure.
///
/// # Example
///
/// ```ignore
/// use ontogen::{gen_api, gen_servers, parse_schema, ApiConfig, SchemaConfig, ServersConfig};
/// use std::collections::HashMap;
/// use std::path::PathBuf;
///
/// let schema = parse_schema(&SchemaConfig {
///     schema_dir: PathBuf::from("src/schema"),
/// })?;
///
/// let api = gen_api(&schema.entities, &ApiConfig {
///     output_dir: PathBuf::from("src/api/v1/generated"),
///     exclude: vec![],
///     scan_dirs: vec![PathBuf::from("src/api/v1")],
///     state_type: "AppState".into(),
///     store_type: Some("Store".into()),
///     schema_module_path: "crate::schema".into(),
/// })?;
///
/// gen_servers(
///     Some(&api),
///     &[PathBuf::from("src/api/v1")],
///     &ServersConfig {
///         api_dir: PathBuf::from("src/api/v1"),
///         state_type: "AppState".into(),
///         service_import_path: "crate::service".into(),
///         types_import_path: "crate::schema".into(),
///         state_import: "crate::AppState".into(),
///         naming: Default::default(),
///         generators: vec![],
///         client_generators: vec![],
///         rustfmt_edition: "2021".into(),
///         sse_route_overrides: HashMap::new(),
///         ts_skip_commands: vec![],
///         route_prefix: None,
///         store_type: Some("Store".into()),
///         store_import: Some("crate::Store".into()),
///         pagination: None,
///     },
/// )?;
/// # Ok::<(), ontogen::CodegenError>(())
/// ```
pub fn gen_servers(
    api: Option<&ApiOutput>,
    scan_dirs: &[PathBuf],
    config: &ServersConfig,
) -> Result<ServersOutput, CodegenError> {
    servers::generate(api, scan_dirs, config)
}

// ── Configuration types ─────────────────────────────────────────────

/// Configuration for [`parse_schema`].
///
/// Points the parser at a directory of `.rs` schema files. The directory is
/// scanned recursively and every file containing `#[derive(OntologyEntity)]`
/// types contributes one or more [`EntityDef`]s to the output.
pub struct SchemaConfig {
    /// Path to the schema source directory (e.g., `src/schema/`).
    pub schema_dir: PathBuf,
}

/// Configuration for [`gen_seaorm`].
///
/// Drives generation of two sibling output trees: SeaORM entity modules
/// (the `Model` / `ActiveModel` / `Relation` types) and the conversion module
/// that maps between SeaORM models and your domain types.
pub struct SeaOrmConfig {
    /// Output path for generated SeaORM entity code (e.g.,
    /// `src/persistence/db/entities`).
    pub entity_output: PathBuf,
    /// Output path for generated DB conversion code (e.g.,
    /// `src/persistence/db/conversions`).
    pub conversion_output: PathBuf,
    /// Entity names (PascalCase) to skip in conversion generation. Useful for
    /// entities that need hand-written `From` impls because of unusual mappings.
    pub skip_conversions: Vec<String>,
}

/// Configuration for [`gen_markdown_io`].
///
/// All markdown helpers (parser dispatch, writers, path helpers, and the
/// `fs_ops` module) land under a single output directory; downstream code
/// imports them as one cohesive module.
pub struct MarkdownIoConfig {
    /// Output directory for generated writer, parser dispatch, and `fs_ops`
    /// (e.g., `src/persistence/markdown/generated`).
    pub output_dir: PathBuf,
}

/// Configuration for [`gen_dtos`].
///
/// Used when you want input DTOs without a full store layer — for example,
/// in a thin client crate that posts payloads to a remote service. When using
/// [`gen_store`], the same DTOs are emitted automatically and this config is
/// not needed.
pub struct DtoConfig {
    /// Output path for generated `Create` and `Update` input types
    /// (e.g., `src/dtos/generated`).
    pub output_dir: PathBuf,
}

/// Configuration for [`gen_store`].
///
/// The store is the layer between API handlers and SeaORM: it owns CRUD
/// methods, `Update` structs, `From` implementations between DTOs and
/// SeaORM models, and `populate_relations` helpers. `hooks_dir` controls
/// whether per-entity lifecycle hook files are scaffolded for you.
pub struct StoreConfig {
    /// Output directory for generated store modules (e.g., `src/store/generated/`).
    pub output_dir: PathBuf,
    /// Directory for scaffolded hook files (e.g., `src/store/hooks/`).
    /// When `Some`, hook files are scaffolded once per entity (never overwritten).
    /// When `None`, hook scaffolding is skipped — generated CRUD still calls
    /// hooks, so the consuming crate must provide its own hook modules.
    pub hooks_dir: Option<PathBuf>,
    /// Import path for the schema module in generated code (e.g., `"crate::schema"`).
    /// Defaults to `"crate::schema"`.
    pub schema_module_path: String,
}

/// Configuration for [`gen_api`].
///
/// Drives generation of CRUD forwarding functions and metadata collection.
/// Hand-written API modules under [`scan_dirs`](Self::scan_dirs) are parsed
/// with `syn` and merged with generated modules so transports get a unified
/// view of the API surface.
pub struct ApiConfig {
    /// Output directory for generated API modules (e.g., `src/api/v1/generated/`).
    pub output_dir: PathBuf,
    /// Entity names (PascalCase) to exclude from API generation. Useful for
    /// internal-only entities or those with bespoke handlers.
    pub exclude: Vec<String>,
    /// Directories to scan for hand-written API modules (e.g., `["src/api/v1"]`).
    /// Scanned modules are merged with generated CRUD modules into a unified
    /// [`ApiOutput`]. When empty, only generated CRUD modules are included.
    pub scan_dirs: Vec<PathBuf>,
    /// The application state type name used as the first parameter of every
    /// generated handler (e.g., `"AppState"`).
    pub state_type: String,
    /// Optional store type name used when generating store-bound handlers
    /// (e.g., `"Store"`). When `None`, generated CRUD calls free functions.
    pub store_type: Option<String>,
    /// Import path for the schema module in generated code (e.g., `"crate::schema"`).
    /// Defaults to `"crate::schema"`.
    pub schema_module_path: String,
}

/// Configuration for [`gen_servers`].
///
/// Controls both server-side transport handlers (Axum, Tauri IPC, etc.) and
/// client-side artifacts (TypeScript transports, admin registry). The set of
/// outputs is determined by [`generators`](Self::generators) and
/// [`client_generators`](Self::client_generators); leave either empty to
/// disable that side of generation.
pub struct ServersConfig {
    /// Directory to scan for API source files when no [`ApiOutput`] is supplied.
    pub api_dir: PathBuf,
    /// The application state type name used by route handlers (e.g., `"AppState"`).
    pub state_type: String,
    /// Import path for the service module that exposes business logic
    /// (e.g., `"crate::service"`).
    pub service_import_path: String,
    /// Import path for schema types referenced by handlers
    /// (e.g., `"crate::schema"`).
    pub types_import_path: String,
    /// Import path for the state type (e.g., `"crate::AppState"`).
    pub state_import: String,
    /// Naming overrides for plural and singular entity names. See
    /// [`servers::NamingConfig`].
    pub naming: servers::NamingConfig,
    /// Which server transport generators to run (Axum, Tauri IPC, etc.).
    pub generators: Vec<servers::ServerGeneratorConfig>,
    /// Which client generators to run (TypeScript transports, admin registry, etc.).
    pub client_generators: Vec<servers::ClientGenerator>,
    /// Rustfmt edition for formatting generated Rust (e.g., `"2021"`).
    pub rustfmt_edition: String,
    /// SSE route overrides keyed by entity name; values are full URL paths.
    pub sse_route_overrides: std::collections::HashMap<String, String>,
    /// IPC commands to skip in TypeScript transport generation.
    pub ts_skip_commands: Vec<String>,
    /// Optional route prefix applied to every generated route
    /// (e.g., `/projects/:project_id`).
    pub route_prefix: Option<servers::RoutePrefix>,
    /// Store type name for entity-scoped handler functions (e.g., `"Store"`).
    pub store_type: Option<String>,
    /// Import path for the [`store_type`](Self::store_type) (e.g., `"crate::Store"`).
    pub store_import: Option<String>,
    /// Optional pagination configuration for list operations.
    pub pagination: Option<servers::PaginationConfig>,
}

// `AdminLayerConfig` and `install_admin_layer` are re-exported from the
// `admin` module to preserve the public API at `ontogen::install_admin_layer`.
// The implementation now lives in `src/admin.rs` so this file stays focused
// on the core pipeline entry points and config types.
pub use admin::AdminLayerConfig;

/// Install the ontogen admin layer into a Nuxt app — see [`admin::install`].
///
/// Re-exported here for `build.rs` ergonomics. Equivalent to
/// `ontogen::admin::install(config)`.
///
/// # Errors
///
/// Returns [`CodegenError::Client`] if `nuxt.config.ts` cannot be read or
/// written.
pub fn install_admin_layer(config: &AdminLayerConfig) -> Result<(), CodegenError> {
    admin::install(config)
}
