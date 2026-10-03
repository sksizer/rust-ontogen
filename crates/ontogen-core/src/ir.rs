//! Intermediate Representation types that flow between generators.
//!
//! Each generator produces a typed output struct. Downstream generators accept
//! these as `Option<&Output>` parameters - enrichment, not requirements.
//! All merge layers normalize generated + scanned sources into the **same types**.

use std::path::PathBuf;

use crate::model::{EntityDef, EnumDef};

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
    /// The string enums declared beside the entities.
    pub enums: Vec<EnumDef>,
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
/// (ADR 0001). Carries what the store emitter needs: the per-entity
/// metadata that resolves directories, frontmatter type discriminators and
/// the frontmatter module path. The layout, vault root, list cap and OKF
/// options are not here: they reach the runtime only through the generated
/// `open_vault`, and the store code never builds a vault. How ids are
/// derived is the store's choice, not the vault's: see
/// `StoreConfig::id_strategy`.
#[derive(Debug, Clone)]
pub struct MarkdownIoOutput {
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
    /// The OKF `type:` every record of this entity carries, e.g. `Workout`
    /// — also how its records are told apart under [`MarkdownLayout::Flat`].
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
    /// `vault_root/<id>.md`, all entities flat, sharing one id space; each
    /// entity's list, count and get see only records whose frontmatter
    /// `type:` is its own (or absent).
    Flat,
}

/// How the generated store fills the id of a record created without one,
/// on either backend.
///
/// An id that is empty or whitespace-only is absent; any other id (from the
/// caller or a `before_create` hook) is used as given under every strategy.
/// A derived id that is taken, or reserved, is probed as `-2`, `-3`, … .
/// With no id to use, a create returns `AppError::{Entity}IdRequired`.
///
/// There is no default: the store-wide strategy (`StoreConfig::id_strategy`)
/// is always chosen explicitly, and an entity's
/// `#[ontology(entity, id = "...")]` overrides it for that entity.
///
/// Mirrors the markdown runtime crate's `IdStrategy`, which the markdown
/// store emits literally (the runtime crate stays free of ontogen
/// dependencies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdStrategy {
    /// The caller must supply the id.
    Provided,
    /// Slugify the value of the named field (e.g. `title`), which must be a
    /// plain `String` field on every entity the strategy applies to.
    SlugFromField(String),
    /// A fresh UUID v4. A SeaORM store needs `ontogen-core`'s `uuid` feature,
    /// a markdown store `markdown-store`'s.
    Uuid,
}

/// The opt-in OKF 0.2 artifacts a markdown vault writes beside its records.
/// Both are off by default; the vault is an OKF bundle either way.
///
/// The generator bakes them into the emitted `open_vault` constructor, so
/// the build configuration is the one place a consumer sets them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OkfOptions {
    /// Keep a generated `index.md` (OKF §8) in the vault root and in every
    /// directory holding records, regenerated on each real write. The root
    /// index declares `okf_version: "0.2"`.
    pub index: bool,
    /// Stamp `generated: { by, at }` (OKF §5.2) on every real write, with
    /// this value as `by`. It must be an OKF §7 actor naming a program,
    /// `<producer>/<version>` or `process:<id>`; anything else, `human:<id>`
    /// included, fails the build. A schema field stored under the
    /// `generated` key is then a build error too.
    pub generated_by: Option<String>,
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
    /// emitter genuinely needs the per-entity mapping to emit correct code.
    Markdown(MarkdownIoOutput),
}

/// Whether the DTO `From` impls emitted by `gen_store` strip wikilink
/// syntax from relation id fields (`[[id]]` → `id`).
///
/// Each backend has a default — the markdown backend strips at its typed
/// boundary, SQL backends pass ids through untouched — and
/// `StoreConfig::wikilink_policy` can override it for hybrid consumers:
/// a SQL-backed store whose wire contract still accepts wikilinked ids
/// (e.g. an API fed by markdown-authoring agents).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WikilinkPolicy {
    /// Strip `[[id]]` → `id` on every relation field.
    Strip,
    /// Pass relation ids through untouched.
    Passthrough,
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
    /// Event subscription: an SSE route and an IPC subscribe/unsubscribe
    /// command pair. The fn's `params` are the subscription arguments and its
    /// `return_type` is the item type.
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
