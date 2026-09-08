//! Configuration for the server-side transport codegen pipeline.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::servers::types::NamingConfig;

/// Top-level configuration for the server transport codegen pipeline.
///
/// Client-side TypeScript and admin-registry codegen has its own carrier in
/// [`crate::clients::config`]; the two pipelines no longer share a config.
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

    /// Which server-side generators to run and their output paths.
    pub generators: Vec<ServerGenerator>,

    /// Rust edition for `rustfmt` (e.g., `"2021"`).
    pub rustfmt_edition: String,

    /// SSE route overrides: map from event function name to custom route path
    /// (e.g., `"graph_updated"` → `"/api/events/graph"`).
    pub sse_route_overrides: HashMap<String, String>,

    /// Optional route prefix for project scoping.
    ///
    /// When set, generates project-scoped routes (e.g., `/api/projects/{project_id}/nodes`)
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

    /// Optional pagination support for list operations.
    ///
    /// When set, all `OpKind::List` handlers add `limit`/`offset` query params
    /// and wrap return values in `PaginatedResult<T>`.
    pub pagination: Option<PaginationConfig>,

    /// API surfaces scanned in addition to the primary one described by the
    /// fields above. See [`ApiSurface`].
    pub extra_surfaces: Vec<ApiSurface>,
}

impl Config {
    /// The primary surface, assembled from the top-level fields.
    pub(crate) fn primary_surface(&self) -> ApiSurface {
        ApiSurface {
            api_dir: self.api_dir.clone(),
            service_import_path: self.service_import_path.clone(),
            types_import_path: self.types_import_path.clone(),
            store_accessor: None,
            store_type: self.store_type.clone(),
            pagination: self.pagination.clone(),
            paginated_modules: Vec::new(),
            schema_dir: None,
        }
    }

    /// Every surface, primary first. Indexes match `ApiFn::surface`.
    pub(crate) fn surfaces(&self) -> Vec<ApiSurface> {
        std::iter::once(self.primary_surface()).chain(self.extra_surfaces.iter().cloned()).collect()
    }

    /// Pagination for `module`'s list handlers when the fn came from `surface`.
    pub(crate) fn pagination_for(&self, module: &str, surface: usize) -> Option<&PaginationConfig> {
        pagination_for(&self.pagination, &self.extra_surfaces, module, surface)
    }

    /// True when any surface paginates, so the shared `PaginatedResult` types are needed.
    pub(crate) fn any_pagination(&self) -> bool {
        self.pagination.is_some() || self.extra_surfaces.iter().any(|s| s.pagination.is_some())
    }
}

/// Shared by the server and client configs: index 0 is the primary surface,
/// index `n` is `extra_surfaces[n - 1]`.
pub(crate) fn pagination_for<'a>(
    primary: &'a Option<PaginationConfig>,
    extra_surfaces: &'a [ApiSurface],
    module: &str,
    surface: usize,
) -> Option<&'a PaginationConfig> {
    match surface.checked_sub(1) {
        None => primary.as_ref(),
        Some(i) => extra_surfaces[i].pagination_for(module),
    }
}

/// Configuration for pagination support across all list endpoints.
#[derive(Debug, Clone)]
pub struct PaginationConfig {
    /// Default page size when `limit` is not specified.
    pub default_limit: u32,
    /// Maximum allowed page size. Requests above this are clamped.
    pub max_limit: u32,
}

/// One directory of API modules, with the import paths and store accessor
/// its generated handlers use.
///
/// The top-level fields of [`ServersConfig`](crate::ServersConfig) and
/// [`ClientsConfig`](crate::ClientsConfig) describe the primary surface;
/// `extra_surfaces` adds more. All surfaces are merged into the one router,
/// IPC handler, MCP registry and TypeScript transport. A module present in
/// several surfaces becomes one module under one route and command prefix,
/// with the rules that no function name may appear in more than one surface
/// and that the five CRUD functions (`list`, `get_by_id`, `create`, `update`,
/// `delete`) all come from the same surface.
#[derive(Debug, Clone)]
pub struct ApiSurface {
    /// Directory containing this surface's API source files.
    pub api_dir: PathBuf,
    /// Import path for this surface's service modules
    /// (e.g., `"determined_fitness::api"`).
    pub service_import_path: String,
    /// Import path for the types this surface's handlers reference
    /// (e.g., `"determined_fitness::schema"`).
    pub types_import_path: String,
    /// The state method store-scoped handlers call to obtain the store:
    /// `state.{store_accessor}().await`. `None` means `"store"`.
    pub store_accessor: Option<String>,
    /// Store type name for this surface's entity-scoped functions
    /// (e.g., `"Store"`). `None` treats every function as state-scoped.
    pub store_type: Option<String>,
    /// Pagination for this surface's list operations.
    pub pagination: Option<PaginationConfig>,
    /// When non-empty, restricts [`pagination`](Self::pagination) to these
    /// module names. Empty means every module of the surface paginates.
    pub paginated_modules: Vec<String>,
    /// Directory of this surface's `#[ontology(entity)]` structs. The admin
    /// registry parses it for the surface's field definitions; without it
    /// each of the surface's entities ships with `fields: []`. The surface's
    /// TypeScript types come from the long-tail pool either way.
    pub schema_dir: Option<PathBuf>,
}

/// Accessor used when [`ApiSurface::store_accessor`] is `None`.
pub const DEFAULT_STORE_ACCESSOR: &str = "store";

impl ApiSurface {
    /// The effective store accessor method name.
    #[must_use]
    pub fn store_accessor(&self) -> &str {
        self.store_accessor.as_deref().unwrap_or(DEFAULT_STORE_ACCESSOR)
    }

    /// Pagination for `module`, honouring `paginated_modules`.
    #[must_use]
    pub fn pagination_for(&self, module: &str) -> Option<&PaginationConfig> {
        self.pagination
            .as_ref()
            .filter(|_| self.paginated_modules.is_empty() || self.paginated_modules.iter().any(|m| m == module))
    }
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
            route_prefix: None,
            store_type: None,
            store_import: None,
            pagination: None,
            extra_surfaces: Vec::new(),
        }
    }
}

/// A path prefix inserted before entity routes, with extractable parameters.
///
/// For example, `"projects/:project_id"` produces routes like
/// `/api/projects/{project_id}/nodes` (axum 0.8 syntax) and generates
/// handlers that extract `project_id` from the path.
#[derive(Debug, Clone)]
pub struct RoutePrefix {
    /// The path segment(s) to insert, with params in `:name` form
    /// (e.g., `"projects/:project_id"`). Normalized to axum's `{name}`
    /// form when routes are emitted.
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
