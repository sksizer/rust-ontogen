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

use crate::servers::types::NamingConfig;
use crate::servers::{PaginationConfig, RoutePrefix};

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

    /// Optional pagination support for list operations.
    pub pagination: Option<PaginationConfig>,

    /// Additional source roots to merge into the ontogen-ts type pool.
    pub pool_extra_roots: Vec<PathBuf>,
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
