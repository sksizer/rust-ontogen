//! Server transport generators - HTTP (Axum), IPC (Tauri), MCP.

// All submodules are crate-internal; their public types are re-exported below
// where intended for downstream consumption. External code reaches them via
// `ontogen::servers::Foo` (or, more commonly, via the top-level re-exports in
// `lib.rs`), not via the longer `ontogen::servers::config::Foo` path.
pub(crate) mod classify;
pub(crate) mod config;
pub(crate) mod error_map;
pub(crate) mod generators;
pub(crate) mod parse;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod types;

// Re-export key types at the servers module level
pub use config::{ApiSurface, DEFAULT_STORE_ACCESSOR, PaginationConfig, PrefixParam, RoutePrefix, ServerGenerator};
pub use parse::{ApiFn, ApiModule, EventFn, Param};
pub use types::NamingConfig;

// Re-export server generator config for use in ServersConfig
pub use config::ServerGenerator as ServerGeneratorConfig;

use std::path::PathBuf;

use crate::CodegenError;
use crate::ir::{ApiOutput, HttpRouteMeta, IpcCommandMeta, McpToolMeta, ParamMeta, ServersOutput};
use crate::model::EntityDef;
use crate::resource::ResourceModel;

/// Generate server transports (Axum / Tauri IPC / MCP).
///
/// When `api` is `Some`, future versions will use structured metadata.
/// When `None`, falls back to scanning source files (current behavior).
///
/// Client-side TypeScript + admin-registry generation is the sibling
/// [`crate::gen_clients`] entry point; this function no longer touches
/// the TS surface.
pub fn generate(
    entities: &[EntityDef],
    _api: Option<&ApiOutput>,
    _scan_dirs: &[PathBuf],
    config: &crate::ServersConfig,
) -> Result<ServersOutput, CodegenError> {
    let resources = ResourceModel::build(entities, &config.naming).map_err(CodegenError::Server)?;
    let error_map = match &config.error_source_dir {
        Some(dir) => error_map::scan(dir).map_err(CodegenError::Server)?,
        None => None,
    };

    // Convert unified ServersConfig → internal Config
    let legacy_config = config::Config {
        api_dir: config.api_dir.clone(),
        state_type: config.state_type.clone(),
        service_import_path: config.service_import_path.clone(),
        types_import_path: config.types_import_path.clone(),
        state_import: config.state_import.clone(),
        naming: config.naming.clone(),
        generators: config.generators.clone(),
        sse_route_overrides: config.sse_route_overrides.clone(),
        route_prefix: config.route_prefix.clone(),
        store_type: config.store_type.clone(),
        store_import: config.store_import.clone(),
        pagination: config.pagination.clone(),
        extra_surfaces: config.extra_surfaces.clone(),
        resources,
        error_map,
    };

    // Run the transport generation pipeline
    let modules = generate_transport(&legacy_config).map_err(CodegenError::Server)?;

    if let (Some(dir), None) = (&config.error_source_dir, &legacy_config.error_map) {
        let affected: Vec<String> = modules
            .iter()
            .flat_map(|m| {
                let functions = m
                    .functions
                    .iter()
                    .filter(|f| generators::http::returns_app_error(f, &legacy_config))
                    .map(|f| &f.name);
                let events = m
                    .events
                    .iter()
                    .filter(|ev| generators::http::event_returns_app_error(ev, &legacy_config))
                    .map(|ev| &ev.name);
                functions.chain(events).map(move |name| format!("{}::{name}", m.name))
            })
            .collect();
        if let Some(warning) = error_map::missing_enum_warning(dir, &affected) {
            println!("{warning}");
        }
    }

    Ok(extract_server_metadata(&modules, &legacy_config))
}

/// Build `ServersOutput` from the same parsed modules the generators consumed.
///
/// HTTP routes mirror the path/method decisions made by the HTTP generator
/// (including project-scoping for store-based modules when `route_prefix`
/// is set), one row per method of each route it registers. A junction op of
/// a resource module has no route of its own: its module's relationship
/// routes are reported instead, with their `{rel}` capture. IPC commands
/// and MCP tools are 1:1 with API functions - `route_prefix` does not
/// affect them.
fn extract_server_metadata(modules: &[parse::ApiModule], config: &config::Config) -> ServersOutput {
    let mut http_routes = Vec::new();
    let mut ipc_commands = Vec::new();
    let mut mcp_tools = Vec::new();

    for m in modules {
        for f in &m.functions {
            let handler_name = generators::ipc::command_name(&m.name, f, config);
            let params: Vec<ParamMeta> =
                f.params.iter().map(|p| ParamMeta { name: p.name.clone(), param_type: p.ty.clone() }).collect();

            if !generators::http::is_relationship_op(m, f, config) {
                let (method, path) = generators::http::route_of(m, f, config);
                http_routes.push(HttpRouteMeta {
                    method: method.to_ascii_uppercase(),
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

        for (method, path, handler_name) in generators::http::relationship_route_table(m, config) {
            http_routes.push(HttpRouteMeta {
                method: method.to_ascii_uppercase(),
                path,
                handler_name,
                module_name: m.name.clone(),
            });
        }

        // Event ops: an SSE route (plus a prefix-scoped one when route_prefix
        // is set) and an IPC subscribe/unsubscribe pair. MCP skips them.
        for ev in &m.events {
            http_routes.push(HttpRouteMeta {
                method: "GET".to_string(),
                path: generators::http::axum_path(&ev.sse_route(&config.sse_route_overrides)),
                handler_name: format!("{}_sse", ev.name),
                module_name: m.name.clone(),
            });

            if let Some(prefix) = &config.route_prefix {
                http_routes.push(HttpRouteMeta {
                    method: "GET".to_string(),
                    path: generators::http::axum_path(
                        &ev.sse_route_scoped(&config.sse_route_overrides, &prefix.segments),
                    ),
                    handler_name: format!("{}_sse_scoped", ev.name),
                    module_name: m.name.clone(),
                });
            }

            let mut subscribe_params: Vec<ParamMeta> =
                ev.params.iter().map(|p| ParamMeta { name: p.name.clone(), param_type: p.ty.clone() }).collect();
            subscribe_params.push(ParamMeta {
                name: "channel".to_string(),
                param_type: format!("tauri::ipc::Channel<ontogen_core::events::EventFrame<{}>>", ev.item_type),
            });
            ipc_commands.push(IpcCommandMeta {
                command_name: format!("{}_subscribe", ev.name),
                params: subscribe_params,
                return_type: "u64".to_string(),
            });
            ipc_commands.push(IpcCommandMeta {
                command_name: format!("{}_unsubscribe", ev.name),
                params: vec![ParamMeta { name: "id".to_string(), param_type: "u64".to_string() }],
                return_type: "bool".to_string(),
            });
        }
    }

    ServersOutput { http_routes, ipc_commands, mcp_tools }
}

/// Run the server-side transport generation pipeline (parse API modules + generate server code).
///
/// Parses API modules and generates server code for each configured
/// [`ServerGenerator`]. Returns the parsed `ApiModule` list so callers can
/// use it for test generation or other downstream tasks.
pub(crate) fn generate_transport(config: &config::Config) -> Result<Vec<parse::ApiModule>, String> {
    // Project-scoped handlers open the store through the one
    // `route_prefix.state_accessor`, which yields the primary surface's store
    // type; an extra surface's store-scoped fns expect their own store, so
    // the combination cannot be generated correctly.
    if let Some(prefix) = &config.route_prefix
        && let Some(surface) = config.extra_surfaces.iter().find(|s| s.store_type.is_some())
    {
        return Err(format!(
            "ontogen: `route_prefix` cannot be combined with an extra API surface that has a `store_type` (surface \
             `{}`, store type `{}`); scoped handlers always open the store via `{}`, which yields the primary \
             surface's store",
            surface.api_dir.display(),
            surface.store_type.as_deref().unwrap_or_default(),
            prefix.state_accessor,
        ));
    }

    let surfaces = config.surfaces();
    let scanned = parse::scan_surfaces(&surfaces, &config.state_type)?;

    for record in &scanned.skips {
        println!("cargo:warning={record}");
    }

    let mut modules = scanned.modules;
    parse::qualify_shared_types(&mut modules, &surfaces);
    parse::apply_singleton_overlay(&mut modules, &config.naming);
    parse::apply_command_overrides(&mut modules, &config.naming);
    parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces)?;
    if modules.is_empty() {
        return Ok(modules);
    }
    if config.generators.iter().any(|g| matches!(g, config::ServerGenerator::HttpAxum { .. })) {
        classify::check_http_ops(&modules, &config.resources, config.route_prefix.as_ref())?;
        generators::http::check_resource_ops(&modules, config)?;
        for warning in generators::http::unplaced_app_error_warnings(&modules, config) {
            println!("{warning}");
        }
    }

    if config.generators.iter().any(|g| matches!(g, config::ServerGenerator::TauriIpc { .. })) {
        generators::ipc::check_wire_keys(&modules, config)?;
    }
    if config.generators.iter().any(|g| matches!(g, config::ServerGenerator::Mcp { .. })) {
        generators::mcp::check_scope_key(&modules, config)?;
    }

    for generator in &config.generators {
        match generator {
            config::ServerGenerator::HttpAxum { output } => {
                generators::http::generate(output, &modules, config);
            }
            config::ServerGenerator::Mcp { output } => {
                generators::mcp::generate(output, &modules, config);
            }
            config::ServerGenerator::TauriIpc { output } => {
                generators::ipc::generate(output, &modules, config);
            }
        }
    }

    // Note: Rust server generators use write_and_format() internally,
    // so no separate formatting pass is needed. All formatting happens in
    // memory before write_if_changed, preventing unnecessary mtime changes.

    Ok(modules)
}
