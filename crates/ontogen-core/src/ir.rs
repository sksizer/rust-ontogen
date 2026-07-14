//! Intermediate Representation types that flow between generators.
//!
//! Each generator produces a typed output struct. Downstream generators accept
//! these as `Option<&Output>` parameters - enrichment, not requirements.
//! All merge layers normalize generated + scanned sources into the **same types**.

use std::path::PathBuf;

use crate::model::EntityDef;

// ── Source discriminator (shared across all layers) ─────────────────

/// Where did this method/module originate?
/// Used by downstream generators to emit correct import paths.
#[derive(Debug, Clone)]
pub enum Source {
    /// Generated from EntityDef by a codegen layer.
    Generated { module_path: String },
    /// Scanned from a hand-written source file.
    Scanned { module_path: String, file_path: PathBuf },
}

// ── Schema output ───────────────────────────────────────────────────

/// Output from `parse_schema`. The starting point for the pipeline.
pub struct SchemaOutput {
    pub entities: Vec<EntityDef>,
}

// ── Persistence output ──────────────────────────────────────────────

/// SeaORM-specific output. Produced by `gen_seaorm`.
#[derive(Debug, Clone)]
pub struct SeaOrmOutput {
    /// Table names and column mappings per entity.
    pub entity_tables: Vec<EntityTableMeta>,
    /// Junction table metadata for many-to-many relations.
    pub junction_tables: Vec<JunctionMeta>,
    /// Which from_model/to_active_model conversions were generated.
    pub conversion_fns: Vec<ConversionMeta>,
}

/// Metadata about a generated SeaORM entity table.
#[derive(Debug, Clone)]
pub struct EntityTableMeta {
    pub entity_name: String,
    pub table_name: String,
    pub module_path: String,
    pub columns: Vec<ColumnMeta>,
}

/// A single column in a SeaORM entity.
#[derive(Debug, Clone)]
pub struct ColumnMeta {
    pub name: String,
    pub column_type: String,
    pub is_primary_key: bool,
}

/// Metadata about a junction table for many-to-many relations.
#[derive(Debug, Clone)]
pub struct JunctionMeta {
    pub table_name: String,
    pub source_entity: String,
    pub target_entity: String,
    pub source_fk: String,
    pub target_fk: String,
}

/// Metadata about generated from_model/to_active_model conversions.
#[derive(Debug, Clone)]
pub struct ConversionMeta {
    pub entity_name: String,
    pub module_path: String,
}

/// Markdown-backend output. Produced by `gen_markdown_io`; consumed by
/// `gen_store` when emitting CRUD bodies against the markdown runtime
/// (ADR 0001). Carries the vault configuration plus the per-entity
/// metadata the store emitter needs to resolve paths, frontmatter type
/// discriminators, and id derivation.
#[derive(Debug, Clone)]
pub struct MarkdownIoOutput {
    /// Where the `.md` records live, relative to the consumer crate root
    /// (e.g. `data/vault`).
    pub vault_root: PathBuf,
    /// On-disk arrangement of record files under the vault root.
    pub layout: MarkdownLayout,
    /// How new records derive an id when the caller didn't supply one.
    pub id_strategy: IdStrategy,
    /// Hard cap on records parsed per `list()` before the runtime errors —
    /// the ADR's explicit scale ceiling, threaded into the generated
    /// vault construction.
    pub list_cap: usize,
    /// Module path (in the consumer crate) of the markdown-io generated
    /// module the store emitter imports `{Entity}Frontmatter` types from,
    /// e.g. `crate::persistence::markdown::generated`.
    pub module_path: String,
    /// Per-entity metadata, one row per schema entity.
    pub entities: Vec<MarkdownEntityMeta>,
}

/// Per-entity markdown metadata the store emitter indexes by entity name.
#[derive(Debug, Clone)]
pub struct MarkdownEntityMeta {
    /// Entity type name, e.g. `Workout`.
    pub entity_name: String,
    /// Frontmatter `type:` discriminator, e.g. `workout` — how records of
    /// this entity are recognized under [`MarkdownLayout::Flat`].
    pub type_name: String,
    /// Directory segment for [`MarkdownLayout::PerEntityDir`], e.g.
    /// `workouts`.
    pub dir_segment: String,
    /// The field carrying the markdown body (after the frontmatter fence),
    /// if the entity declares one via `#[ontology(body)]`.
    pub body_field: Option<String>,
    /// many_to_many fields whose authoritative side is THIS entity's
    /// frontmatter (the other side is a derived reverse-walk view).
    pub authoritative_m2m: Vec<String>,
}

/// On-disk arrangement of record files under the vault root.
///
/// Generator-side mirror of the markdown runtime crate's `VaultLayout` —
/// the runtime crate stays free of ontogen dependencies, so the generator
/// bridges by emitting the runtime variant literally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownLayout {
    /// `vault_root/<dir_segment>/<id>.md` — the default.
    PerEntityDir,
    /// `vault_root/<id>.md`, all entities flat; relies on the frontmatter
    /// `type:` discriminator and id prefixes for disambiguation.
    Flat,
}

/// Id-derivation strategy for new markdown records.
///
/// Generator-side mirror of the markdown runtime crate's `IdStrategy`
/// (same bridging rationale as [`MarkdownLayout`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdStrategy {
    /// The caller must supply the id; an empty one is a runtime error.
    Provided,
    /// Slugify the value of the named field (e.g. `title`) when the id is
    /// absent, de-duplicating with `-2`, `-3`, … suffixes.
    SlugFromField(String),
    /// A fresh UUID v4 (requires the runtime crate's `uuid` feature).
    Uuid,
}

/// Generation-time persistence backend selector for `gen_store` (ADR 0001).
///
/// Owned (no lifetime) so it can sit in `StoreConfig` and be threaded
/// through the `Pipeline` builder without viral borrows. A closed enum by
/// design: ADR 0001 (alternative C) rejects an out-of-tree backend trait —
/// new backends are added here, by upstream PR.
#[derive(Debug, Clone)]
pub enum Backend {
    /// SeaORM/SQL backend. The payload is reserved for future enrichment
    /// (the SeaORM emitter currently derives everything by convention);
    /// `None` is accepted wherever the metadata isn't available.
    Seaorm(Option<SeaOrmOutput>),
    /// Markdown-file backend. Always carries metadata: the markdown
    /// emitter genuinely needs the vault layout, id strategy, and
    /// per-entity mapping to emit correct code.
    Markdown(MarkdownIoOutput),
}

// ── Store output ────────────────────────────────────────────────────

/// Store layer output. Methods from both generated and scanned sources,
/// normalized into the same `StoreMethodMeta` type.
#[derive(Debug)]
pub struct StoreOutput {
    /// Generated + scanned store methods, same type.
    pub methods: Vec<StoreMethodMeta>,
    /// Scaffolded hook file paths and function names.
    pub scaffolded_hooks: Vec<ScaffoldMeta>,
    /// Per-entity broadcast channels (when enabled).
    pub change_channels: Vec<ChannelMeta>,
}

/// A store method - same type whether generated from schema or scanned from custom/.
#[derive(Debug, Clone)]
pub struct StoreMethodMeta {
    /// Entity this method belongs to (e.g., "Node", "Widget").
    pub entity_name: String,
    /// Method name (e.g., "create_node", "bulk_reparent_nodes").
    pub name: String,
    /// Whether this is a CRUD operation or a custom method.
    pub kind: StoreMethodKind,
    /// Method parameters.
    pub params: Vec<ParamMeta>,
    /// Return type as a string.
    pub return_type: String,
    /// Where this method came from.
    pub source: Source,
}

/// Discriminates CRUD from custom store methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreMethodKind {
    /// A standard CRUD operation.
    Crud(CrudOp),
    /// Anything scanned that doesn't match the CRUD pattern.
    Custom,
}

/// The five standard CRUD operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrudOp {
    List,
    Get,
    Create,
    Update,
    Delete,
}

/// Metadata about a scaffolded hook file.
#[derive(Debug, Clone)]
pub struct ScaffoldMeta {
    pub entity_name: String,
    pub file_path: PathBuf,
    pub functions: Vec<String>,
}

/// Metadata about a per-entity change channel.
#[derive(Debug, Clone)]
pub struct ChannelMeta {
    pub entity_name: String,
    pub subscribe_method: String,
    pub event_type: String,
}

// ── API output ──────────────────────────────────────────────────────

/// API layer output. Modules from both generated and scanned sources,
/// normalized into the same `ApiModule` type.
pub struct ApiOutput {
    /// Generated + scanned API modules, same type.
    pub modules: Vec<ApiModule>,
}

/// An API module - may contain functions from both generated and scanned sources.
#[derive(Debug, Clone)]
pub struct ApiModule {
    /// Module name (e.g., "node").
    pub name: String,
    /// All functions in this module (mixed sources, same type).
    pub fns: Vec<ApiFnMeta>,
    /// Whether functions use AppState or Store as their first parameter.
    pub state_type: StateKind,
}

/// An API function - same type whether generated or scanned.
#[derive(Debug, Clone)]
pub struct ApiFnMeta {
    /// Function name (e.g., "create", "archive").
    pub name: String,
    /// Doc comment (used for MCP tool descriptions, OpenAPI docs, etc.).
    pub doc: String,
    /// Function parameters.
    ///
    /// For state-bearing fns, this is every input *after* the leading
    /// state/store parameter (which the generators inject as the handler's
    /// `State<...>` extractor). For fns marked `#[ontogen::stateless]`,
    /// the IR carries every declared input — there is no leading state
    /// slot to skip.
    pub params: Vec<ParamMeta>,
    /// Return type as a string.
    pub return_type: String,
    /// Where this function came from.
    pub source: Source,
    /// Classified operation for HTTP verb routing.
    pub classified_op: OpKind,
    /// `true` when the source `pub fn` was annotated `#[ontogen::stateless]`.
    ///
    /// Stateless fns opt out of the state/store first-param rule entirely.
    /// Server-transport generators read this flag to emit handlers without
    /// a `State<...>` extractor and without forwarding any positional state
    /// argument. Generated CRUD functions always set this to `false`.
    pub is_stateless: bool,
    /// Per-function override for the emitted IPC command / TS method name.
    ///
    /// Populated from either the source-side `#[ontogen(rename = "...")]`
    /// attribute or the build-side `NamingConfig::command_overrides` map.
    /// When `Some`, server-transport generators use this string verbatim as
    /// the IPC command name (and the TS HTTP client camelCases it for the
    /// method name) in place of the default `{entity}_{fn_name}` scheme.
    /// HTTP route paths and the underlying Rust function name are unaffected.
    /// Generated CRUD functions always set this to `None`.
    pub command_override: Option<String>,
}

/// Whether a function operates on a project-scoped Store or the global AppState.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    AppState,
    Store,
}

/// Classified operation type - drives HTTP method and route structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpKind {
    List,
    GetById,
    Create,
    Update,
    Delete,
    /// `list_{children}(parent_id)` - list child entities of a parent.
    /// Generates `GET /api/<parents>/{parent_id}/<children>`.
    JunctionList {
        /// URL segment for the child collection, e.g. "roles".
        child_segment: String,
    },
    /// `add_{child}(parent_id, child_id)` - add a child to a parent.
    /// Generates `POST /api/<parents>/{parent_id}/<children>`.
    JunctionAdd {
        /// URL segment for the child collection, e.g. "roles".
        child_segment: String,
    },
    /// `remove_{child}(parent_id, child_id)` - remove a child from a parent.
    /// Generates `DELETE /api/<parents>/{parent_id}/<children>/{child_id}`.
    JunctionRemove {
        /// URL segment for the child collection, e.g. "roles".
        child_segment: String,
    },
    /// Custom read (GET with non-standard params).
    CustomGet,
    /// Custom write (POST with non-standard params).
    CustomPost,
    /// SSE event stream.
    EventStream,
}

// ── Server output ───────────────────────────────────────────────────

/// Server transport output. Describes the concrete endpoints generated
/// so client generators can mirror them exactly.
pub struct ServersOutput {
    /// HTTP routes generated.
    pub http_routes: Vec<HttpRouteMeta>,
    /// Tauri IPC commands generated.
    pub ipc_commands: Vec<IpcCommandMeta>,
    /// MCP tool definitions generated.
    pub mcp_tools: Vec<McpToolMeta>,
}

/// Metadata about a generated HTTP route.
#[derive(Debug, Clone)]
pub struct HttpRouteMeta {
    pub method: String,
    pub path: String,
    pub handler_name: String,
    pub module_name: String,
}

/// Metadata about a generated Tauri IPC command.
#[derive(Debug, Clone)]
pub struct IpcCommandMeta {
    pub command_name: String,
    pub params: Vec<ParamMeta>,
    pub return_type: String,
}

/// Metadata about a generated MCP tool.
#[derive(Debug, Clone)]
pub struct McpToolMeta {
    pub tool_name: String,
    pub description: String,
    pub params: Vec<ParamMeta>,
}

// ── Shared types ────────────────────────────────────────────────────

/// A function/method parameter.
#[derive(Debug, Clone)]
pub struct ParamMeta {
    pub name: String,
    pub param_type: String,
}
