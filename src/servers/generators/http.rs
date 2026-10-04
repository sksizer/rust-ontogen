#![allow(clippy::too_many_lines, clippy::format_push_string)]

//! Generate Axum HTTP route handlers from API modules.
//!
//! Every route speaks JSON:API through the `ontogen-jsonapi` runtime crate:
//! a resource module's CRUD ops are served as its resource (wire contract
//! §7, §8), and every other op as a custom op with a meta-only document
//! (§10), at the routes §10.4 gives CRUD-named ops with no resource and
//! junction ops. Event streams send resource objects or `meta.result`
//! frames (§12). The one exception is a list that takes a filter, which
//! keeps its flat query and flat success shape; its errors are `errors[]`
//! documents (§13) like every other route's.

use std::fs;
use std::path::Path;

use ontogen_core::ir::OpKind;

use crate::persistence::dto::{create_field_required, field_to_create_type};
use crate::resource::{Arity, Resource, list_takes_filter, member_name};
use crate::servers::classify::classify_op;
use crate::servers::config::{Config, RoutePrefix};
use crate::servers::error_map::VariantShape;
use crate::servers::generators::surface_use_stmts_where;
use crate::servers::parse::{ApiFn, ApiModule, EventFn, Param, is_page_param, is_resume_param};
use crate::servers::types::{
    capitalize, event_name, extract_input_type, forward_arg_expr, inner_type, param_to_owned_type, to_pascal_case,
};

/// Convert colon-style path params (`:name`) to axum 0.8's `{name}` form.
///
/// Config-facing strings (`route_prefix.segments`, `sse_route_overrides`)
/// keep the `:name` convention so existing consumer build scripts don't
/// break; every axum route string is normalized through here at emission.
pub(in crate::servers) fn axum_path(path: &str) -> String {
    path.split('/')
        .map(|segment| match segment.strip_prefix(':') {
            Some(name) => format!("{{{name}}}"),
            None => segment.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The resource `f` is served as ([`ResourceModel::serving`]).
///
/// [`ResourceModel::serving`]: crate::resource::ResourceModel::serving
pub(in crate::servers) fn served_resource<'a>(m: &ApiModule, f: &ApiFn, config: &'a Config) -> Option<&'a Resource> {
    config.resources.serving(&m.name, f)
}

/// Check that every op [`served_resource`] picks can be served as its
/// resource: it takes the state or a store, the arguments its operation
/// passes, and returns the entity. A create or update also needs a
/// `get_by_id` in every module it links to, for the linked-resource checks
/// (§13.2 step 8).
pub(in crate::servers) fn check_resource_ops(modules: &[ApiModule], config: &Config) -> Result<(), String> {
    for m in modules {
        for f in &m.functions {
            let Some(resource) = served_resource(m, f, config) else { continue };
            let op = classify_op(f);
            let entity = resource.entity.name.as_str();
            let refuse = |why: String| -> Result<(), String> {
                Err(format!(
                    "ontogen: `{}::{}` is served as the JSON:API resource `{}`, so it {why}",
                    m.name, f.name, resource.resource_type
                ))
            };
            if f.is_stateless {
                return refuse("must take the state or a store".to_string());
            }
            let (arity, shape) = match op {
                OpKind::GetById | OpKind::Delete => (1, "`id: &str`"),
                OpKind::Create => (1, "its input"),
                OpKind::Update => (2, "`id: &str` and its input"),
                _ => (f.params.len(), ""),
            };
            if f.params.len() != arity {
                return refuse(format!("must take {shape} after the state or store"));
            }
            let is_entity = |ty: &str| ty.rsplit("::").next() == Some(entity);
            let returns_entity = match op {
                OpKind::List => f.return_type.starts_with("Vec<") && is_entity(&inner_type(&f.return_type)),
                OpKind::Delete => true,
                _ => is_entity(&f.return_type),
            };
            if !returns_entity {
                let want = if op == OpKind::List { format!("Vec<{entity}>") } else { entity.to_string() };
                return refuse(format!("must return `{want}`"));
            }
            if matches!(op, OpKind::Create | OpKind::Update) {
                for rel in &resource.relationships {
                    if linked_lookup(modules, &rel.target_module).is_none() {
                        return refuse(format!(
                            "checks every `{}` it links with `{}::get_by_id`, which no module provides",
                            rel.target_type, rel.target_module
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// The module and `get_by_id` a linked id of `target_module` is looked up
/// with.
fn linked_lookup<'a>(modules: &'a [ApiModule], target_module: &str) -> Option<(&'a ApiModule, &'a ApiFn)> {
    let m = modules.iter().find(|m| m.name == target_module)?;
    let f = m.functions.iter().find(|f| f.name == "get_by_id" && !f.is_stateless)?;
    Some((m, f))
}

/// Routes by path, in first-registration order, each with its methods.
///
/// Every route gets a method fallback answering `405` with `Allow` (§13.5).
/// Two registrations of one path merge here, since Axum cannot merge two
/// method routers that both carry a fallback.
#[derive(Default)]
struct Routes(Vec<(String, Vec<(&'static str, String)>)>);

impl Routes {
    fn add(&mut self, path: &str, method: &'static str, handler: &str) {
        let i = match self.0.iter().position(|(p, _)| p == path) {
            Some(i) => i,
            None => {
                self.0.push((path.to_string(), Vec::new()));
                self.0.len() - 1
            }
        };
        self.0[i].1.push((method, handler.to_string()));
    }

    /// The routing fns the routes call, in name order: each route's first
    /// method. The rest chain as `MethodRouter` methods.
    fn methods(&self) -> Vec<&'static str> {
        let mut methods: Vec<&'static str> = self.0.iter().filter_map(|(_, m)| m.first().map(|(m, _)| *m)).collect();
        methods.sort_unstable();
        methods.dedup();
        methods
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (path, methods) in &self.0 {
            let router: Vec<String> = methods.iter().map(|(m, h)| format!("{m}({h})")).collect();
            let allowed: Vec<String> =
                methods.iter().map(|(m, _)| format!("Method::{}", m.to_ascii_uppercase())).collect();
            out.push_str(&format!(
                "        .route(\"{path}\", {}.fallback(allow([{}])))\n",
                router.join("."),
                allowed.join(", ")
            ));
        }
        out
    }
}

/// How a handler reaches its fn's first argument.
///
/// Every binding a generated handler makes of its own is `ontogen_`-prefixed
/// (`ontogen_state`, `ontogen_store`, `ontogen_query`, …), and a fn's
/// arguments are bound under their own names: an argument may be named
/// anything, `state` and `store` included, without shadowing the handler's.
struct Access {
    /// Lines opening the store, if the fn takes one.
    open: String,
    /// The fn's first argument.
    arg: &'static str,
}

/// The unscoped handler's access: the fn's surface accessor for a
/// store-scoped fn, the state otherwise.
fn unscoped_access(f: &ApiFn) -> Access {
    if f.first_param_is_store {
        Access {
            open: format!(
                "    let ontogen_store = ontogen_state.{}().await.map_err(internal_error)?;\n",
                f.store_accessor
            ),
            arg: "&ontogen_store",
        }
    } else {
        Access { open: String::new(), arg: "&ontogen_state" }
    }
}

/// The ident a scoped handler binds the route prefix's value to. The
/// prefix param's configured name is only a route segment: as an ident it
/// could be any name a handler already binds (`query`, `state`, `id`,
/// `path_params`, a function's own parameter), so no handler uses it.
const SCOPE: &str = "ontogen_scope";

/// A scoped handler's access through the prefix accessor. Its failure is a
/// `500`: the accessor's error does not say whether the scope is missing
/// (§11.1).
fn scoped_access(prefix: &RoutePrefix) -> Access {
    Access {
        open: format!(
            "    let ontogen_store = ontogen_state.{}(&{SCOPE}).map_err(internal_error)?;\n",
            prefix.state_accessor
        ),
        arg: "&ontogen_store",
    }
}

/// `.map_err(…)` for a call returning `f`'s error: `app_error` for the
/// consumer's `AppError`, `internal_error` for anything else (§13.4).
fn err_map(f: &ApiFn, config: &Config) -> &'static str {
    if returns_app_error(f, config) { ".map_err(app_error)" } else { ".map_err(internal_error)" }
}

/// True when `f` fails with the `AppError` that `app_error` takes: the one
/// in the primary surface's types module (§13.4). Anything else maps
/// through `internal_error`, which takes any `Display` error, so a type
/// this cannot place is a `500` rather than a build failure.
pub(crate) fn returns_app_error(f: &ApiFn, config: &Config) -> bool {
    is_app_error(f.error_type.as_deref(), f.surface, config)
}

/// [`returns_app_error`] for an event fn's subscribe call.
pub(crate) fn event_returns_app_error(ev: &EventFn, config: &Config) -> bool {
    is_app_error(ev.error_type.as_deref(), ev.surface, config)
}

fn is_app_error(error: Option<&str>, surface: usize, config: &Config) -> bool {
    error_path(error, surface, config).is_some_and(|path| path == app_error_path(config))
}

/// An error type as a path the generated crate can compare, read in the
/// surface of the fn that returns it.
///
/// The parser has resolved the type through `f`'s file's `use` items.
/// Then a `crate::` path is in the crate the surface's service path names
/// (`fitness::api` puts `crate::schema::AppError` at
/// `fitness::schema::AppError`), and a path the surface's own
/// `{types_import_path}::AppError` ends with (a bare `AppError` no `use`
/// binds, `schema::AppError`) is that surface's `AppError`.
///
/// Not resolved, so taken as written: a type alias (`type E = AppError;`
/// does not match), a re-export (`crate::schema::error::AppError` is not
/// proven to be `crate::schema::AppError`), and `self::`/`super::` paths. A bare
/// `AppError` a glob (`use fitness::schema::*;`) brings in is read as the
/// surface's own, which is wrong only when the glob names another surface's
/// types module.
fn error_path(error: Option<&str>, surface: usize, config: &Config) -> Option<String> {
    let error = error?;
    let (service, types) = match surface {
        0 => (config.service_import_path.as_str(), config.types_import_path.as_str()),
        i => {
            let surface = &config.extra_surfaces[i - 1];
            (surface.service_import_path.as_str(), surface.types_import_path.as_str())
        }
    };
    let own = format!("{types}::AppError");
    let crate_root = service.split("::").next().unwrap_or("crate");
    let path = match error.strip_prefix("crate::") {
        Some(rest) if crate_root != "crate" => format!("{crate_root}::{rest}"),
        _ => error.to_string(),
    };
    if own == path || own.ends_with(&format!("::{path}")) { Some(own) } else { Some(path) }
}

/// A `cargo:warning` for each fn whose error type is named `AppError` but
/// is no surface's `{types_import_path}::AppError`, such as one under the
/// primary types module by another path (`crate::schema::error::AppError`).
/// It may be the consumer's `AppError` re-exported, but that cannot be
/// proven here, so its errors answer `500 internal_error`.
pub(crate) fn unplaced_app_error_warnings(modules: &[ApiModule], config: &Config) -> Vec<String> {
    let known: Vec<String> = std::iter::once(app_error_path(config))
        .chain(config.extra_surfaces.iter().map(|s| format!("{}::AppError", s.types_import_path)))
        .collect();
    let fns = modules.iter().flat_map(|m| {
        let functions = m.functions.iter().map(move |f| (m, &f.name, f.error_type.as_deref(), f.surface));
        functions.chain(m.events.iter().map(move |ev| (m, &ev.name, ev.error_type.as_deref(), ev.surface)))
    });
    fns.filter_map(|(m, name, error, surface)| {
        let path = error_path(error, surface, config)?;
        (path.rsplit("::").next() == Some("AppError") && !known.contains(&path)).then(|| {
            format!(
                "cargo:warning=ontogen: `{}::{name}` returns `{path}`, which is not proven to be `{}`; its errors \
                 answer 500 internal_error. If it is that type, name it by that path.",
                m.name,
                app_error_path(config),
            )
        })
    })
    .collect()
}

fn await_str(is_async: bool) -> &'static str {
    if is_async { ".await" } else { "" }
}

/// The path of the consumer's `AppError` as generated code names it.
fn app_error_path(config: &Config) -> String {
    format!("{}::AppError", config.types_import_path)
}

/// Generate HTTP route handlers and write to the output file.
pub fn generate(output: &Path, modules: &[ApiModule], config: &Config) {
    // Handlers and the helpers they share, emitted first so that the
    // on-demand helpers and the `use` items are exactly those they use.
    let mut out = String::new();

    let has_events = modules.iter().any(|m| !m.events.is_empty());
    if has_events {
        out.push_str(SSE_HELPERS);
    }

    let frames = event_resources(modules, config);
    if modules.iter().any(|m| m.functions.iter().any(|f| served_resource(m, f, config).is_some())) {
        out.push_str(RESOURCE_HELPERS);
    }
    let mut with_object: Vec<&str> = Vec::new();
    for m in modules {
        if emit_resource_helpers(&mut out, m, modules, config, &frames) {
            with_object.push(&m.name);
        }
    }
    for (resource, item_type) in &frames {
        if !with_object.contains(&resource.module.as_str()) {
            out.push_str(&format!("// ── `{}` ──\n\n", resource.resource_type));
            emit_resource_object(&mut out, resource, item_type);
        }
        emit_frame_data(&mut out, resource, item_type);
    }

    // Unscoped junction routes go last, in path order. `BTreeMap` keeps the
    // bytes stable across runs, which `write_if_changed` relies on (a
    // `tauri dev` watcher would otherwise rebuild forever).
    let mut routes = Routes::default();
    let mut junction_routes: std::collections::BTreeMap<String, Vec<(&'static str, String)>> =
        std::collections::BTreeMap::new();

    for m in modules {
        // With a `route_prefix`, a store-scoped fn is served under the prefix
        // only (below); a state-scoped or stateless fn keeps its unscoped
        // route. Decided per fn: a merged module may mix both kinds.
        let functions: Vec<&ApiFn> = m.functions.iter().filter(|f| !is_scoped(f, config)).collect();
        if functions.is_empty() {
            continue;
        }
        out.push_str(&format!("// ── {} Handlers ──\n\n", capitalize(&m.name)));
        for f in functions {
            let (method, path, handler_name) = emit_fn(&mut out, m, f, config, None);
            if is_junction(f) {
                junction_routes.entry(path).or_default().push((method, handler_name));
            } else {
                routes.add(&path, method, &handler_name);
            }
        }
    }
    for (path, methods) in &junction_routes {
        for (method, handler) in methods {
            routes.add(path, method, handler);
        }
    }

    for m in modules {
        for ev in &m.events {
            generate_sse_handler(&mut out, &mut routes, m, ev, config, None);
        }
    }

    if let Some(prefix) = &config.route_prefix {
        out.push_str("\n// ── Project-Scoped Handlers ──\n\n");
        for m in modules {
            for f in m.functions.iter().filter(|f| is_scoped(f, config)) {
                let (method, path, handler_name) = emit_fn(&mut out, m, f, config, Some(prefix));
                routes.add(&path, method, &handler_name);
            }
        }
        for m in modules {
            for ev in &m.events {
                generate_sse_handler(&mut out, &mut routes, m, ev, config, Some(prefix));
            }
        }
    }

    let state_type = &config.state_type;
    out.push_str(&format!(
        "/// Generated routes. Call this from your main router.\npub fn entity_routes() -> Router<Arc<{state_type}>> {{\n    Router::new()\n"
    ));
    out.push_str(&routes.render());
    out.push_str("}\n");

    let mut helpers = String::new();
    emit_error_helpers(&mut helpers, config);
    helpers.push_str(ROUTE_HELPERS);
    let handlers = without_comments(&out);
    for (name, definition) in ON_DEMAND_HELPERS {
        if uses_ident(&handlers, name) {
            helpers.push_str(definition);
        }
    }
    let body = helpers + &out;
    let code = without_comments(&body);

    let mut file = String::from(
        "\
#![allow(dead_code, unused_imports, clippy::pedantic, clippy::needless_borrow)]
//! Auto-generated HTTP route handlers. DO NOT EDIT.
//!
//! Generated by ontogen from API source files.

use std::sync::Arc;

",
    );
    file.push_str(&runtime_imports(&code, &routes));
    file.push('\n');
    let mut use_stmts = surface_use_stmts_where(modules, config, &|name| uses_ident(&code, name));
    let named = |path: &str| uses_ident(&code, path.rsplit("::").next().unwrap_or(path));
    use_stmts.extend(
        std::iter::once(&config.state_import)
            .chain(&config.store_import)
            .filter(|path| named(path))
            .map(|path| format!("use {path};\n")),
    );
    use_stmts.sort();
    for stmt in &use_stmts {
        file.push_str(stmt);
    }
    if has_events {
        file.push_str(
            "\
use std::convert::Infallible;
use axum::response::sse::{Event, KeepAlive, Sse};
use ontogen_core::events::EventFrame;
",
        );
    }
    file.push('\n');
    file.push_str(&body);

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("Failed to create output directory");
    }
    crate::write_and_format(output, file).expect("Failed to write HTTP generated file");
}

/// Emit `f`'s handler, served under `scope` when given, returning its method,
/// its route and its handler name.
fn emit_fn(
    out: &mut String,
    m: &ApiModule,
    f: &ApiFn,
    config: &Config,
    scope: Option<&RoutePrefix>,
) -> (&'static str, String, String) {
    let handler_name = handler_name(m, f, config, scope.is_some());
    let access = match scope {
        _ if f.is_stateless => None,
        Some(prefix) => Some(scoped_access(prefix)),
        None => Some(unscoped_access(f)),
    };
    let (method, path) = route_of(m, f, config);
    if let Some(resource) = served_resource(m, f, config) {
        let op = ResourceOp { m, f, resource, handler_name: &handler_name, config };
        resource_handler(out, &op, access.expect("a resource op takes the state or a store"), scope);
    } else if is_filtered_list(f) {
        legacy_list_handler(out, m, f, &handler_name, access, scope, config);
    } else {
        op_handler(out, m, f, &op_shape(m, f, config, scope.is_some()), &handler_name, access, scope, config);
    }
    (method, path, handler_name)
}

/// The handler name: the IPC command's name, which is unique in the file,
/// and for a scoped handler that name with `_scoped`.
fn handler_name(m: &ApiModule, f: &ApiFn, config: &Config, scoped: bool) -> String {
    let name = crate::servers::generators::ipc::command_name(&m.name, f, config);
    if scoped { format!("{name}_scoped") } else { name }
}

/// The method and route `f` is served at: under the route prefix when `f`
/// is store-scoped and one is configured. The generator and the server
/// metadata both read it, so they cannot disagree.
pub(in crate::servers) fn route_of(m: &ApiModule, f: &ApiFn, config: &Config) -> (&'static str, String) {
    let scope = config.route_prefix.as_ref().filter(|_| f.first_param_is_store);
    let url = config.naming.url_for_module(m);
    let base = match scope {
        None => format!("/api/{url}"),
        Some(prefix) => format!("/api/{}/{url}", axum_path(&prefix.segments)),
    };
    let (method, path) = if served_resource(m, f, config).is_some() {
        match classify_op(f) {
            OpKind::List => ("get", ""),
            OpKind::GetById => ("get", "/{id}"),
            OpKind::Create => ("post", ""),
            OpKind::Update => ("patch", "/{id}"),
            OpKind::Delete => ("delete", "/{id}"),
            _ => unreachable!("served_resource picks CRUD ops only"),
        }
    } else if is_filtered_list(f) {
        ("get", "")
    } else {
        let shape = op_shape(m, f, config, scope.is_some());
        return (shape.method, format!("{base}{}", shape.path));
    };
    (method, format!("{base}{path}"))
}

fn is_junction(f: &ApiFn) -> bool {
    matches!(classify_op(f), OpKind::JunctionList { .. } | OpKind::JunctionAdd { .. } | OpKind::JunctionRemove { .. })
}

/// A `list` that takes anything but its page: a filter. No filter is read
/// from the wire as JSON:API (§7.3), so this list keeps its flat handler.
fn is_filtered_list(f: &ApiFn) -> bool {
    classify_op(f) == OpKind::List && list_takes_filter(f)
}

/// Every resource an event op's item type names, with the item type as the
/// file names it, in event order.
fn event_resources<'a>(modules: &'a [ApiModule], config: &'a Config) -> Vec<(&'a Resource, &'a str)> {
    let mut found: Vec<(&Resource, &str)> = Vec::new();
    for ev in modules.iter().flat_map(|m| &m.events) {
        if let Some(resource) = config.resources.by_item_type(&ev.item_type_ast)
            && !found.iter().any(|(r, _)| r.module == resource.module)
        {
            found.push((resource, &ev.item_type));
        }
    }
    found
}

/// `code` without its comment lines, which name types and helpers without
/// using them.
fn without_comments(code: &str) -> String {
    code.lines().filter(|line| !line.trim_start().starts_with("//")).flat_map(|line| [line, "\n"]).collect()
}

/// True when `body` names `ident` unqualified: as a whole identifier, not
/// after `::`.
fn uses_ident(body: &str, ident: &str) -> bool {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    body.match_indices(ident).any(|(at, _)| {
        let before = &body[..at];
        let after = body[at + ident.len()..].chars().next();
        !before.ends_with(|c: char| is_ident(c) || c == ':') && !after.is_some_and(is_ident)
    })
}

/// The `use` items of `axum`, `ontogen_jsonapi` and `serde` that `body`
/// names, and the routing fns `routes` calls.
fn runtime_imports(body: &str, routes: &Routes) -> String {
    let used = |names: &[&'static str]| -> Vec<&'static str> {
        names.iter().copied().filter(|name| uses_ident(body, name)).collect()
    };
    let tree = |prefix: &str, items: Vec<String>| match items.as_slice() {
        [] => None,
        [item] if item == "self" => Some(prefix.to_string()),
        [item] => Some(format!("{prefix}::{item}")),
        _ => Some(format!("{prefix}::{{{}}}", items.join(", "))),
    };
    let strings = |names: Vec<&str>| names.into_iter().map(str::to_string).collect::<Vec<_>>();

    let mut extract = strings(used(&["State"]));
    extract.extend(tree("rejection", strings(used(&["QueryRejection"]))));
    let mut axum = vec!["Router".to_string()];
    axum.extend(tree("extract", extract));
    axum.extend(tree("http", strings(used(&["Method", "StatusCode"]))));
    axum.extend(tree("response", strings(used(&["Json", "Response"]))));
    axum.extend(tree("routing", strings(routes.methods())));

    let mut jsonapi = strings(used(&[
        "Document",
        "ErrorCode",
        "ErrorObject",
        "Linkage",
        "Links",
        "LookupKey",
        "PageMeta",
        "QueryParams",
        "QuerySpec",
        "Relationship",
        "ResourceIdentifier",
        "ResourceObject",
        "ResultFrame",
        "ResultMeta",
    ]));
    jsonapi.extend(tree("error", strings(used(&["method_not_allowed"]))));
    jsonapi.extend(tree("extract", strings(used(&["AcceptGuard", "Body", "NoParams", "Path", "Query", "RouteQuery"]))));
    jsonapi.extend(tree("links", strings(used(&["CanonicalQuery", "encode_path_segment", "pagination_links"]))));
    let mut request = Vec::new();
    if body.contains("request::") {
        request.push("self".to_string());
    }
    request.extend(strings(used(&["Endpoint", "LinkedId", "ResourceData"])));
    jsonapi.extend(tree("request", request));
    if body.contains("response::") {
        jsonapi.push("response".to_string());
    }

    let mut out = format!("use axum::{{{}}};\n", axum.join(", "));
    out.push_str(&format!("use ontogen_jsonapi::{{{}}};\n", jsonapi.join(", ")));
    if let Some(serde) = tree("serde", strings(used(&["Deserialize", "Serialize"]))) {
        out.push_str(&format!("use {serde};\n"));
    }
    out
}

/// `app_error` maps the consumer's `AppError` (§13.4); `internal_error`
/// takes every failure no `AppError` describes.
fn emit_error_helpers(out: &mut String, config: &Config) {
    match &config.error_map {
        Some(map) => {
            let path = app_error_path(config);
            out.push_str(&format!(
                "/// An `AppError` as an error object: the status its variant's name gives,\n/// and the name in \
                 snake_case as the code.\nfn app_error(e: {path}) -> ErrorObject {{\n    let (status, code) \
                 = match &e {{\n"
            ));
            for v in &map.variants {
                out.push_str(&format!(
                    "        {} => ({}, \"{}\"),\n",
                    v.pattern(&path),
                    status_const(v.status),
                    v.code
                ));
            }
            out.push_str("    };\n    ErrorObject::app(status, code, e.to_string())\n}\n\n");
        }
        None => out.push_str(
            "\
/// No `AppError` was found in the schema directory, so no error carries a
/// status of its own: every one is a `500`.
fn app_error(e: impl std::fmt::Display) -> ErrorObject {
    ErrorObject::internal(e.to_string())
}

",
        ),
    }
    out.push_str(
        "\
/// A failure no `AppError` describes: opening the store, a scope accessor,
/// or an op with another error type.
fn internal_error(e: impl std::fmt::Display) -> ErrorObject {
    ErrorObject::internal(e.to_string())
}

",
    );
}

fn status_const(status: u16) -> String {
    match status {
        400 => "StatusCode::BAD_REQUEST".to_string(),
        403 => "StatusCode::FORBIDDEN".to_string(),
        404 => "StatusCode::NOT_FOUND".to_string(),
        409 => "StatusCode::CONFLICT".to_string(),
        500 => "StatusCode::INTERNAL_SERVER_ERROR".to_string(),
        other => format!("StatusCode::from_u16({other}).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)"),
    }
}

/// Emitted once: the `405` fallback every route installs (§13.5).
const ROUTE_HELPERS: &str = "\
/// The method fallback of a route serving `allowed`: `405` with `Allow`.
fn allow<const N: usize>(
    allowed: [Method; N],
) -> impl Fn(Method) -> std::future::Ready<Response> + Clone + Send + Sync + 'static {
    move |method| std::future::ready(method_not_allowed(&method, &allowed))
}

";

/// Helpers emitted only when a handler names them, each with its name.
const ON_DEMAND_HELPERS: &[(&str, &str)] = &[
    (
        "query_rejection",
        "\
/// The error document for a rejection of Axum's own `Query`, which a list
/// that takes a filter and an event stream still read their parameters with.
fn query_rejection(e: QueryRejection) -> ErrorObject {
    ErrorObject::new(ErrorCode::InvalidQueryParameter, e.body_text())
}

",
    ),
    (
        "PaginatedResult",
        "\
/// One page of a list that is not served as a resource.
#[derive(Serialize)]
pub struct PaginatedResult<T: Serialize> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

",
    ),
    (
        "PaginationParams",
        "\
/// The page of a list that takes a filter, beside its filter.
#[derive(Deserialize)]
pub struct PaginationParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

",
    ),
    (
        "result_frame",
        "\
/// Writes an event item that is not an entity as `{\"meta\":{\"result\":…}}`.
fn result_frame<T: Serialize>(event: Event, item: &T) -> Result<Event, axum::Error> {
    event.json_data(ResultFrame::new(item))
}

",
    ),
    (
        "PageOpArgs",
        "\
/// A paginated list that is not served as a resource takes its page as
/// `opArg[limit]` and `opArg[offset]`.
struct PageOpArgs;

impl RouteQuery for PageOpArgs {
    const SPEC: QuerySpec = QuerySpec { op_args: &[\"limit\", \"offset\"], ..QuerySpec::NONE };
}

",
    ),
];

/// Emitted once when any resource is served: the query parameters each
/// resource route accepts (§6), and the rules every resource shares.
const RESOURCE_HELPERS: &str = "\
// ── JSON:API resources ──

struct ListParams;

impl RouteQuery for ListParams {
    const SPEC: QuerySpec = QuerySpec { sort: true, include: true, ..QuerySpec::NONE };
}

struct PagedListParams;

impl RouteQuery for PagedListParams {
    const SPEC: QuerySpec = QuerySpec { sort: true, include: true, page: true, ..QuerySpec::NONE };
}

struct GetParams;

impl RouteQuery for GetParams {
    const SPEC: QuerySpec = QuerySpec { include: true, ..QuerySpec::NONE };
}

/// No list takes an `order` argument, so every `sort` asks for an order the
/// server does not support.
fn refuse_sort(query: &QueryParams, type_name: &str) -> Result<(), ErrorObject> {
    match query.sort()? {
        None => Ok(()),
        Some(_) => Err(ErrorObject::new(ErrorCode::InvalidSortField, format!(\"`{type_name}` cannot be sorted\"))
            .with_parameter(\"sort\")),
    }
}

/// No route includes related resources, so every `include` names a path the
/// server cannot include.
fn refuse_include(query: &QueryParams, type_name: &str) -> Result<(), ErrorObject> {
    match query.include()? {
        None => Ok(()),
        Some(_) => Err(ErrorObject::new(
            ErrorCode::InvalidIncludePath,
            format!(\"`{type_name}` has no relationship that can be included\"),
        )
        .with_parameter(\"include\")),
    }
}

/// The effective `(offset, limit)` of a paginated list.
fn page(query: &QueryParams, default_limit: u32, max_limit: u32) -> Result<(u32, u32), ErrorObject> {
    let offset = query.page_offset()?.unwrap_or(0);
    let limit = query.page_limit()?.unwrap_or(default_limit).min(max_limit);
    Ok((offset, limit))
}

/// Sets `name` when the request document carried it.
fn set_field(fields: &mut serde_json::Map<String, serde_json::Value>, name: &str, value: Option<&serde_json::Value>) {
    if let Some(value) = value {
        fields.insert(name.to_owned(), value.clone());
    }
}

/// A create or update input from the fields its request document set. Each
/// field was checked against its type, so a failure here is the server's.
fn from_fields<T: serde::de::DeserializeOwned>(
    fields: serde_json::Map<String, serde_json::Value>,
) -> Result<T, ErrorObject> {
    serde_json::from_value(serde_json::Value::Object(fields)).map_err(internal_error)
}

";

/// The Rust type a module's CRUD fns read and return, as this file names it.
fn entity_type(m: &ApiModule) -> Option<String> {
    m.functions.iter().find_map(|f| match classify_op(f) {
        OpKind::List if f.return_type.starts_with("Vec<") => Some(inner_type(&f.return_type)),
        OpKind::GetById | OpKind::Create | OpKind::Update => Some(f.return_type.clone()),
        _ => None,
    })
}

/// Names of a resource's generated helpers. They carry the module name and
/// a suffix that no `{entity}_{fn}` handler name is likely to end in.
struct ResourceNames {
    attributes: String,
    linked: String,
    resource: String,
    fields: String,
    key: String,
    frame: String,
}

fn resource_names(module: &str) -> ResourceNames {
    let pascal = to_pascal_case(module);
    ResourceNames {
        attributes: format!("{pascal}ResourceAttributes"),
        linked: format!("{pascal}LinkedIds"),
        resource: format!("{module}_as_resource"),
        fields: format!("{module}_request_fields"),
        key: format!("{module}_lookup_key"),
        frame: format!("{module}_frame_data"),
    }
}

/// The attributes serializer and the resource builder of `resource`, whose
/// entity this file names `entity_ty` (§5).
fn emit_resource_object(out: &mut String, resource: &Resource, entity_ty: &str) {
    let names = resource_names(&resource.module);
    let type_name = &resource.resource_type;
    let entity = &resource.entity.name;
    let id_field = &resource.id_field;
    let attrs = &names.attributes;
    out.push_str(&format!(
        "/// `{entity}`'s attributes: every field but the id and the relations, in\n/// declaration \
         order.\nstruct {attrs}<'a>(&'a {entity_ty});\n\nimpl Serialize for {attrs}<'_> {{\n    fn \
         serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {{\n        use \
         serde::ser::SerializeStruct;\n        let mut attributes = serializer.serialize_struct(\"{attrs}\", \
         {})?;\n",
        resource.attributes.len()
    ));
    for a in &resource.attributes {
        out.push_str(&format!("        attributes.serialize_field(\"{}\", &self.0.{})?;\n", a.name, a.field));
    }
    out.push_str("        attributes.end()\n    }\n}\n\n");

    out.push_str(&format!(
        "/// `entity` as a resource object of type `{type_name}`, its `links.self`\n/// under `collection`.\nfn \
         {}<'a>(entity: &'a {entity_ty}, collection: &str) -> ResourceObject<{attrs}<'a>> {{\n    let self_link \
         = format!(\"{{collection}}/{{}}\", encode_path_segment(&entity.{id_field}));\n    \
         ResourceObject::new(\"{type_name}\", entity.{id_field}.clone(), {attrs}(entity), self_link)",
        names.resource
    ));
    for rel in &resource.relationships {
        let target = &rel.target_type;
        let field = &rel.field;
        let linkage = match rel.arity {
            Arity::ToOne { nullable: true } => format!(
                "Linkage::ToOne(entity.{field}.as_ref().map(|id| ResourceIdentifier::new(\"{target}\", \
                 id.as_str())))"
            ),
            Arity::ToOne { nullable: false } => {
                format!("Linkage::ToOne(Some(ResourceIdentifier::new(\"{target}\", entity.{field}.as_str())))")
            }
            Arity::ToMany => format!(
                "Linkage::ToMany(entity.{field}.iter().map(|id| ResourceIdentifier::new(\"{target}\", \
                 id.as_str())).collect())"
            ),
        };
        out.push_str(&format!("\n        .with_relationship(\"{}\", Relationship::from_data({linkage}))", rel.name));
    }
    out.push_str("\n}\n\n");
}

/// The writer of an event frame's `data:` for an item of `resource`'s
/// entity, which this file names `item_type`: its resource object without
/// links (§12). The resource builder needs a collection for the links it
/// leaves out; the unscoped one is as good as any.
fn emit_frame_data(out: &mut String, resource: &Resource, item_type: &str) {
    let names = resource_names(&resource.module);
    out.push_str(&format!(
        "/// A `{}` event item as its resource object. A frame is not tied to a\n/// request URL, so it carries no \
         links.\nfn {}(event: Event, entity: &{item_type}) -> Result<Event, axum::Error> {{\n    \
         event.json_data({}(entity, \"/api/{}\").into_unlinked())\n}}\n\n",
        resource.entity.name, names.frame, names.resource, resource.resource_type
    ));
}

/// Per resource, emitted once whichever handlers serve it: the attributes
/// serializer and resource builder (§5), the `{id}` lookup key (§8.1), and
/// the reader of create and update documents (§8.2, §8.3 step 7). With the
/// resource among `frames`, its event frame writer too (§12).
///
/// Returns whether it emitted the resource builder.
fn emit_resource_helpers(
    out: &mut String,
    m: &ApiModule,
    modules: &[ApiModule],
    config: &Config,
    frames: &[(&Resource, &str)],
) -> bool {
    let served: Vec<OpKind> =
        m.functions.iter().filter(|f| served_resource(m, f, config).is_some()).map(classify_op).collect();
    if served.is_empty() {
        return false;
    }
    let Some(resource) = config.resources.by_module(&m.name) else { return false };
    let names = resource_names(&m.name);
    let type_name = &resource.resource_type;
    let entity = &resource.entity.name;
    let id_field = &resource.id_field;

    out.push_str(&format!("// ── `{type_name}` ──\n\n"));

    let entity_ty = entity_type(m).or_else(|| {
        frames.iter().find(|(r, _)| r.module == resource.module).map(|(_, item_type)| item_type.to_string())
    });
    if let Some(entity_ty) = &entity_ty {
        emit_resource_object(out, resource, entity_ty);
    }
    let emitted_object = entity_ty.is_some();

    // An `{id}` that does not percent-decode names nothing: the store's own
    // `404` for a missing id (§8.1).
    let not_found = format!("{entity}NotFound");
    let missing = match config.error_map.as_ref().and_then(|map| map.variants.iter().find(|v| v.name == not_found)) {
        Some(v) if v.shape == VariantShape::Tuple(1) => {
            format!("app_error({}::{not_found}(id.to_string()))", app_error_path(config))
        }
        _ => format!("ErrorObject::internal(format!(\"`{{id}}` names no `{type_name}`\"))"),
    };
    out.push_str(&format!(
        "/// The id an `{{id}}` path segment names.\nfn {}(id: &LookupKey) -> Result<&str, ErrorObject> {{\n    \
         id.as_str().ok_or_else(|| {missing})\n}}\n\n",
        names.key
    ));

    if !served.iter().any(|op| matches!(op, OpKind::Create | OpKind::Update)) {
        return emitted_object;
    }

    let has_relationships = !resource.relationships.is_empty();
    if has_relationships {
        out.push_str(&format!(
            "/// The ids a request document for `{type_name}` links, by relationship, to\n/// be checked to name \
             resources that exist.\n#[derive(Default)]\nstruct {} {{\n",
            names.linked
        ));
        for rel in &resource.relationships {
            let ty = if rel.is_to_many() { "Vec<LinkedId>" } else { "Option<LinkedId>" };
            out.push_str(&format!("    {}: {ty},\n", rel.name));
        }
        out.push_str("}\n\n");
    }

    let returns = if has_relationships {
        format!("(serde_json::Map<String, serde_json::Value>, {})", names.linked)
    } else {
        "serde_json::Map<String, serde_json::Value>".to_string()
    };
    let declared: Vec<String> = resource.attributes.iter().map(|a| format!("\"{}\"", a.name)).collect();
    let rel_fields: Vec<String> =
        resource.relationships.iter().map(|r| format!("(\"{}\", \"{}\")", member_name(&r.field), r.name)).collect();
    out.push_str(&format!(
        "/// The fields a create or update document for `{type_name}` sets, named as\n/// the input's fields, \
         each member checked against the schema.\nfn {}(data: &ResourceData, create: bool) -> Result<{returns}, ErrorObject> {{\n    let \
         attributes = data.attributes.as_ref();\n    request::check_attribute_names(attributes, \"{type_name}\", \
         &[{}], &[{}])?;\n    let mut fields = serde_json::Map::new();\n    if create {{\n        \
         fields.insert(\"{}\".to_owned(), serde_json::Value::String(data.id.clone().unwrap_or_default()));\n    \
         }}\n",
        names.fields,
        declared.join(", "),
        rel_fields.join(", "),
        member_name(id_field),
    ));
    for a in &resource.attributes {
        let field = resource.entity.fields.iter().find(|fd| fd.name == a.field).expect("an attribute is a field");
        let required = if create_field_required(field) { "create" } else { "false" };
        out.push_str(&format!(
            "    set_field(&mut fields, \"{name}\", request::attribute::<{ty}>(attributes, \"{name}\", \
             {required})?);\n",
            name = a.name,
            ty = field_to_create_type(field),
        ));
    }
    if !has_relationships {
        out.push_str(&format!(
            "    request::check_relationship_names(data.relationships()?, \"{type_name}\", &[])?;\n    \
             Ok(fields)\n}}\n\n"
        ));
        return emitted_object;
    }
    let rel_names: Vec<String> = resource.relationships.iter().map(|r| format!("\"{}\"", r.name)).collect();
    out.push_str(&format!(
        "    let relationships = data.relationships()?;\n    request::check_relationship_names(relationships, \
         \"{type_name}\", &[{}])?;\n    let mut linked = {}::default();\n",
        rel_names.join(", "),
        names.linked
    ));
    for rel in &resource.relationships {
        let name = &rel.name;
        let target = &rel.target_type;
        let field = member_name(&rel.field);
        let pointer = format!("/data/relationships/{name}");
        match rel.arity {
            Arity::ToOne { nullable } => {
                let read = format!(
                    "let id = request::to_one(rel, \"{pointer}\", \"{target}\", {nullable})?;\n        \
                     fields.insert(\"{field}\".to_owned(), id.clone().map_or(serde_json::Value::Null, \
                     serde_json::Value::String));\n        linked.{name} = id.map(|id| LinkedId {{ id, pointer: \
                     \"{pointer}/data\".to_owned() }});\n"
                );
                if nullable {
                    out.push_str(&format!(
                        "    if let Some(rel) = relationships.and_then(|r| r.get(\"{name}\")) {{\n        {read}    }}\n"
                    ));
                } else {
                    out.push_str(&format!(
                        "    match relationships.and_then(|r| r.get(\"{name}\")) {{\n        Some(rel) => {{\n        \
                         {read}        }}\n        None if create => return \
                         Err(request::missing_relationship(\"{name}\", relationships)),\n        None => {{}}\n    }}\n"
                    ));
                }
            }
            Arity::ToMany => out.push_str(&format!(
                "    if let Some(rel) = relationships.and_then(|r| r.get(\"{name}\")) {{\n        let ids = \
                 request::to_many_linked(rel, \"{pointer}\", \"{target}\", None)?;\n        \
                 fields.insert(\"{field}\".to_owned(), ids.iter().map(|l| \
                 serde_json::Value::String(l.id.clone())).collect());\n        linked.{name} = ids;\n    }}\n"
            )),
        }
    }
    out.push_str("    Ok((fields, linked))\n}\n\n");
    emit_check_linked(out, m, resource, modules, config);
    emitted_object
}

/// One CRUD op served as its resource.
#[derive(Clone, Copy)]
struct ResourceOp<'a> {
    m: &'a ApiModule,
    f: &'a ApiFn,
    resource: &'a Resource,
    handler_name: &'a str,
    config: &'a Config,
}

/// Whether `f`'s handler is the scoped one: a store-scoped fn under a
/// `route_prefix` has no other.
fn is_scoped(f: &ApiFn, config: &Config) -> bool {
    f.first_param_is_store && config.route_prefix.is_some()
}

/// `{module}_check_linked`, or `{module}_check_linked_scoped` for the scoped
/// handlers.
fn check_linked_fn(module: &str, scoped: bool) -> String {
    if scoped { format!("{module}_check_linked_scoped") } else { format!("{module}_check_linked") }
}

/// Step 8 of a create or update (§13.2), once per resource and handler
/// kind: every linked id, in declared relationship order, fetched with its
/// target's `get_by_id`.
///
/// The helper opens each store it reads through the accessor its handler
/// would use, since the store's type cannot be named here: the handler's own
/// store is opened twice, which costs one accessor call.
fn emit_check_linked(out: &mut String, m: &ApiModule, resource: &Resource, modules: &[ApiModule], config: &Config) {
    let writes = m.functions.iter().filter(|f| {
        matches!(classify_op(f), OpKind::Create | OpKind::Update) && served_resource(m, f, config).is_some()
    });
    let mut kinds: Vec<bool> = writes.map(|f| is_scoped(f, config)).collect();
    kinds.sort_unstable();
    kinds.dedup();

    let type_name = &resource.resource_type;
    let state_type = &config.state_type;
    let linked_ty = resource_names(&m.name).linked;
    for scoped in kinds {
        let prefix = config.route_prefix.as_ref().filter(|_| scoped);
        let scope_param = prefix.map(|p| format!("{SCOPE}: &{}, ", p.params[0].rust_type));
        let mut opens: Vec<String> = Vec::new();
        let mut checks = String::new();
        for rel in &resource.relationships {
            let Some((tm, tf)) = linked_lookup(modules, &rel.target_module) else { continue };
            let svc = tm.service_ident(tf.surface);
            let arg = match (tf.first_param_is_store, prefix) {
                (false, _) => "state".to_string(),
                (true, Some(prefix)) => {
                    let open =
                        format!("    let store = state.{}({SCOPE}).map_err(internal_error)?;\n", prefix.state_accessor);
                    if !opens.contains(&open) {
                        opens.push(open);
                    }
                    "&store".to_string()
                }
                (true, None) => {
                    let accessor = &tf.store_accessor;
                    let open = format!("    let {accessor} = state.{accessor}().await.map_err(internal_error)?;\n");
                    if !opens.contains(&open) {
                        opens.push(open);
                    }
                    format!("&{accessor}")
                }
            };
            let not_found = format!("{}NotFound", rel.target_entity);
            let not_found_arm = config
                .error_map
                .as_ref()
                .and_then(|map| map.variants.iter().find(|v| v.name == not_found))
                .filter(|_| returns_app_error(tf, config))
                .map(|v| {
                    format!(
                        "            Err({}) => return Err(linked.not_found(\"{}\")),\n",
                        v.pattern(&app_error_path(config)),
                        rel.target_type
                    )
                })
                .unwrap_or_default();
            let fallback = if returns_app_error(tf, config) { "app_error" } else { "internal_error" };
            let each = if rel.is_to_many() { "for linked in" } else { "if let Some(linked) =" };
            checks.push_str(&format!(
                "    {each} &linked.{} {{\n        match {svc}::get_by_id({arg}, &linked.id){} {{\n            \
                 Ok(_) => {{}}\n{not_found_arm}            Err(e) => return Err({fallback}(e)),\n        }}\n    }}\n",
                rel.name,
                await_str(tf.is_async),
            ));
        }
        out.push_str(&format!(
            "/// Checks that each id a create or update document for `{type_name}` links\n/// names a resource \
             that exists, in the order the document was read.\nasync fn {}(state: &{state_type}, {}linked: &{linked_ty}) -> \
             Result<(), ErrorObject> {{\n{}{checks}    Ok(())\n}}\n\n",
            check_linked_fn(&m.name, scoped),
            scope_param.unwrap_or_default(),
            opens.concat(),
        ));
    }
}

/// The step-7 read of a create or update body, and the step-8 call that
/// checks every id it links (§13.2).
fn write_steps(op: &ResourceOp<'_>) -> (String, String) {
    let fields_fn = resource_names(&op.m.name).fields;
    let create = classify_op(op.f) == OpKind::Create;
    if op.resource.relationships.is_empty() {
        return (format!("    let fields = {fields_fn}(&data, {create})?;\n"), String::new());
    }
    let scoped = is_scoped(op.f, op.config);
    let scope_arg = if op.config.route_prefix.is_some() && scoped { format!("&{SCOPE}, ") } else { String::new() };
    (
        format!("    let (fields, linked) = {fields_fn}(&data, {create})?;\n"),
        format!("    {}(&ontogen_state, {scope_arg}&linked).await?;\n", check_linked_fn(&op.m.name, scoped)),
    )
}

/// Emit the JSON:API handler of one CRUD op (§7, §8), served at
/// [`route_of`].
///
/// Checks run in §13.2 order: `Accept`, then `Content-Type` when the op
/// reads a body, then the path, the query and the body. With `scope`, the
/// prefix parameter comes first in the path, and links carry the prefix
/// (§11.1).
fn resource_handler(out: &mut String, op: &ResourceOp<'_>, access: Access, scope: Option<&RoutePrefix>) {
    let ResourceOp { m, f, resource, handler_name, config, .. } = *op;
    let names = resource_names(&m.name);
    let svc = m.service_ident(f.surface);
    let type_name = &resource.resource_type;
    let url_plural = config.naming.url_for_module(m);
    let state_type = &config.state_type;
    let aw = await_str(f.is_async);
    let map_err = err_map(f, config);
    let Access { open, arg } = access;
    let key = &names.key;
    let as_resource = &names.resource;

    // The collection's and the item's path parameters, as a pattern and the
    // type `Path` reads.
    let (collection, collection_path, item_path) = match scope {
        None => (
            format!("    let collection = \"/api/{url_plural}\";\n"),
            None,
            ("id".to_string(), "LookupKey".to_string()),
        ),
        Some(prefix) => {
            let pp = &prefix.params[0];
            let mut args = Vec::new();
            let template: Vec<String> = prefix
                .segments
                .split('/')
                .map(|segment| match segment.strip_prefix(':') {
                    Some(name) => {
                        let ident = if name == pp.name { SCOPE } else { name };
                        args.push(format!("encode_path_segment(&{ident}.to_string())"));
                        "{}".to_string()
                    }
                    None => segment.to_string(),
                })
                .collect();
            (
                format!(
                    "    let collection = &format!(\"/api/{}/{url_plural}\", {});\n",
                    template.join("/"),
                    args.join(", ")
                ),
                Some((SCOPE.to_string(), pp.rust_type.clone())),
                (format!("({SCOPE}, id)"), format!("({}, LookupKey)", pp.rust_type)),
            )
        }
    };
    let extract = |(pattern, ty): &(String, String)| format!("    Path({pattern}): Path<{ty}>,\n");
    // A handler that reads a body answers its media type (§13.2 step 3)
    // before its path and query (steps 4 and 5), but Axum runs `Body` last:
    // so it takes those two as `Result`s and answers them after `Body`.
    let deferred = |path: Option<&(String, String)>| {
        let (extractors, checks) = match path {
            Some((pattern, ty)) => (
                format!("    path_params: Result<Path<{ty}>, ErrorObject>,\n"),
                format!("    let Path({pattern}) = path_params?;\n"),
            ),
            None => (String::new(), String::new()),
        };
        (
            format!("{extractors}    query: Result<Query<NoParams>, ErrorObject>,\n    body: Body,\n"),
            format!("{checks}    query?;\n    let body = body.into_bytes()?;\n"),
        )
    };
    let collection_extract = collection_path.as_ref().map(extract).unwrap_or_default();
    let item_extract = extract(&item_path);
    let head =
        format!("async fn {handler_name}(\n    State(ontogen_state): State<Arc<{state_type}>>,\n    _: AcceptGuard,\n");
    let tail = ") -> Result<Response, ErrorObject> {\n";
    let single_response = format!(
        "    let resource = {as_resource}(&entity, collection);\n    let links = \
         Links::new(resource.links().self_link());\n    Ok(response::ok(&Document::new(resource, links)))\n}}\n\n"
    );

    match classify_op(f) {
        OpKind::List => {
            let (params, page, call, document) = match config.pagination_for(&m.name, f.surface) {
                Some(pg) => (
                    "PagedListParams",
                    format!("    let (offset, limit) = page(&query, {}, {})?;\n", pg.default_limit, pg.max_limit),
                    format!(
                        "    let items = {svc}::list({arg}, Some(u64::from(limit)), \
                         Some(u64::from(offset))){aw}{map_err}?;\n    let total = \
                         {svc}::count({arg}){aw}{map_err}?;\n"
                    ),
                    "    let links = pagination_links(collection, &CanonicalQuery::new(), offset, limit, total);\n    \
                     Ok(response::ok(&Document::new(data, links).with_meta(PageMeta { total, limit, offset })))\n"
                        .to_string(),
                ),
                None => {
                    let page_args = if f.takes_page() { ", None, None" } else { "" };
                    (
                        "ListParams",
                        String::new(),
                        format!("    let items = {svc}::list({arg}{page_args}){aw}{map_err}?;\n"),
                        "    Ok(response::ok(&Document::new(data, Links::new(collection))))\n".to_string(),
                    )
                }
            };
            out.push_str(&format!(
                "{head}{collection_extract}    query: Query<{params}>,\n{tail}    refuse_sort(&query, \
                 \"{type_name}\")?;\n    refuse_include(&query, \"{type_name}\")?;\n{page}{open}{call}{collection}    \
                 let data: Vec<_> = items.iter().map(|entity| {as_resource}(entity, \
                 collection)).collect();\n{document}}}\n\n"
            ));
        }
        OpKind::GetById => {
            out.push_str(&format!(
                "{head}{item_extract}    query: Query<GetParams>,\n{tail}    refuse_include(&query, \
                 \"{type_name}\")?;\n{open}    let entity = {svc}::get_by_id({arg}, \
                 {key}(&id)?){aw}{map_err}?;\n{collection}{single_response}"
            ));
        }
        OpKind::Create => {
            let input_ty = extract_input_type(&f.params[0].ty);
            let (fields, checks) = write_steps(op);
            let already_exists = format!("{}AlreadyExists", resource.entity.name);
            let variant = config
                .error_map
                .as_ref()
                .and_then(|map| map.variants.iter().find(|v| v.name == already_exists))
                .filter(|_| returns_app_error(f, config));
            // Only a client id is in the request for the pointer to name
            // (§8.2).
            let create_err = match variant {
                Some(v) => format!(
                    ".map_err(|e| match e {{\n            e @ {} if data.id.is_some() => \
                     app_error(e).with_pointer(\"/data/id\"),\n            e => app_error(e),\n        }})",
                    v.pattern(&app_error_path(config))
                ),
                None => map_err.to_string(),
            };
            let (extractors, checks_first) = deferred(collection_path.as_ref());
            out.push_str(&format!(
                "{head}{extractors}{tail}{checks_first}{collection}    let endpoint = Endpoint {{ type_name: \
                 \"{type_name}\", path: collection }};\n    let data = request::parse_create(&body, endpoint, |id| {{\n        \
                 ontogen_core::id::validate_id(id).map_err(|e| e.reason)\n    }})?;\n{fields}    let input: \
                 {input_ty} = from_fields(fields)?;\n{open}{checks}    let entity = {svc}::create({arg}, \
                 input){aw}{create_err}?;\n    let resource = {as_resource}(&entity, collection);\n    let location \
                 = resource.links().self_link().to_owned();\n    let links = Links::new(location.as_str());\n    \
                 Ok(response::created(&location, &Document::new(resource, links)))\n}}\n\n"
            ));
        }
        OpKind::Update => {
            let input_ty = extract_input_type(&f.params[1].ty);
            let (fields, checks) = write_steps(op);
            let (extractors, checks_first) = deferred(Some(&item_path));
            out.push_str(&format!(
                "{head}{extractors}{tail}{checks_first}{collection}    let path = \
                 format!(\"{{collection}}/{{id}}\");\n    let endpoint = Endpoint {{ type_name: \"{type_name}\", path: \
                 &path }};\n    let data = request::parse_update(&body, endpoint, &id)?;\n{fields}    let input: {input_ty} = \
                 from_fields(fields)?;\n{open}{checks}    let entity = {svc}::update({arg}, {key}(&id)?, \
                 input){aw}{map_err}?;\n{single_response}"
            ));
        }
        OpKind::Delete => {
            out.push_str(&format!(
                "{head}{item_extract}    _: Query<NoParams>,\n{tail}{open}    {svc}::delete({arg}, \
                 {key}(&id)?){aw}{map_err}?;\n    Ok(response::no_content())\n}}\n\n"
            ));
        }
        _ => unreachable!("served_resource picks CRUD ops only"),
    }
}

/// Emit a list that takes a filter: its query keeps Axum's flat `Query`, and
/// its success its flat shape. Under `scope` it is the same handler with the
/// prefix parameter first, paging through the store as the unscoped one does.
fn legacy_list_handler(
    out: &mut String,
    m: &ApiModule,
    f: &ApiFn,
    handler_name: &str,
    access: Option<Access>,
    scope: Option<&RoutePrefix>,
    config: &Config,
) {
    let svc = m.service_ident(f.surface);
    let ret_type = &f.return_type;
    let state_type = &config.state_type;
    let await_str = if f.is_async { "\n        .await" } else { "" };
    let err_map = err_map(f, config);
    let (store_let, first_arg) = match access {
        Some(Access { open, arg }) => (open, vec![arg.to_string()]),
        None => (String::new(), Vec::new()),
    };
    let pagination = config.pagination_for(&m.name, f.surface);
    let paginated = pagination.is_some() && ret_type.starts_with("Vec<");
    // A `*Query` struct, or plain params (`skill_id: &str`). A list that
    // takes the page owns its `limit`/`offset`: they are never filters.
    let query_param = f.params.iter().find(|p| p.ty.contains("Query"));
    let plain_params: Vec<_> = f
        .params
        .iter()
        .filter(|p| !p.ty.contains("Query") && !p.is_input() && (!f.takes_page() || !is_page_param(p)))
        .collect();

    let mut extractors = String::new();
    if !f.is_stateless {
        extractors.push_str(&format!("\n    State(ontogen_state): State<Arc<{state_type}>>,"));
    }
    if let Some(prefix) = scope {
        extractors.push_str(&format!("\n    Path({SCOPE}): Path<{}>,", prefix.params[0].rust_type));
    }
    let mut unwraps = String::new();
    let mut filter_args: Vec<String> = Vec::new();
    if let Some(qp) = query_param {
        let qt = extract_input_type(&qp.ty);
        extractors.push_str(&format!("\n    ontogen_filter: Result<axum::extract::Query<{qt}>, QueryRejection>,"));
        unwraps.push_str("    let axum::extract::Query(ontogen_filter) = ontogen_filter.map_err(query_rejection)?;\n");
        filter_args.push("ontogen_filter".to_string());
    }
    for pp in &plain_params {
        let name = &pp.name;
        extractors.push_str(&format!("\n    {name}: Result<axum::extract::Query<String>, QueryRejection>,"));
        unwraps.push_str(&format!("    let axum::extract::Query({name}) = {name}.map_err(query_rejection)?;\n"));
        filter_args.push(format!("&{name}"));
    }

    if let Some(pg) = pagination
        && paginated
    {
        let item_type = inner_type(ret_type);
        let default_limit = pg.default_limit;
        let max_limit = pg.max_limit;
        // `count` takes the same filter after `list` has consumed it, so a
        // by-value filter is cloned into the list call.
        let count_args = [first_arg.clone(), filter_args.clone()].concat().join(", ");
        let mut list_args: Vec<String> = first_arg.clone();
        list_args
            .extend(filter_args.iter().map(|a| if a == "ontogen_filter" { format!("{a}.clone()") } else { a.clone() }));
        list_args.extend(["Some(u64::from(ontogen_limit))".to_string(), "Some(u64::from(ontogen_offset))".to_string()]);
        let list_args = list_args.join(", ");
        extractors.push_str("\n    ontogen_page: Result<axum::extract::Query<PaginationParams>, QueryRejection>,");
        unwraps.push_str("    let axum::extract::Query(ontogen_page) = ontogen_page.map_err(query_rejection)?;\n");
        out.push_str(&format!(
            "\
async fn {handler_name}({extractors}
) -> Result<Json<PaginatedResult<{item_type}>>, ErrorObject> {{
{unwraps}{store_let}    let ontogen_limit = ontogen_page.limit.unwrap_or({default_limit}).min({max_limit});
    let ontogen_offset = ontogen_page.offset.unwrap_or(0);
    let ontogen_items = {svc}::list({list_args}){await_str}
        {err_map}?;
    let ontogen_total = {svc}::count({count_args}){await_str}
        {err_map}?;
    Ok(Json(PaginatedResult {{
        items: ontogen_items,
        total: ontogen_total,
        limit: ontogen_limit,
        offset: ontogen_offset,
    }}))
}}

"
        ));
    } else {
        // This surface does not paginate: a list that takes the page gets the whole table.
        let mut args: Vec<String> = [first_arg, filter_args].concat();
        if f.takes_page() {
            args.extend(["None".to_string(), "None".to_string()]);
        }
        out.push_str(&format!(
            "\
async fn {handler_name}({extractors}
) -> Result<Json<{ret_type}>, ErrorObject> {{
{unwraps}{store_let}    {svc}::list({}){await_str}
        .map(Json)
        {err_map}
}}

",
            args.join(", ")
        ));
    }
}

/// How a custom op, or an op served as one (§10.4), reads its request and
/// answers.
struct OpShape<'a> {
    method: &'static str,
    /// The route below the module's collection: `/{action}/{p}`, `/{id}`, ….
    path: String,
    /// Arguments read from the path, in path order.
    path_args: Vec<&'a Param>,
    /// `Option` arguments read from `opArg[…]`, on a route with no body.
    query_args: Vec<&'a Param>,
    /// Arguments read from `meta.args`, in declaration order, on a route
    /// that reads a body.
    body_args: Option<Vec<&'a Param>>,
    /// The page of a paginated list, read from `opArg[limit]` and
    /// `opArg[offset]`.
    page: Option<Paging>,
    /// Whether success is `204` whatever the fn returns.
    no_content: bool,
}

/// Where a paginated list that is not served as a resource takes its page.
#[derive(Clone, Copy)]
enum Paging {
    /// From the store's page-taking `list`, with `count` for the total.
    Store { default_limit: u32, max_limit: u32 },
    /// From the fn's whole result, sliced in memory: a junction list.
    InMemory { default_limit: u32, max_limit: u32 },
}

/// The request and response shape of `f`, served as a custom op (§10.2):
/// a `CustomGet` or `CustomPost` at `/{action}`, or a CRUD-named or junction
/// op at its §10.4 route. A scoped junction op is served as the custom op
/// its read-ness makes it, at `/{action}`; only that route differs, so a
/// scoped `JunctionList` pages like an unscoped one (§11.1).
///
/// Past the arguments a route names, a route that reads a body reads every
/// other argument from `meta.args`; one that does not reads an `Option`
/// from `opArg[…]` and anything else from one more path segment, as a
/// `CustomGet` does.
fn op_shape<'a>(m: &ApiModule, f: &'a ApiFn, config: &Config, scoped: bool) -> OpShape<'a> {
    let classified = classify_op(f);
    let scoped_junction_list = scoped && matches!(classified, OpKind::JunctionList { .. });
    let op = match &classified {
        OpKind::JunctionList { .. } if scoped => OpKind::CustomGet,
        OpKind::JunctionAdd { .. } | OpKind::JunctionRemove { .. } if scoped => OpKind::CustomPost,
        op => op.clone(),
    };
    let paging = config.pagination_for(&m.name, f.surface).filter(|_| f.return_type.starts_with("Vec<"));
    let (method, mut path, named) = match &op {
        OpKind::CustomGet | OpKind::CustomPost => {
            let action = config.naming.derive_action(&m.name, &f.name);
            let path = if action.is_empty() { String::new() } else { format!("/{action}") };
            (if op == OpKind::CustomGet { "get" } else { "post" }, path, 0)
        }
        OpKind::List => ("get", String::new(), 0),
        OpKind::GetById => ("get", "/{id}".to_string(), 1),
        OpKind::Create => ("post", String::new(), 0),
        OpKind::Update => ("patch", "/{id}".to_string(), 1),
        OpKind::Delete => ("delete", "/{id}".to_string(), 1),
        OpKind::JunctionList { child_segment } => ("get", format!("/{{parent_id}}/{child_segment}"), 1),
        OpKind::JunctionAdd { child_segment } => ("post", format!("/{{parent_id}}/{child_segment}"), 1),
        OpKind::JunctionRemove { child_segment } => {
            ("delete", format!("/{{parent_id}}/{child_segment}/{{child_id}}"), 2)
        }
        OpKind::EventStream => unreachable!("event fns are not ApiFns"),
    };
    let (named, rest) = f.params.split_at(named.min(f.params.len()));
    let mut shape = OpShape {
        method,
        path: String::new(),
        path_args: named.iter().collect(),
        query_args: Vec::new(),
        body_args: None,
        page: None,
        // From the op as classified: a scoped junction add or remove is
        // served as a custom op but answers as its unscoped route does.
        no_content: f.return_type == "()"
            || matches!(classified, OpKind::Delete | OpKind::JunctionAdd { .. } | OpKind::JunctionRemove { .. }),
    };
    match op {
        // An unfiltered list's only parameters are its page.
        OpKind::List => {
            shape.page = paging.map(|pg| Paging::Store { default_limit: pg.default_limit, max_limit: pg.max_limit });
        }
        OpKind::JunctionList { .. } => {
            shape.page = paging.map(|pg| Paging::InMemory { default_limit: pg.default_limit, max_limit: pg.max_limit });
        }
        OpKind::CustomGet if scoped_junction_list => {
            shape.path_args.extend(rest);
            path.extend(rest.iter().map(|p| format!("/{{{}}}", p.name)));
            shape.page = paging.map(|pg| Paging::InMemory { default_limit: pg.default_limit, max_limit: pg.max_limit });
        }
        _ if matches!(method, "post" | "patch") => shape.body_args = Some(rest.iter().collect()),
        _ => {
            for p in rest {
                if p.is_option() {
                    shape.query_args.push(p);
                } else {
                    path.push_str(&format!("/{{{}}}", p.name));
                    shape.path_args.push(p);
                }
            }
        }
    }
    shape.path = path;
    shape
}

/// Emit the handler of an op served as a custom op (§10): a meta-only
/// document with the fn's `Ok` value as `meta.result`, or `204`.
///
/// Checks run in §13.2 order: `Accept`, then for a route that reads a body
/// its media type, then the path, the query and the body. Axum runs `Body`
/// last, so a handler reading one takes the path and the query as `Result`s
/// and answers them after it.
///
/// The fn's arguments are bound under their own names and every other
/// binding carries the `ontogen_` prefix (see [`Access`]), so an argument
/// may be named `state`, `store`, `query`, `body`, `args`, `result`, `limit`
/// or anything else a handler needs.
#[allow(clippy::too_many_arguments)]
fn op_handler(
    out: &mut String,
    m: &ApiModule,
    f: &ApiFn,
    shape: &OpShape<'_>,
    handler_name: &str,
    access: Option<Access>,
    scope: Option<&RoutePrefix>,
    config: &Config,
) {
    let svc = m.service_ident(f.surface);
    let state_type = &config.state_type;
    let aw = await_str(f.is_async);
    let map_err = err_map(f, config);

    let mut names: Vec<String> = Vec::new();
    let mut types: Vec<String> = Vec::new();
    if let Some(prefix) = scope {
        names.push(SCOPE.to_string());
        types.push(prefix.params[0].rust_type.clone());
    }
    for p in &shape.path_args {
        names.push(p.name.clone());
        types.push(param_to_owned_type(&p.ty_ast));
    }
    let path = match names.len() {
        0 => None,
        1 => Some((names[0].clone(), types[0].clone())),
        _ => Some((format!("({})", names.join(", ")), format!("({})", types.join(", ")))),
    };

    let query_spec = if shape.page.is_some() {
        Some("PageOpArgs".to_string())
    } else if shape.query_args.is_empty() {
        None
    } else {
        let spec = format!("{}{}OpArgs", to_pascal_case(&m.name), to_pascal_case(&f.name));
        let declared: Vec<String> = shape.query_args.iter().map(|p| format!("\"{}\"", p.name)).collect();
        out.push_str(&format!(
            "struct {spec};\n\nimpl RouteQuery for {spec} {{\n    const SPEC: QuerySpec = QuerySpec {{ op_args: \
             &[{}], ..QuerySpec::NONE }};\n}}\n\n",
            declared.join(", ")
        ));
        Some(spec)
    };

    let mut extractors = String::new();
    if access.is_some() {
        extractors.push_str(&format!("    State(ontogen_state): State<Arc<{state_type}>>,\n"));
    }
    extractors.push_str("    _: AcceptGuard,\n");
    let mut steps = String::new();
    match &shape.body_args {
        Some(args) => {
            if let Some((pattern, ty)) = &path {
                extractors.push_str(&format!("    ontogen_path: Result<Path<{ty}>, ErrorObject>,\n"));
                steps.push_str(&format!("    let Path({pattern}) = ontogen_path?;\n"));
            }
            extractors.push_str("    ontogen_query: Result<Query<NoParams>, ErrorObject>,\n    ontogen_body: Body,\n");
            let has_required = args.iter().any(|p| !p.is_option());
            let declared: Vec<String> = args.iter().map(|p| format!("\"{}\"", p.name)).collect();
            steps.push_str(&format!(
                "    ontogen_query?;\n    let ontogen_bytes = ontogen_body.into_bytes()?;\n    let ontogen_args = \
                 request::op_args(&ontogen_bytes, {has_required})?;\n    \
                 request::check_op_arg_names(&ontogen_args, &[{}])?;\n",
                declared.join(", ")
            ));
            for p in args {
                steps.push_str(&format!(
                    "    let {name} = request::op_arg::<{ty}>(&ontogen_args, \"{name}\", {required})?;\n",
                    name = p.name,
                    ty = param_to_owned_type(&p.ty_ast),
                    required = !p.is_option(),
                ));
            }
        }
        None => {
            if let Some((pattern, ty)) = &path {
                extractors.push_str(&format!("    Path({pattern}): Path<{ty}>,\n"));
            }
            match &query_spec {
                Some(spec) => extractors.push_str(&format!("    ontogen_query: Query<{spec}>,\n")),
                None => extractors.push_str("    _: Query<NoParams>,\n"),
            }
            // `opArg[…]` in byte order of name (§13.2 step 5).
            if let Some(Paging::Store { default_limit, max_limit } | Paging::InMemory { default_limit, max_limit }) =
                shape.page
            {
                steps.push_str(&format!(
                    "    let ontogen_limit = \
                     ontogen_query.page_op_arg(\"limit\")?.unwrap_or({default_limit}).min({max_limit});\n    \
                     let ontogen_offset = ontogen_query.page_op_arg(\"offset\")?.unwrap_or(0);\n"
                ));
            }
            let mut by_name = shape.query_args.clone();
            by_name.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
            for p in by_name {
                let owned = param_to_owned_type(&p.ty_ast);
                let inner = owned.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')).unwrap_or(&owned);
                steps.push_str(&format!(
                    "    let {name} = ontogen_query.op_arg::<{inner}>(\"{name}\")?;\n",
                    name = p.name
                ));
            }
        }
    }

    let (open, first_arg) = match access {
        Some(Access { open, arg }) => (open, Some(arg)),
        None => (String::new(), None),
    };
    let args: Vec<String> = first_arg
        .map(str::to_string)
        .into_iter()
        .chain(f.params.iter().map(|p| match shape.page {
            Some(Paging::Store { .. }) => format!("Some(u64::from(ontogen_{}))", p.name),
            None if classify_op(f) == OpKind::List => "None".to_string(),
            _ => forward_arg_expr(&p.name, &p.ty_ast),
        }))
        .collect();
    let call = format!("{svc}::{}({}){aw}{map_err}?", f.name, args.join(", "));
    let respond = match shape.page {
        _ if shape.no_content => format!("    {call};\n    Ok(response::no_content())\n"),
        Some(Paging::Store { .. }) => {
            let count_arg = first_arg.unwrap_or_default();
            format!(
                "    let ontogen_items = {call};\n    let ontogen_total = {svc}::count({count_arg}){aw}{map_err}?;\n\
                 {PAGE_RESULT}{OK_RESULT}"
            )
        }
        Some(Paging::InMemory { .. }) => format!(
            "    let ontogen_all = {call};\n    let ontogen_total = ontogen_all.len() as u64;\n    let ontogen_items = \
             ontogen_all.into_iter().skip(ontogen_offset as usize).take(ontogen_limit as usize).collect();\n\
             {PAGE_RESULT}{OK_RESULT}"
        ),
        None => format!("    let ontogen_result = {call};\n{OK_RESULT}"),
    };
    out.push_str(&format!(
        "async fn {handler_name}(\n{extractors}) -> Result<Response, ErrorObject> {{\n{steps}{open}{respond}}}\n\n"
    ));
}

/// A custom op's success: its `Ok` value as `meta.result` (§10.1).
const OK_RESULT: &str = "    Ok(response::ok(&Document::meta_only(ResultMeta { result: ontogen_result })))\n";

/// A list's page as `meta.result`.
const PAGE_RESULT: &str = "    let ontogen_result = PaginatedResult {\n        items: ontogen_items,\n        total: \
                           ontogen_total,\n        limit: ontogen_limit,\n        offset: ontogen_offset,\n    };\n";

/// Shared SSE plumbing, emitted once when any module has events.
///
/// `sse_stream` turns an event fn's receiver into frames with
/// `ontogen_core::events::next_frame`: a lagged receiver becomes an
/// `event: lag` frame carrying `{"skipped":n}` and the stream stays open;
/// closed senders end it. Keep-alive comments let a dead client's stream (and
/// its receiver) drop before the next event. Each item's `data:` is written
/// by the handler's `FrameData` (§12): `result_frame`, or the entity's own
/// `{module}_frame_data`.
const SSE_HELPERS: &str = "\
/// Writes an event item into its frame's `data:`.
type FrameData<T> = fn(Event, &T) -> Result<Event, axum::Error>;

fn sse_event<T>(name: &'static str, frame: EventFrame<T>, data: FrameData<T>) -> Event {
    match frame {
        EventFrame::Event { id, data: item } => {
            let event = data(Event::default().event(name), &item)
                .unwrap_or_else(|e| Event::default().event(\"error\").data(e.to_string()));
            match id {
                Some(id) if !id.contains(['\\n', '\\r', '\\0']) => event.id(id),
                _ => event,
            }
        }
        EventFrame::Lag { skipped } => Event::default().event(\"lag\").data(format!(\"{{\\\"skipped\\\":{skipped}}}\")),
    }
}

fn sse_stream<T>(
    name: &'static str,
    rx: tokio::sync::broadcast::Receiver<T>,
    id: ontogen_core::events::IdFn<T>,
    data: FrameData<T>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>>
where
    T: Clone + Send + 'static,
{
    let stream = futures::stream::unfold(rx, move |mut rx| async move {
        ontogen_core::events::next_frame(&mut rx, id).await.map(|frame| (Ok(sse_event(name, frame, data)), rx))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

";

/// Map a path param's Rust type onto the type its `Path` extractor holds.
fn path_extract_type(ty: &str) -> &'static str {
    match ty {
        "i32" => "i32",
        "i64" => "i64",
        "u32" => "u32",
        "u64" => "u64",
        _ => "String",
    }
}

/// Generate one SSE handler and its route for an event fn.
///
/// Required params are path segments and optional params are query params,
/// as for a `CustomGet`. A resumable fn's `resume` comes from the
/// `Last-Event-ID` header, falling back to the `resume` query param (which a
/// client that reconnects itself sends). With `scoped`, the handler takes the
/// prefix param first and calls `state.subscribe_{fn}_for(&prefix, args…)`,
/// which has the event fn's own async/`Result` shape.
fn generate_sse_handler(
    out: &mut String,
    routes: &mut Routes,
    m: &ApiModule,
    ev: &EventFn,
    config: &Config,
    scoped: Option<&RoutePrefix>,
) {
    let fn_name = &ev.name;
    let ev_name = event_name(fn_name);
    let state_type = &config.state_type;
    let suffix = if scoped.is_some() { "_sse_scoped" } else { "_sse" };
    let handler_name = format!("{fn_name}{suffix}");
    let path_params = ev.path_params();
    let query_params = ev.query_params();
    let resumable = ev.is_resumable();
    let query_struct = format!("{}{}EventQuery", to_pascal_case(&m.name), to_pascal_case(fn_name));

    if scoped.is_none() {
        out.push_str(&format!("// ── {fn_name} SSE Handler ──\n\n"));
        if !query_params.is_empty() {
            out.push_str(&format!("#[derive(Deserialize)]\nstruct {query_struct} {{\n"));
            for qp in &query_params {
                out.push_str(&format!("    {}: {},\n", qp.name, param_to_owned_type(&qp.ty_ast)));
            }
            out.push_str("}\n\n");
        }
    }

    out.push_str(&format!("async fn {handler_name}(\n    State(ontogen_state): State<Arc<{state_type}>>,\n"));
    let mut path_names: Vec<String> = Vec::new();
    let mut path_types: Vec<String> = Vec::new();
    if let Some(prefix) = scoped {
        path_names.push(SCOPE.to_string());
        path_types.push(prefix.params[0].rust_type.clone());
    }
    for p in &path_params {
        path_names.push(p.name.clone());
        path_types.push(path_extract_type(&p.ty).to_string());
    }
    match path_names.len() {
        0 => {}
        1 => out.push_str(&format!("    Path({}): Path<{}>,\n", path_names[0], path_types[0])),
        _ => out.push_str(&format!("    Path(({})): Path<({})>,\n", path_names.join(", "), path_types.join(", "))),
    }
    if !query_params.is_empty() {
        out.push_str(&format!("    ontogen_query: Result<axum::extract::Query<{query_struct}>, QueryRejection>,\n"));
    }
    if resumable {
        out.push_str("    ontogen_headers: axum::http::HeaderMap,\n");
    }
    let sse_type = "Sse<impl futures::Stream<Item = Result<Event, Infallible>>>";
    let fallible = ev.returns_result || !query_params.is_empty();
    if fallible {
        out.push_str(&format!(") -> Result<{sse_type}, ErrorObject> {{\n"));
    } else {
        out.push_str(&format!(") -> {sse_type} {{\n"));
    }
    if !query_params.is_empty() {
        out.push_str("    let axum::extract::Query(ontogen_query) = ontogen_query.map_err(query_rejection)?;\n");
    }

    let mut args: Vec<String> = Vec::new();
    for p in &path_params {
        args.push(forward_arg_expr(&p.name, &p.ty_ast));
    }
    for qp in &query_params {
        if is_resume_param(qp) {
            out.push_str(
                "    let ontogen_resume = ontogen_core::events::last_event_id(\n        \
                 ontogen_headers.get(\"last-event-id\").map(|v| v.as_bytes()),\n    )\n    .or(ontogen_query.resume);\n",
            );
            args.push("ontogen_resume".to_string());
        } else {
            args.push(forward_arg_expr(&format!("ontogen_query.{}", qp.name), &qp.ty_ast));
        }
    }
    let call = match scoped {
        Some(_) => {
            let mut all = vec![format!("&{SCOPE}")];
            all.extend(args);
            format!("ontogen_state.subscribe_{fn_name}_for({})", all.join(", "))
        }
        None => {
            let svc = m.service_ident(ev.surface);
            let mut all = vec!["&ontogen_state".to_string()];
            all.extend(args);
            format!("{svc}::{fn_name}({})", all.join(", "))
        }
    };
    let await_str = if ev.is_async { ".await" } else { "" };
    let id_fn = if resumable { "ontogen_core::events::seq_id" } else { "ontogen_core::events::no_id" };
    let data_fn = match config.resources.by_item_type(&ev.item_type_ast) {
        Some(resource) => resource_names(&resource.module).frame,
        None => "result_frame".to_string(),
    };
    if ev.returns_result {
        let map_err = if event_returns_app_error(ev, config) { "app_error" } else { "internal_error" };
        out.push_str(&format!("    let ontogen_rx = {call}{await_str}.map_err({map_err})?;\n"));
    } else {
        out.push_str(&format!("    let ontogen_rx = {call}{await_str};\n"));
    }
    let stream = format!("sse_stream(\"{ev_name}\", ontogen_rx, {id_fn}, {data_fn})");
    if fallible {
        out.push_str(&format!("    Ok({stream})\n}}\n\n"));
    } else {
        out.push_str(&format!("    {stream}\n}}\n\n"));
    }

    let route_path = match scoped {
        Some(prefix) => ev.sse_route_scoped(&config.sse_route_overrides, &prefix.segments),
        None => ev.sse_route(&config.sse_route_overrides),
    };
    routes.add(&axum_path(&route_path), "get", &handler_name);
}
