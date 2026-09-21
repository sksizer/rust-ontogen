//! Configuration for the TypeScript client + admin-registry codegen pipeline.
//!
//! Mirrors the public shape of [`ServersConfig`](crate::ServersConfig) but
//! carries only the fields the client generators read. The four client
//! generators ([`ts_bindings`](crate::clients::generators::ts_bindings),
//! [`ts_client`](crate::clients::generators::ts_client),
//! [`transport`](crate::clients::generators::transport), and
//! [`admin`](crate::clients::generators::admin)) consume the crate-internal
//! [`Config`] struct, which is built from the public [`ClientsConfig`] in
//! [`crate::clients::generate`].

use std::collections::HashMap;
use std::path::PathBuf;

use ontogen_core::utils::TsFormatter;

use crate::servers::types::NamingConfig;
use crate::servers::{ApiSurface, PaginationConfig, RoutePrefix};

/// Crate-internal configuration carrier for the client generators.
///
/// Built from the public [`crate::ClientsConfig`] by [`crate::clients::generate`]
/// and threaded into each generator. Mirrors the shape of
/// [`crate::servers::config::Config`] for the fields client generators share
/// with the server-side dispatch (state types, naming, route prefix), plus
/// the client-only fields (`ts_skip_commands`, `schema_entities`,
/// `pool_extra_roots`).
#[derive(Debug, Clone)]
#[allow(dead_code)] // mirrors ClientsConfig's public shape; not every field is currently consumed by the client generators
pub(crate) struct Config {
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

    /// Which client generators to run and their output paths.
    pub generators: Vec<ClientGenerator>,

    /// How to format generated TypeScript (none / custom hook / external command).
    pub ts_formatter: TsFormatter,

    /// SSE route overrides: map from event function name to custom route path.
    pub sse_route_overrides: HashMap<String, String>,

    /// Commands to skip in the TypeScript client (Tauri-only commands).
    pub ts_skip_commands: Vec<String>,

    /// Optional route prefix for project scoping.
    pub route_prefix: Option<RoutePrefix>,

    /// Optional store type for project-scoped data access.
    pub store_type: Option<String>,

    /// Import path for the store type.
    pub store_import: Option<String>,

    /// Schema entity definitions, used by the admin registry generator to emit
    /// per-field metadata (type, role, relation targets, display hints).
    pub schema_entities: Vec<ontogen_core::model::EntityDef>,

    pub schema_enums: Vec<ontogen_core::model::EnumDef>,

    pub label_overrides: HashMap<String, String>,

    /// Optional pagination support for list operations.
    pub pagination: Option<PaginationConfig>,

    /// Additional source roots to merge into the ontogen-ts type pool.
    pub pool_extra_roots: Vec<PathBuf>,

    /// Source paths to omit from the ontogen-ts type pool after scanning.
    /// Each path is rooted at `CARGO_MANIFEST_DIR`, mirroring
    /// [`pool_extra_roots`](Self::pool_extra_roots). Pool entries whose
    /// module path lies under any excluded path are dropped before the
    /// long-tail resolver runs — the canonical use case is excluding
    /// ontogen's own `gen_seaorm` output, whose per-entity `Relation`
    /// enums otherwise collide with any domain type named `Relation`.
    pub pool_exclude_paths: Vec<PathBuf>,

    /// API surfaces scanned in addition to the primary one. See [`ApiSurface`].
    pub extra_surfaces: Vec<ApiSurface>,
}

impl Config {
    /// Every surface, primary first. Indexes match `ApiFn::surface`.
    pub(crate) fn surfaces(&self) -> Vec<ApiSurface> {
        let primary = ApiSurface {
            api_dir: self.api_dir.clone(),
            service_import_path: self.service_import_path.clone(),
            types_import_path: self.types_import_path.clone(),
            store_accessor: None,
            store_type: self.store_type.clone(),
            pagination: self.pagination.clone(),
            paginated_modules: Vec::new(),
            schema_dir: None,
        };
        std::iter::once(primary).chain(self.extra_surfaces.iter().cloned()).collect()
    }

    /// Pagination for `module`'s list methods when the fn came from `surface`.
    pub(crate) fn pagination_for(&self, module: &str, surface: usize) -> Option<&PaginationConfig> {
        crate::servers::config::pagination_for(&self.pagination, &self.extra_surfaces, module, surface)
    }

    /// True when any surface paginates, so the shared `PaginatedResult` type is needed.
    pub(crate) fn any_pagination(&self) -> bool {
        self.pagination.is_some() || self.extra_surfaces.iter().any(|s| s.pagination.is_some())
    }
}

/// Client-side code generators (TypeScript + admin registry).
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
