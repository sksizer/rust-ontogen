// TODO: review — doc comments updated, removed old crate references
//! Server transport generators — HTTP (Axum), IPC (Tauri), MCP.
//!
//! Also includes client generators (TypeScript, admin registry) which will
//! move to the `clients` module in a later phase.

pub mod classify;
pub mod config;
pub mod generators;
pub mod parse;
#[cfg(test)]
mod tests;
pub mod types;

// Re-export key types at the servers module level
pub use config::{ClientGenerator, Config, GeneratorConfig, PrefixParam, RoutePrefix, ServerGenerator};
pub use parse::{ApiFn, ApiModule, EventFn, Param};
pub use types::NamingConfig;

// Re-export server generator config for use in ServersConfig
pub use config::ServerGenerator as ServerGeneratorConfig;

use std::path::PathBuf;

use crate::CodegenError;
use crate::ir::{ApiOutput, ServersOutput};

/// Generate server transports.
///
/// When `api` is `Some`, future versions will use structured metadata.
/// When `None`, falls back to scanning source files (current behavior).
pub fn generate(
    _api: Option<&ApiOutput>,
    _scan_dirs: &[PathBuf],
    config: &crate::ServersConfig,
) -> Result<ServersOutput, CodegenError> {
    // Convert unified ServersConfig → internal Config
    let legacy_config = config::Config {
        api_dir: config.api_dir.clone(),
        state_type: config.state_type.clone(),
        service_import_path: config.service_import_path.clone(),
        types_import_path: config.types_import_path.clone(),
        state_import: config.state_import.clone(),
        naming: config.naming.clone(),
        generators: config
            .generators
            .iter()
            .map(|g| match g {
                ServerGeneratorConfig::HttpAxum { output } => {
                    config::GeneratorConfig::Server(config::ServerGenerator::HttpAxum { output: output.clone() })
                }
                ServerGeneratorConfig::TauriIpc { output } => {
                    config::GeneratorConfig::Server(config::ServerGenerator::TauriIpc { output: output.clone() })
                }
                ServerGeneratorConfig::Mcp { output } => {
                    config::GeneratorConfig::Server(config::ServerGenerator::Mcp { output: output.clone() })
                }
            })
            .collect(),
        rustfmt_edition: config.rustfmt_edition.clone(),
        sse_route_overrides: config.sse_route_overrides.clone(),
        ts_skip_commands: config.ts_skip_commands.clone(),
        route_prefix: config.route_prefix.clone(),
        store_type: config.store_type.clone(),
        store_import: config.store_import.clone(),
        schema_entities: Vec::new(),
    };

    // Run the transport generation pipeline
    let modules = generate_transport(&legacy_config).map_err(CodegenError::Server)?;

    // Build output metadata for downstream consumers
    // Phase 1: minimal metadata — enough for clients
    let _ = modules; // TODO: extract route/command metadata from generated output

    Ok(ServersOutput { http_routes: vec![], ipc_commands: vec![], mcp_tools: vec![] })
}

/// Run the transport generation pipeline (parse API modules + generate server/client code).
///
/// Parses API modules and generates server/client code for all configured transports.
/// Returns the parsed `ApiModule` list so callers can use it for test generation
/// or other downstream tasks.
pub fn generate_transport(config: &config::Config) -> Result<Vec<parse::ApiModule>, String> {
    if !config.api_dir.exists() {
        return Err(format!("API directory does not exist: {}", config.api_dir.display()));
    }

    let modules = parse::scan_api_dir(&config.api_dir, &config.state_type, config.store_type.as_deref());

    if modules.is_empty() {
        return Ok(modules);
    }

    for generator in &config.generators {
        match generator {
            config::GeneratorConfig::Server(config::ServerGenerator::HttpAxum { output }) => {
                generators::http::generate(output, &modules, config);
            }
            config::GeneratorConfig::Server(config::ServerGenerator::Mcp { output }) => {
                generators::mcp::generate(output, &modules, config);
            }
            config::GeneratorConfig::Server(config::ServerGenerator::TauriIpc { output }) => {
                generators::ipc::generate(output, &modules, config);
            }
            config::GeneratorConfig::Client(config::ClientGenerator::HttpTs { output, bindings_path }) => {
                generators::ts_client::generate(output, bindings_path, &modules, config);
            }
            config::GeneratorConfig::Client(config::ClientGenerator::HttpTauriIpcSplit { output, bindings_path }) => {
                generators::transport::generate(output, bindings_path, &modules, config);
            }
            config::GeneratorConfig::Client(config::ClientGenerator::AdminRegistry { output }) => {
                generators::admin::generate(output, &modules, config);
            }
        }
    }

    // Note: Rust server generators use write_and_format() internally,
    // so no separate rustfmt pass is needed.

    // Format generated TypeScript files
    // Note: prettier is still needed since TS generators use write_if_changed
    // without formatting. Prettier checks mtimes internally and is safe.
    let ts_files: Vec<&std::path::Path> = config
        .generators
        .iter()
        .filter_map(|g| match g {
            config::GeneratorConfig::Client(
                config::ClientGenerator::HttpTs { output, .. }
                | config::ClientGenerator::HttpTauriIpcSplit { output, .. }
                | config::ClientGenerator::AdminRegistry { output },
            ) => Some(output.as_path()),
            config::GeneratorConfig::Server(_) => None,
        })
        .collect();

    crate::prettier(&ts_files);

    // Emit cargo:rerun-if-changed
    crate::emit_rerun_directives(&config.api_dir);

    Ok(modules)
}
