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
pub use config::{
    ClientGenerator, Config, GeneratorConfig, PaginationConfig, PrefixParam, RoutePrefix, ServerGenerator,
};
pub use parse::{ApiFn, ApiModule, EventFn, Param};
pub use types::NamingConfig;

// Re-export server generator config for use in ServersConfig
pub use config::ServerGenerator as ServerGeneratorConfig;

use std::path::PathBuf;

use ontogen_core::ir::OpKind;

use crate::CodegenError;
use crate::ir::{ApiOutput, HttpRouteMeta, IpcCommandMeta, McpToolMeta, ParamMeta, ServersOutput};

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
            .chain(config.client_generators.iter().map(|g| config::GeneratorConfig::Client(g.clone())))
            .collect(),
        rustfmt_edition: config.rustfmt_edition.clone(),
        sse_route_overrides: config.sse_route_overrides.clone(),
        ts_skip_commands: config.ts_skip_commands.clone(),
        route_prefix: config.route_prefix.clone(),
        store_type: config.store_type.clone(),
        store_import: config.store_import.clone(),
        schema_entities: Vec::new(),
        pagination: config.pagination.clone(),
    };

    // Run the transport generation pipeline
    let modules = generate_transport(&legacy_config).map_err(CodegenError::Server)?;

    Ok(extract_server_metadata(&modules, &legacy_config))
}

/// Build `ServersOutput` from the same parsed modules the generators consumed.
///
/// HTTP routes mirror the path/method decisions made by the HTTP generator
/// (including project-scoping for store-based modules when `route_prefix`
/// is set). IPC commands and MCP tools are 1:1 with API functions —
/// `route_prefix` does not affect them.
fn extract_server_metadata(modules: &[parse::ApiModule], config: &config::Config) -> ServersOutput {
    let mut http_routes = Vec::new();
    let mut ipc_commands = Vec::new();
    let mut mcp_tools = Vec::new();

    for m in modules {
        let url_plural = config.naming.url_plural(&m.name);
        let is_store_module = m.functions.first().is_some_and(|f| f.first_param_is_store);

        // HTTP base path: store-based modules get scoped under route_prefix
        // when configured (mirroring http.rs:166-170 + generate_scoped_handlers).
        let http_base = match (&config.route_prefix, is_store_module) {
            (Some(prefix), true) => format!("/api/{}", prefix.segments),
            _ => "/api".to_string(),
        };

        for f in &m.functions {
            let op = classify::classify_op(f);
            let handler_name = generators::ipc::command_name(&m.name, f, config);
            let params: Vec<ParamMeta> =
                f.params.iter().map(|p| ParamMeta { name: p.name.clone(), param_type: p.ty.clone() }).collect();

            if let Some((method, path)) = http_route_for(&op, &http_base, &url_plural, &m.name, f, config) {
                http_routes.push(HttpRouteMeta {
                    method,
                    path,
                    handler_name: handler_name.clone(),
                    module_name: m.name.clone(),
                });
            }

            ipc_commands.push(IpcCommandMeta {
                command_name: handler_name.clone(),
                params: params.clone(),
                return_type: f.return_type.clone(),
            });

            mcp_tools.push(McpToolMeta { tool_name: handler_name, description: f.doc.clone(), params });
        }

        // SSE event streams — HTTP-only. When route_prefix is set, both
        // unscoped and prefix-scoped variants are emitted (http.rs:463-500
        // and the scoped handler block).
        for ev in &m.events {
            let ev_name = crate::servers::types::event_name(&ev.name);
            let unscoped =
                config.sse_route_overrides.get(&ev.name).cloned().unwrap_or_else(|| format!("/api/events/{}", ev_name));
            http_routes.push(HttpRouteMeta {
                method: "GET".to_string(),
                path: unscoped.clone(),
                handler_name: format!("{}_sse", ev.name),
                module_name: m.name.clone(),
            });

            if let Some(prefix) = &config.route_prefix {
                let scoped_path = match config.sse_route_overrides.get(&ev.name) {
                    Some(override_path) => match override_path.strip_prefix("/api/") {
                        Some(rest) => format!("/api/{}/{}", prefix.segments, rest),
                        None => format!("/api/{}{}", prefix.segments, override_path),
                    },
                    None => format!("/api/{}/events/{}", prefix.segments, ev_name),
                };
                http_routes.push(HttpRouteMeta {
                    method: "GET".to_string(),
                    path: scoped_path,
                    handler_name: format!("{}_sse_scoped", ev.name),
                    module_name: m.name.clone(),
                });
            }
        }
    }

    ServersOutput { http_routes, ipc_commands, mcp_tools }
}

/// Compute the HTTP method + path for a classified function.
///
/// Returns `None` for `EventStream` (events are emitted separately from `m.events`).
fn http_route_for(
    op: &OpKind,
    base: &str,
    plural: &str,
    module: &str,
    f: &parse::ApiFn,
    config: &config::Config,
) -> Option<(String, String)> {
    let route = match op {
        OpKind::List => ("GET", format!("{base}/{plural}")),
        OpKind::Create => ("POST", format!("{base}/{plural}")),
        OpKind::GetById => ("GET", format!("{base}/{plural}/:id")),
        OpKind::Update => ("PUT", format!("{base}/{plural}/:id")),
        OpKind::Delete => ("DELETE", format!("{base}/{plural}/:id")),
        OpKind::JunctionList { child_segment } => ("GET", format!("{base}/{plural}/:parent_id/{child_segment}")),
        OpKind::JunctionAdd { child_segment } => ("POST", format!("{base}/{plural}/:parent_id/{child_segment}")),
        OpKind::JunctionRemove { child_segment } => {
            ("DELETE", format!("{base}/{plural}/:parent_id/{child_segment}/:child_id"))
        }
        OpKind::CustomGet | OpKind::CustomPost => {
            let is_get = classify::is_read_operation(&f.name);
            let action = config.naming.derive_action(module, &f.name);
            let mut path = format!("{base}/{plural}");
            if !action.is_empty() {
                path.push('/');
                path.push_str(&action);
            }
            // GET handlers extract path params from non-Option non-Input params.
            // POST handlers put all such params into the JSON body — no path params.
            if is_get {
                for p in &f.params {
                    if !p.ty.starts_with("Option<") && !p.ty.contains("Input") {
                        path.push_str(&format!("/:{}", p.name));
                    }
                }
            }
            (if is_get { "GET" } else { "POST" }, path)
        }
        OpKind::EventStream => return None,
    };
    Some((route.0.to_string(), route.1))
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
    // and TS generators now use write_and_format_ts(), so no separate
    // formatting pass is needed. All formatting happens in memory before
    // write_if_changed, preventing unnecessary mtime changes.

    Ok(modules)
}
