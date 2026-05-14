//! Server transport generators - HTTP (Axum), IPC (Tauri), MCP.
//!
//! Also includes client generators (TypeScript, admin registry) which will
//! move to the `clients` module in a later phase.

// All submodules are crate-internal; their public types are re-exported below
// where intended for downstream consumption. External code reaches them via
// `ontogen::servers::Foo` (or, more commonly, via the top-level re-exports in
// `lib.rs`), not via the longer `ontogen::servers::config::Foo` path.
pub(crate) mod classify;
pub(crate) mod config;
pub(crate) mod generators;
pub(crate) mod parse;
#[cfg(test)]
mod tests;
pub(crate) mod types;

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
        schema_entities: config.schema_entities.clone(),
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
/// is set). IPC commands and MCP tools are 1:1 with API functions -
/// `route_prefix` does not affect them.
fn extract_server_metadata(modules: &[parse::ApiModule], config: &config::Config) -> ServersOutput {
    let mut http_routes = Vec::new();
    let mut ipc_commands = Vec::new();
    let mut mcp_tools = Vec::new();

    for m in modules {
        let url_plural = config.naming.url_for_module(m);
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

        // SSE event streams - HTTP-only. When route_prefix is set, both
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
            let is_get = classify::is_read_op(op);
            let action = config.naming.derive_action(module, &f.name);
            let mut path = format!("{base}/{plural}");
            if !action.is_empty() {
                path.push('/');
                path.push_str(&action);
            }
            // GET handlers extract path params from non-Option non-Input params.
            // POST handlers put all such params into the JSON body - no path params.
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

    let scanned = parse::scan_api_dir(&config.api_dir, &config.state_type, config.store_type.as_deref());

    for record in &scanned.skips {
        println!("cargo:warning={record}");
    }

    let mut modules = scanned.modules;
    parse::apply_singleton_overlay(&mut modules, &config.naming);
    parse::apply_command_overrides(&mut modules, &config.naming);
    if modules.is_empty() {
        return Ok(modules);
    }

    // OF-014 spike (option 1 half): emit schema-known TS aliases (entities +
    // generated DTOs) to every distinct `bindings_path` declared by client
    // generators *before* the client generators run. They then partition
    // referenced types into schema-known (now satisfied) vs long-tail.
    let mut written_bindings: std::collections::HashSet<std::path::PathBuf> = Default::default();
    for generator in &config.generators {
        let bp = match generator {
            config::GeneratorConfig::Client(config::ClientGenerator::HttpTs { bindings_path, .. })
            | config::GeneratorConfig::Client(config::ClientGenerator::HttpTauriIpcSplit { bindings_path, .. }) => {
                Some(bindings_path.clone())
            }
            _ => None,
        };
        if let Some(path) = bp
            && written_bindings.insert(path.clone())
        {
            let body = generators::ts_bindings::emit(&config.schema_entities);
            crate::write_and_format_ts(&path, body).expect("Failed to write schema-known bindings");
        }
    }

    // OF-014 spike (option 3 half): for the long tail, write a side-car
    // binary into the user's crate, build+run it via cargo with a distinct
    // CARGO_TARGET_DIR (avoids the parent cargo's target-dir lock — see
    // rust-lang/cargo#8938), and append its stdout to bindings.ts. After
    // this runs, the existing client generators below see a fully-populated
    // bindings.ts and emit zero FallbackRecords.
    //
    // Env guard: the inner cargo invocation re-runs the user's build.rs,
    // which would otherwise recurse into this block forever. The guard
    // breaks the loop; the inner build still runs schema-known emission
    // (idempotent) but skips re-invoking the side-car.
    let in_sidecar_inner = std::env::var("ONTOGEN_TS_SIDECAR_INNER").is_ok();
    let long_tail = generators::ts_bindings::long_tail(&modules, config, &config.schema_entities);
    if !in_sidecar_inner && !long_tail.is_empty() && !written_bindings.is_empty() {
        let manifest_dir = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR")
                .map_err(|_| "CARGO_MANIFEST_DIR not set; ts_export side-car only runs from a build script")?,
        );
        let lib_crate_name = sidecar_lib_crate_name(&manifest_dir)?;
        let types_module_path = sidecar_types_module_path(&config.types_import_path);
        let source = generators::ts_sidecar::generate_sidecar_source(&lib_crate_name, &types_module_path, &long_tail);
        generators::ts_sidecar::write_sidecar_source(&manifest_dir, &source);
        let target_dir = std::path::PathBuf::from(
            std::env::var("OUT_DIR").map_err(|_| "OUT_DIR not set; ts_export side-car needs build-script env")?,
        )
        .join("ontogen-ts-sidecar-target");
        let ts = generators::ts_sidecar::run_sidecar(&manifest_dir, &target_dir)?;
        for path in &written_bindings {
            generators::ts_sidecar::append_to_bindings(path, &ts)?;
        }
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
                let fallbacks = generators::ts_client::generate(output, bindings_path, &modules, config);
                for record in &fallbacks {
                    println!("cargo:warning={record}");
                }
            }
            config::GeneratorConfig::Client(config::ClientGenerator::HttpTauriIpcSplit { output, bindings_path }) => {
                let fallbacks = generators::transport::generate(output, bindings_path, &modules, config);
                for record in &fallbacks {
                    println!("cargo:warning={record}");
                }
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

/// Read the user crate's Cargo.toml and return the lib crate name (the form
/// used in `use foo::...` imports). Honours an explicit `[lib] name` if set;
/// otherwise normalizes the package name (hyphens → underscores). Spike-grade
/// regex-free parser — productionization should use `cargo metadata`.
fn sidecar_lib_crate_name(manifest_dir: &std::path::Path) -> Result<String, String> {
    let manifest_path = manifest_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("failed to read {} for ts_export side-car: {e}", manifest_path.display()))?;
    let mut in_section = "";
    let mut package_name: Option<String> = None;
    let mut lib_name: Option<String> = None;
    for raw in content.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = match rest {
                "package" => "package",
                "lib" => "lib",
                _ => "",
            };
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let k = key.trim();
            let v = val.trim().trim_matches('"');
            match (in_section, k) {
                ("package", "name") => package_name = Some(v.to_string()),
                ("lib", "name") => lib_name = Some(v.to_string()),
                _ => {}
            }
        }
    }
    let name = lib_name.or_else(|| package_name.map(|n| n.replace('-', "_")));
    name.ok_or_else(|| format!("could not determine lib crate name from {}", manifest_path.display()))
}

/// Convert a Rust import path of the form `crate::foo::bar` into the form
/// usable from a sibling binary: `foo::bar`. The lib crate name will be
/// prepended at side-car generation time.
fn sidecar_types_module_path(types_import_path: &str) -> String {
    types_import_path.strip_prefix("crate::").unwrap_or(types_import_path).to_string()
}
