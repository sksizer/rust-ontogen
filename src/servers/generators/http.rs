#![allow(clippy::too_many_lines, clippy::format_push_string)]

//! Generate Axum HTTP route handlers from API modules.
//!
//! A resource module's CRUD ops are served as JSON:API (wire contract §7,
//! §8) through the `ontogen-jsonapi` runtime crate. Custom ops, junction
//! ops, modules with no entity behind them and event streams keep their own
//! success shapes until phase 1c, but every error the server returns is an
//! `errors[]` document (§13).

use std::fs;
use std::path::Path;

use ontogen_core::ir::OpKind;

use crate::persistence::dto::{create_field_required, field_to_create_type};
use crate::resource::{Arity, Resource, member_name};
use crate::servers::classify::{classify_op, is_read_op};
use crate::servers::config::{Config, RoutePrefix};
use crate::servers::error_map::VariantShape;
use crate::servers::generators::surface_use_stmts;
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
            open: format!("    let store = state.{}().await.map_err(internal_error)?;\n", f.store_accessor),
            arg: "&store",
        }
    } else {
        Access { open: String::new(), arg: "&state" }
    }
}

/// A scoped handler's access through the prefix accessor. Its failure stays
/// a `500` until E0003 settles the accessor's error type (§11.1).
fn scoped_access(prefix: &RoutePrefix) -> Access {
    Access {
        open: format!(
            "    let store = state.{}(&{}).map_err(internal_error)?;\n",
            prefix.state_accessor, prefix.params[0].name
        ),
        arg: "&store",
    }
}

/// `.map_err(…)` for a call returning `f`'s error: `app_error` for the
/// consumer's `AppError`, `internal_error` for anything else (§13.4).
fn err_map(f: &ApiFn, config: &Config) -> &'static str {
    if returns_app_error(f, config) { ".map_err(app_error)" } else { ".map_err(internal_error)" }
}

/// True when `f` fails with the `AppError` that `app_error` takes: the one
/// in the primary surface's types module.
///
/// The error type is read as `f`'s module names it. A bare `AppError`, or a
/// path that ends `f`'s own surface's types path (`schema::AppError`), is
/// that surface's `AppError`; any other path is taken as written. So the
/// `AppError` of another surface's types module maps through
/// `internal_error`: `app_error` cannot take it.
fn returns_app_error(f: &ApiFn, config: &Config) -> bool {
    let Some(error) = f.error_type.as_deref() else { return false };
    let own = match f.surface {
        0 => app_error_path(config),
        i => format!("{}::AppError", config.extra_surfaces[i - 1].types_import_path),
    };
    let resolved = if own == error || own.ends_with(&format!("::{error}")) { own.as_str() } else { error };
    resolved == app_error_path(config)
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
    let mut out = String::new();

    out.push_str(
        "\
#![allow(dead_code, unused_imports, clippy::pedantic, clippy::needless_borrow)]
//! Auto-generated HTTP route handlers. DO NOT EDIT.
//!
//! Generated by ontogen from API source files.

use std::sync::Arc;

use axum::{
    extract::{
        State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{Method, StatusCode},
    response::{Json, Response},
    routing::{delete, get, patch, post, put},
    Router,
};
use ontogen_jsonapi::{
    Document, ErrorCode, ErrorObject, Linkage, Links, LookupKey, PageMeta, QueryParams, QuerySpec, Relationship,
    ResourceIdentifier, ResourceObject,
    error::method_not_allowed,
    extract::{AcceptGuard, Body, ContentTypeGuard, NoParams, Path, Query, RouteQuery},
    links::{CanonicalQuery, encode_path_segment, pagination_links},
    request::{self, Endpoint, LinkedId, ResourceData},
    response,
};
use serde::{Deserialize, Serialize};

",
    );

    // Emit use statements in sorted order (matches rustfmt alphabetical sort)
    let mut use_stmts = surface_use_stmts(modules, config);

    use_stmts.push(format!("use {};\n", config.state_import));

    if let Some(ref store_import) = config.store_import {
        use_stmts.push(format!("use {};\n", store_import));
    }

    use_stmts.sort();
    for stmt in &use_stmts {
        out.push_str(stmt);
    }

    // Add SSE-specific imports if any module has events
    let has_events = modules.iter().any(|m| !m.events.is_empty());
    if has_events {
        out.push_str(
            "\
use std::convert::Infallible;
use axum::response::sse::{Event, KeepAlive, Sse};
use ontogen_core::events::EventFrame;
",
        );
    }

    out.push('\n');

    emit_error_helpers(&mut out, config);
    out.push_str(ROUTE_HELPERS);

    if has_events {
        out.push_str(SSE_HELPERS);
    }

    // A list outside a resource, and a junction list, keep `PaginatedResult`.
    let legacy_pagination = modules.iter().any(|m| {
        m.functions.iter().any(|f| {
            let op = classify_op(f);
            let unserved_list = op == OpKind::List && served_resource(m, f, config).is_none();
            (unserved_list || matches!(op, OpKind::JunctionList { .. }))
                && config.pagination_for(&m.name, f.surface).is_some()
        })
    });
    if legacy_pagination {
        out.push_str(
            "\
#[derive(Serialize)]
pub struct PaginatedResult<T: Serialize> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Deserialize)]
pub struct PaginationParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

",
        );
    }

    if modules.iter().any(|m| m.functions.iter().any(|f| served_resource(m, f, config).is_some())) {
        out.push_str(RESOURCE_HELPERS);
        for m in modules {
            emit_resource_helpers(&mut out, m, config);
        }
    }

    // Generate handlers and collect routes.
    //
    // `junction_routes` uses BTreeMap so the emit order is the natural sort
    // of route paths, not HashMap iteration order. Iteration order matters
    // because the generated `http.rs` is written via `write_if_changed`:
    // if the bytes differ between cargo invocations, the file gets rewritten
    // even when no API surface changed — and on consumers running
    // `tauri dev`, that triggers an infinite rebuild loop (the file
    // watcher sees the new mtime, kicks off another `cargo run`, which
    // re-runs build.rs, which re-emits with a fresh per-process RandomState
    // seed, etc.).
    let mut routes = Routes::default();
    let mut junction_routes: std::collections::BTreeMap<String, Vec<(&'static str, String)>> =
        std::collections::BTreeMap::new();

    for m in modules {
        let module = &m.name;
        let base = format!("/api/{}", config.naming.url_for_module(m));

        // Store-scoped fns (first param is &Store) get scoped routes only
        // (generate_scoped_handlers below) when route_prefix is set. Otherwise
        // each store-scoped handler opens the store through its surface's
        // accessor. Decided per fn: a merged module may mix both kinds.
        let functions: Vec<&ApiFn> =
            m.functions.iter().filter(|f| !(f.first_param_is_store && config.route_prefix.is_some())).collect();
        if functions.is_empty() {
            continue;
        }

        out.push_str(&format!("// ── {} Handlers ──\n\n", capitalize(module)));

        for f in functions {
            let op = classify_op(f);
            let handler_name = crate::servers::generators::ipc::command_name(module, f, config);

            if let Some(resource) = served_resource(m, f, config) {
                let op = ResourceOp { m, f, resource, handler_name: &handler_name, modules, config };
                let (method, path) = resource_handler(&mut out, &op, unscoped_access(f), None);
                routes.add(&format!("{base}{path}"), method, &handler_name);
                continue;
            }

            match op {
                OpKind::List | OpKind::GetById | OpKind::Create | OpKind::Update | OpKind::Delete => {
                    let (method, path) = legacy_crud_handler(&mut out, m, f, &handler_name, config);
                    routes.add(&format!("{base}{path}"), method, &handler_name);
                }

                OpKind::JunctionList { .. } | OpKind::JunctionAdd { .. } | OpKind::JunctionRemove { .. } => {
                    let (method, path) = junction_handler(&mut out, m, f, &handler_name, config);
                    junction_routes.entry(format!("{base}{path}")).or_default().push((method, handler_name));
                }

                OpKind::CustomGet | OpKind::CustomPost => {
                    generate_generic_http_handler(&mut out, &mut routes, m, f, config);
                }

                OpKind::EventStream => continue,
            }
        }
    }

    for (path, methods) in &junction_routes {
        for (method, handler) in methods {
            routes.add(path, method, handler);
        }
    }

    // Generate SSE handlers for event functions
    for m in modules {
        for ev in &m.events {
            generate_sse_handler(&mut out, &mut routes, m, ev, config, None);
        }
    }

    // Generate project-scoped handler variants if route_prefix is configured
    if let Some(prefix) = &config.route_prefix {
        generate_scoped_handlers(&mut out, &mut routes, modules, config, prefix);
    }

    // Router function
    let state_type = &config.state_type;
    out.push_str(&format!(
        "/// Generated routes. Call this from your main router.\npub fn entity_routes() -> Router<Arc<{state_type}>> {{\n    Router::new()\n"
    ));
    out.push_str(&routes.render());
    out.push_str("}\n");

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("Failed to create output directory");
    }
    crate::write_and_format(output, out).expect("Failed to write HTTP generated file");
}

/// `app_error` maps the consumer's `AppError` (§13.4); `internal_error`
/// takes every failure no `AppError` describes.
fn emit_error_helpers(out: &mut String, config: &Config) {
    match &config.error_map {
        Some(map) => {
            let path = app_error_path(config);
            out.push_str(&format!(
                "/// An `AppError` as an error object: the status its variant's name gives,\n/// and the name in \
                 snake_case as the code (§13.4).\nfn app_error(e: {path}) -> ErrorObject {{\n    let (status, code) \
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
/// status of its own: every one is a `500` (§13.4).
fn app_error(e: impl std::fmt::Display) -> ErrorObject {
    ErrorObject::internal(e.to_string())
}

",
        ),
    }
    out.push_str(
        "\
/// A failure no `AppError` describes: opening the store, a scope accessor,
/// or an op with another error type (§13.3).
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

/// Emitted once: the `405` fallback every route installs (§13.5), and the
/// error documents for the Axum extractors the ops outside a resource still
/// use.
const ROUTE_HELPERS: &str = "\
/// The method fallback of a route serving `allowed` (§13.5).
fn allow<const N: usize>(
    allowed: [Method; N],
) -> impl Fn(Method) -> std::future::Ready<Response> + Clone + Send + Sync + 'static {
    move |method| std::future::ready(method_not_allowed(&method, &allowed))
}

fn json_rejection(e: JsonRejection) -> ErrorObject {
    match e.status() {
        StatusCode::UNSUPPORTED_MEDIA_TYPE => {
            ErrorObject::new(ErrorCode::UnsupportedMediaType, e.body_text()).with_header(\"Content-Type\")
        }
        StatusCode::PAYLOAD_TOO_LARGE => ErrorObject::new(ErrorCode::ContentTooLarge, e.body_text()),
        _ => ErrorObject::new(ErrorCode::InvalidDocument, e.body_text()),
    }
}

fn query_rejection(e: QueryRejection) -> ErrorObject {
    ErrorObject::new(ErrorCode::InvalidQueryParameter, e.body_text())
}

";

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

/// No list takes an `order` argument yet, so every `sort` asks for an order
/// the server does not support (§7.4).
fn refuse_sort(query: &QueryParams, type_name: &str) -> Result<(), ErrorObject> {
    match query.sort()? {
        None => Ok(()),
        Some(_) => Err(ErrorObject::new(ErrorCode::InvalidSortField, format!(\"`{type_name}` cannot be sorted\"))
            .with_parameter(\"sort\")),
    }
}

/// No route includes related resources yet, so every `include` names a path
/// the server cannot include (§7.5).
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

/// The effective `(offset, limit)` of a paginated list (§7.2).
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
}

fn resource_names(module: &str) -> ResourceNames {
    let pascal = to_pascal_case(module);
    ResourceNames {
        attributes: format!("{pascal}ResourceAttributes"),
        linked: format!("{pascal}LinkedIds"),
        resource: format!("{module}_as_resource"),
        fields: format!("{module}_request_fields"),
        key: format!("{module}_lookup_key"),
    }
}

/// Per resource, emitted once whichever handlers serve it: the attributes
/// serializer and resource builder (§5), the `{id}` lookup key (§8.1), and
/// the reader of create and update documents (§8.2, §8.3 step 7).
fn emit_resource_helpers(out: &mut String, m: &ApiModule, config: &Config) {
    let served: Vec<OpKind> =
        m.functions.iter().filter(|f| served_resource(m, f, config).is_some()).map(classify_op).collect();
    if served.is_empty() {
        return;
    }
    let Some(resource) = config.resources.by_module(&m.name) else { return };
    let names = resource_names(&m.name);
    let type_name = &resource.resource_type;
    let entity = &resource.entity.name;
    let id_field = &resource.id_field;

    out.push_str(&format!("// ── `{type_name}` ──\n\n"));

    if let Some(entity_ty) = entity_type(m) {
        let attrs = &names.attributes;
        out.push_str(&format!(
            "/// `{entity}`'s attributes: every field but the id and the relations, in\n/// declaration order \
             (§5.3).\nstruct {attrs}<'a>(&'a {entity_ty});\n\nimpl Serialize for {attrs}<'_> {{\n    fn \
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
            "/// A `{type_name}` resource object whose `links.self` sits under `collection`\n/// (§5.2).\nfn \
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
            out.push_str(&format!(
                "\n        .with_relationship(\"{}\", Relationship::from_data({linkage}))",
                rel.name
            ));
        }
        out.push_str("\n}\n\n");
    }

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
        "/// The id an `{{id}}` path segment names (§8.1).\nfn {}(id: &LookupKey) -> Result<&str, ErrorObject> {{\n    \
         id.as_str().ok_or_else(|| {missing})\n}}\n\n",
        names.key
    ));

    if !served.iter().any(|op| matches!(op, OpKind::Create | OpKind::Update)) {
        return;
    }

    let has_relationships = !resource.relationships.is_empty();
    if has_relationships {
        out.push_str(&format!(
            "/// The ids a `{type_name}` request document links, by relationship, for the\n/// linked-resource \
             checks (§13.2 step 8).\n#[derive(Default)]\nstruct {} {{\n",
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
        "/// Step 7 of a `{type_name}` create or update document (§8.2, §8.3): the fields\n/// it sets, named as \
         the input's fields.\nfn {}(data: &ResourceData, create: bool) -> Result<{returns}, ErrorObject> {{\n    let \
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
        return;
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
}

/// One CRUD op served as its resource.
#[derive(Clone, Copy)]
struct ResourceOp<'a> {
    m: &'a ApiModule,
    f: &'a ApiFn,
    resource: &'a Resource,
    handler_name: &'a str,
    modules: &'a [ApiModule],
    config: &'a Config,
}

/// The step-7 read of a create or update body, then its step-8 lookups:
/// every linked id, in declared relationship order, fetched with its
/// target's `get_by_id` (§13.2).
fn write_steps(op: &ResourceOp<'_>, scoped: bool) -> (String, String) {
    let fields_fn = resource_names(&op.m.name).fields;
    let create = classify_op(op.f) == OpKind::Create;
    if op.resource.relationships.is_empty() {
        return (format!("    let fields = {fields_fn}(&data, {create})?;\n"), String::new());
    }
    let mut checks = String::new();
    for rel in &op.resource.relationships {
        let Some((tm, tf)) = linked_lookup(op.modules, &rel.target_module) else { continue };
        let svc = tm.service_ident(tf.surface);
        let arg = if !tf.first_param_is_store {
            "&state".to_string()
        } else if scoped || (op.f.first_param_is_store && tf.store_accessor == op.f.store_accessor) {
            "&store".to_string()
        } else {
            format!("&state.{}().await.map_err(internal_error)?", tf.store_accessor)
        };
        let not_found = format!("{}NotFound", rel.target_entity);
        let not_found_arm = op
            .config
            .error_map
            .as_ref()
            .and_then(|map| map.variants.iter().find(|v| v.name == not_found))
            .filter(|_| returns_app_error(tf, op.config))
            .map(|v| {
                format!(
                    "            Err({}) => return Err(linked.not_found(\"{}\")),\n",
                    v.pattern(&app_error_path(op.config)),
                    rel.target_type
                )
            })
            .unwrap_or_default();
        let fallback = if returns_app_error(tf, op.config) { "app_error" } else { "internal_error" };
        let each = if rel.is_to_many() { "for linked in" } else { "if let Some(linked) =" };
        checks.push_str(&format!(
            "    {each} &linked.{} {{\n        match {svc}::get_by_id({arg}, &linked.id){} {{\n            \
             Ok(_) => {{}}\n{not_found_arm}            Err(e) => return Err({fallback}(e)),\n        }}\n    }}\n",
            rel.name,
            await_str(tf.is_async),
        ));
    }
    (format!("    let (fields, linked) = {fields_fn}(&data, {create})?;\n"), checks)
}

/// Emit the JSON:API handler of one CRUD op (§7, §8), returning its method
/// and its path below the collection.
///
/// Extractors run in §13.2 order: `Accept`, `Content-Type`, path, query,
/// body. With `scope`, the prefix parameter comes first in the path, and
/// links carry the prefix (§11.1).
fn resource_handler(
    out: &mut String,
    op: &ResourceOp<'_>,
    access: Access,
    scope: Option<&RoutePrefix>,
) -> (&'static str, &'static str) {
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

    let (collection, collection_path, item_path) = match scope {
        None => (
            format!("    let collection = \"/api/{url_plural}\";\n"),
            String::new(),
            "    Path(id): Path<LookupKey>,\n".to_string(),
        ),
        Some(prefix) => {
            let pp = &prefix.params[0];
            let mut args = Vec::new();
            let template: Vec<String> = prefix
                .segments
                .split('/')
                .map(|segment| match segment.strip_prefix(':') {
                    Some(name) => {
                        args.push(format!("encode_path_segment(&{name}.to_string())"));
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
                format!("    Path({}): Path<{}>,\n", pp.name, pp.rust_type),
                format!("    Path(({}, id)): Path<({}, LookupKey)>,\n", pp.name, pp.rust_type),
            )
        }
    };
    let head = format!("async fn {handler_name}(\n    State(state): State<Arc<{state_type}>>,\n    _: AcceptGuard,\n");
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
                "{head}{collection_path}    query: Query<{params}>,\n{tail}    refuse_sort(&query, \
                 \"{type_name}\")?;\n    refuse_include(&query, \"{type_name}\")?;\n{page}{open}{call}{collection}    \
                 let data: Vec<_> = items.iter().map(|entity| {as_resource}(entity, \
                 collection)).collect();\n{document}}}\n\n"
            ));
            ("get", "")
        }
        OpKind::GetById => {
            out.push_str(&format!(
                "{head}{item_path}    query: Query<GetParams>,\n{tail}    refuse_include(&query, \
                 \"{type_name}\")?;\n{open}    let entity = {svc}::get_by_id({arg}, \
                 {key}(&id)?){aw}{map_err}?;\n{collection}{single_response}"
            ));
            ("get", "/{id}")
        }
        OpKind::Create => {
            let input_ty = extract_input_type(&f.params[0].ty);
            let (fields, checks) = write_steps(op, scope.is_some());
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
            out.push_str(&format!(
                "{head}    _: ContentTypeGuard,\n{collection_path}    _: Query<NoParams>,\n    body: \
                 Body,\n{tail}{collection}    let endpoint = Endpoint {{ type_name: \"{type_name}\", path: collection \
                 }};\n    let data = request::parse_create(&body.0, endpoint, |id| {{\n        \
                 ontogen_core::id::validate_id(id).map_err(|e| e.reason)\n    }})?;\n{fields}    let input: \
                 {input_ty} = from_fields(fields)?;\n{open}{checks}    let entity = {svc}::create({arg}, \
                 input){aw}{create_err}?;\n    let resource = {as_resource}(&entity, collection);\n    let location \
                 = resource.links().self_link().to_owned();\n    let links = Links::new(location.as_str());\n    \
                 Ok(response::created(&location, &Document::new(resource, links)))\n}}\n\n"
            ));
            ("post", "")
        }
        OpKind::Update => {
            let input_ty = extract_input_type(&f.params[1].ty);
            let (fields, checks) = write_steps(op, scope.is_some());
            out.push_str(&format!(
                "{head}    _: ContentTypeGuard,\n{item_path}    _: Query<NoParams>,\n    body: \
                 Body,\n{tail}{collection}    let path = format!(\"{{collection}}/{{id}}\");\n    let endpoint = \
                 Endpoint {{ type_name: \"{type_name}\", path: &path }};\n    let data = \
                 request::parse_update(&body.0, endpoint, &id)?;\n{fields}    let input: {input_ty} = \
                 from_fields(fields)?;\n{open}{checks}    let entity = {svc}::update({arg}, {key}(&id)?, \
                 input){aw}{map_err}?;\n{single_response}"
            ));
            ("patch", "/{id}")
        }
        OpKind::Delete => {
            out.push_str(&format!(
                "{head}{item_path}    _: Query<NoParams>,\n{tail}{open}    {svc}::delete({arg}, \
                 {key}(&id)?){aw}{map_err}?;\n    Ok(response::no_content())\n}}\n\n"
            ));
            ("delete", "/{id}")
        }
        _ => unreachable!("served_resource picks CRUD ops only"),
    }
}

/// Emit an op named like a CRUD op that is not served as a resource: one in
/// a module with no entity behind it, or a list that takes a filter.
/// Returns its method and its path below the collection.
fn legacy_crud_handler(
    out: &mut String,
    m: &ApiModule,
    f: &ApiFn,
    handler_name: &str,
    config: &Config,
) -> (&'static str, &'static str) {
    let svc = m.service_ident(f.surface);
    let fn_name = &f.name;
    let ret_type = &f.return_type;
    let state_type = &config.state_type;
    let await_str = if f.is_async { "\n        .await" } else { "" };
    let err_map = err_map(f, config);
    let Access { open: store_let, arg: first_arg } = unscoped_access(f);

    match classify_op(f) {
        OpKind::List => {
            let pagination = config.pagination_for(&m.name, f.surface);
            let paginated = pagination.is_some() && ret_type.starts_with("Vec<");
            // Check for a query parameter struct (e.g., ListAgentsQuery)
            let query_param = f.params.iter().find(|p| p.ty.contains("Query"));
            // Check for plain string params (e.g., skill_id: &str) - scoped list filters.
            // A list that takes the page owns its limit/offset: they are never filters.
            let plain_params: Vec<_> = f
                .params
                .iter()
                .filter(|p| {
                    !p.ty.contains("Query") && !p.ty.contains("Input") && (!f.takes_page() || !is_page_param(p))
                })
                .collect();

            let mut extra_extractors = String::new();
            let mut unwraps = String::new();
            let mut extra_args = String::new();
            if let Some(qp) = query_param {
                let qt = extract_input_type(&qp.ty);
                extra_extractors.push_str(&format!("\n    query: Result<axum::extract::Query<{qt}>, QueryRejection>,"));
                unwraps.push_str("    let axum::extract::Query(query) = query.map_err(query_rejection)?;\n");
                extra_args.push_str(", query");
            }
            for pp in &plain_params {
                let name = &pp.name;
                extra_extractors
                    .push_str(&format!("\n    {name}: Result<axum::extract::Query<String>, QueryRejection>,"));
                unwraps
                    .push_str(&format!("    let axum::extract::Query({name}) = {name}.map_err(query_rejection)?;\n"));
                extra_args.push_str(&format!(", &{name}"));
            }

            if let Some(pg) = pagination
                && paginated
            {
                let item_type = inner_type(ret_type);
                let default_limit = pg.default_limit;
                let max_limit = pg.max_limit;
                // A filtered page calls `count` with the same filter,
                // after `list` has consumed it, so the by-value filter is
                // cloned into the list call and the original goes to
                // count. With no filter both are `extra_args`.
                let count_args = extra_args.clone();
                let list_args = extra_args.replace(", query", ", query.clone()");
                extra_extractors
                    .push_str("\n    pagination: Result<axum::extract::Query<PaginationParams>, QueryRejection>,");
                unwraps.push_str("    let axum::extract::Query(pagination) = pagination.map_err(query_rejection)?;\n");
                out.push_str(&format!(
                    "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,{extra_extractors}
) -> Result<Json<PaginatedResult<{item_type}>>, ErrorObject> {{
{unwraps}{store_let}    let limit = pagination.limit.unwrap_or({default_limit}).min({max_limit});
    let offset = pagination.offset.unwrap_or(0);
    let items = {svc}::list({first_arg}{list_args}, Some(u64::from(limit)), Some(u64::from(offset))){await_str}
        {err_map}?;
    let total = {svc}::count({first_arg}{count_args}){await_str}
        {err_map}?;
    Ok(Json(PaginatedResult {{ items, total, limit, offset }}))
}}

"
                ));
            } else {
                // This surface does not paginate: a list that takes the page gets the whole table.
                let page_args = if f.takes_page() { ", None, None" } else { "" };
                out.push_str(&format!(
                    "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,{extra_extractors}
) -> Result<Json<{ret_type}>, ErrorObject> {{
{unwraps}{store_let}    {svc}::list({first_arg}{extra_args}{page_args}){await_str}
        .map(Json)
        {err_map}
}}

"
                ));
            }
            ("get", "")
        }

        OpKind::GetById => {
            out.push_str(&format!(
                "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(id): Path<String>,
) -> Result<Json<{ret_type}>, ErrorObject> {{
{store_let}    {svc}::{fn_name}({first_arg}, &id){await_str}
        .map(Json)
        {err_map}
}}

"
            ));
            ("get", "/{id}")
        }

        OpKind::Create => {
            let input_type = extract_input_type(&f.params[0].ty);
            out.push_str(&format!(
                "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    input: Result<Json<{input_type}>, JsonRejection>,
) -> Result<(StatusCode, Json<{ret_type}>), ErrorObject> {{
    let Json(input) = input.map_err(json_rejection)?;
{store_let}    {svc}::create({first_arg}, input){await_str}
        .map(|entity| (StatusCode::CREATED, Json(entity)))
        {err_map}
}}

"
            ));
            ("post", "")
        }

        OpKind::Update => {
            let input_type = extract_input_type(&f.params[1].ty);
            out.push_str(&format!(
                "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(id): Path<String>,
    input: Result<Json<{input_type}>, JsonRejection>,
) -> Result<Json<{ret_type}>, ErrorObject> {{
    let Json(input) = input.map_err(json_rejection)?;
{store_let}    {svc}::update({first_arg}, &id, input){await_str}
        .map(Json)
        {err_map}
}}

"
            ));
            ("put", "/{id}")
        }

        OpKind::Delete => {
            out.push_str(&format!(
                "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ErrorObject> {{
{store_let}    {svc}::delete({first_arg}, &id){await_str}
        .map(|_| StatusCode::NO_CONTENT)
        {err_map}
}}

"
            ));
            ("delete", "/{id}")
        }

        _ => unreachable!("legacy_crud_handler takes CRUD ops only"),
    }
}

/// Emit a junction op's handler, returning its method and its path below
/// the collection.
fn junction_handler(
    out: &mut String,
    m: &ApiModule,
    f: &ApiFn,
    handler_name: &str,
    config: &Config,
) -> (&'static str, String) {
    let svc = m.service_ident(f.surface);
    let fn_name = &f.name;
    let ret_type = &f.return_type;
    let state_type = &config.state_type;
    let await_str = if f.is_async { "\n        .await" } else { "" };
    let err_map = err_map(f, config);
    let Access { open: store_let, arg: first_arg } = unscoped_access(f);
    let pagination = config.pagination_for(&m.name, f.surface);

    match classify_op(f) {
        OpKind::JunctionList { child_segment } => {
            if let Some(pg) = pagination
                && ret_type.starts_with("Vec<")
            {
                let item_type = inner_type(ret_type);
                let default_limit = pg.default_limit;
                let max_limit = pg.max_limit;
                out.push_str(&format!(
                    "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(parent_id): Path<String>,
    pagination: Result<axum::extract::Query<PaginationParams>, QueryRejection>,
) -> Result<Json<PaginatedResult<{item_type}>>, ErrorObject> {{
    let axum::extract::Query(pagination) = pagination.map_err(query_rejection)?;
{store_let}    let all_items = {svc}::{fn_name}({first_arg}, &parent_id){await_str}
        {err_map}?;
    let total = all_items.len() as u64;
    let limit = pagination.limit.unwrap_or({default_limit}).min({max_limit});
    let offset = pagination.offset.unwrap_or(0);
    let items = all_items.into_iter().skip(offset as usize).take(limit as usize).collect();
    Ok(Json(PaginatedResult {{ items, total, limit, offset }}))
}}

"
                ));
            } else {
                out.push_str(&format!(
                    "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(parent_id): Path<String>,
) -> Result<Json<{ret_type}>, ErrorObject> {{
{store_let}    {svc}::{fn_name}({first_arg}, &parent_id){await_str}
        .map(Json)
        {err_map}
}}

"
                ));
            }
            ("get", format!("/{{parent_id}}/{child_segment}"))
        }

        OpKind::JunctionAdd { child_segment } => {
            let child_id_param = if f.params.len() >= 2 { &f.params[1].name } else { "child_id" };
            // A missing child id is the client's mistake: `400`, not `500`
            // (E0003 phase 1).
            out.push_str(&format!(
                "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(parent_id): Path<String>,
    body: Result<Json<std::collections::HashMap<String, String>>, JsonRejection>,
) -> Result<StatusCode, ErrorObject> {{
    let Json(body) = body.map_err(json_rejection)?;
    let child_id = body
        .get(\"{child_id_param}\")
        .ok_or_else(|| ErrorObject::new(ErrorCode::InvalidDocument, \"`{child_id_param}` is required\"))?;
{store_let}    {svc}::{fn_name}({first_arg}, &parent_id, child_id){await_str}
        {err_map}?;
    Ok(StatusCode::NO_CONTENT)
}}

"
            ));
            ("post", format!("/{{parent_id}}/{child_segment}"))
        }

        OpKind::JunctionRemove { child_segment } => {
            out.push_str(&format!(
                "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path((parent_id, child_id)): Path<(String, String)>,
) -> Result<StatusCode, ErrorObject> {{
{store_let}    {svc}::{fn_name}({first_arg}, &parent_id, &child_id){await_str}
        {err_map}?;
    Ok(StatusCode::NO_CONTENT)
}}

"
            ));
            ("delete", format!("/{{parent_id}}/{child_segment}/{{child_id}}"))
        }

        _ => unreachable!("junction_handler takes junction ops only"),
    }
}

/// Shared SSE plumbing, emitted once when any module has events.
///
/// `sse_stream` turns an event fn's receiver into frames with
/// `ontogen_core::events::next_frame`: a lagged receiver becomes an
/// `event: lag` frame carrying `{"skipped":n}` and the stream stays open;
/// closed senders end it. Keep-alive comments let a dead client's stream (and
/// its receiver) drop before the next event.
const SSE_HELPERS: &str = "\
fn sse_event<T: Serialize>(name: &'static str, frame: EventFrame<T>) -> Event {
    match frame {
        EventFrame::Event { id, data } => {
            let event = Event::default()
                .event(name)
                .json_data(&data)
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
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>>
where
    T: Clone + Serialize + Send + 'static,
{
    let stream = futures::stream::unfold(rx, move |mut rx| async move {
        ontogen_core::events::next_frame(&mut rx, id).await.map(|frame| (Ok(sse_event(name, frame)), rx))
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

    out.push_str(&format!("async fn {handler_name}(\n    State(state): State<Arc<{state_type}>>,\n"));
    let mut path_names: Vec<String> = Vec::new();
    let mut path_types: Vec<String> = Vec::new();
    if let Some(prefix) = scoped {
        let pp = &prefix.params[0];
        path_names.push(pp.name.clone());
        path_types.push(pp.rust_type.clone());
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
        out.push_str(&format!("    q: Result<axum::extract::Query<{query_struct}>, QueryRejection>,\n"));
    }
    if resumable {
        out.push_str("    headers: axum::http::HeaderMap,\n");
    }
    let sse_type = "Sse<impl futures::Stream<Item = Result<Event, Infallible>>>";
    let fallible = ev.returns_result || !query_params.is_empty();
    if fallible {
        out.push_str(&format!(") -> Result<{sse_type}, ErrorObject> {{\n"));
    } else {
        out.push_str(&format!(") -> {sse_type} {{\n"));
    }
    if !query_params.is_empty() {
        out.push_str("    let axum::extract::Query(q) = q.map_err(query_rejection)?;\n");
    }

    let mut args: Vec<String> = Vec::new();
    for p in &path_params {
        args.push(forward_arg_expr(&p.name, &p.ty_ast));
    }
    for qp in &query_params {
        if is_resume_param(qp) {
            out.push_str(
                "    let resume = ontogen_core::events::last_event_id(\n        \
                 headers.get(\"last-event-id\").map(|v| v.as_bytes()),\n    )\n    .or(q.resume);\n",
            );
            args.push("resume".to_string());
        } else {
            args.push(forward_arg_expr(&format!("q.{}", qp.name), &qp.ty_ast));
        }
    }
    let call = match scoped {
        Some(prefix) => {
            let pp_name = &prefix.params[0].name;
            let mut all = vec![format!("&{pp_name}")];
            all.extend(args);
            format!("state.subscribe_{fn_name}_for({})", all.join(", "))
        }
        None => {
            let svc = m.service_ident(ev.surface);
            let mut all = vec!["&state".to_string()];
            all.extend(args);
            format!("{svc}::{fn_name}({})", all.join(", "))
        }
    };
    let await_str = if ev.is_async { ".await" } else { "" };
    let id_fn = if resumable { "ontogen_core::events::seq_id" } else { "ontogen_core::events::no_id" };
    if ev.returns_result {
        out.push_str(&format!("    let rx = {call}{await_str}.map_err(internal_error)?;\n"));
    } else {
        out.push_str(&format!("    let rx = {call}{await_str};\n"));
    }
    if fallible {
        out.push_str(&format!("    Ok(sse_stream(\"{ev_name}\", rx, {id_fn}))\n}}\n\n"));
    } else {
        out.push_str(&format!("    sse_stream(\"{ev_name}\", rx, {id_fn})\n}}\n\n"));
    }

    let route_path = match scoped {
        Some(prefix) => ev.sse_route_scoped(&config.sse_route_overrides, &prefix.segments),
        None => ev.sse_route(&config.sse_route_overrides),
    };
    routes.add(&axum_path(&route_path), "get", &handler_name);
}

/// A custom op's parameters by where they travel.
struct CustomParams<'a> {
    body_struct: Option<&'a Param>,
    query: Vec<&'a Param>,
    path: Vec<&'a Param>,
    body_fields: Vec<&'a Param>,
}

impl<'a> CustomParams<'a> {
    fn of(f: &'a ApiFn) -> Self {
        let is_get = is_read_op(&classify_op(f));
        let body_struct = f.params.iter().find(|p| p.ty.contains("Input"));
        let required = || f.params.iter().filter(|p| !p.ty.starts_with("Option<") && !p.ty.contains("Input"));
        Self {
            body_struct,
            query: f.params.iter().filter(|p| p.ty.starts_with("Option<")).collect(),
            path: if is_get { required().collect() } else { vec![] },
            body_fields: if !is_get && body_struct.is_none() { required().collect() } else { vec![] },
        }
    }

    /// The query and body structs a custom op deserializes into.
    fn emit_structs(&self, out: &mut String, module: &str, fn_name: &str) {
        if !self.query.is_empty() {
            let struct_name = format!("{}{}Query", to_pascal_case(module), to_pascal_case(fn_name));
            out.push_str(&format!("#[derive(Deserialize)]\nstruct {} {{\n", struct_name));
            for qp in &self.query {
                out.push_str(&format!("    {}: {},\n", qp.name, param_to_owned_type(&qp.ty_ast)));
            }
            out.push_str("}\n\n");
        }
        if !self.body_fields.is_empty() {
            let struct_name = format!("{}{}Body", to_pascal_case(module), to_pascal_case(fn_name));
            out.push_str(&format!("#[derive(Deserialize)]\nstruct {} {{\n", struct_name));
            for bf in &self.body_fields {
                out.push_str(&format!("    {}: {},\n", bf.name, param_to_owned_type(&bf.ty_ast)));
            }
            out.push_str("}\n\n");
        }
    }

    /// The query and body extractors, Axum's own until phase 1c gives
    /// custom ops their documents, with their rejections turned into error
    /// documents. Returns the lines that unwrap them.
    fn emit_extractors(&self, out: &mut String, module: &str, fn_name: &str) -> String {
        let mut unwraps = String::new();
        if !self.query.is_empty() {
            let struct_name = format!("{}{}Query", to_pascal_case(module), to_pascal_case(fn_name));
            out.push_str(&format!("    q: Result<axum::extract::Query<{struct_name}>, QueryRejection>,\n"));
            unwraps.push_str("    let axum::extract::Query(q) = q.map_err(query_rejection)?;\n");
        }
        if let Some(bs) = self.body_struct {
            let input_type = extract_input_type(&bs.ty);
            out.push_str(&format!("    input: Result<Json<{input_type}>, JsonRejection>,\n"));
            unwraps.push_str("    let Json(input) = input.map_err(json_rejection)?;\n");
        }
        if !self.body_fields.is_empty() {
            let struct_name = format!("{}{}Body", to_pascal_case(module), to_pascal_case(fn_name));
            out.push_str(&format!("    body: Result<Json<{struct_name}>, JsonRejection>,\n"));
            unwraps.push_str("    let Json(body) = body.map_err(json_rejection)?;\n");
        }
        unwraps
    }

    /// The call, its result mapping and the end of the handler.
    fn emit_call(&self, out: &mut String, svc: &str, f: &ApiFn, first_arg: Option<&str>, config: &Config) {
        let mut args: Vec<String> = first_arg.map(str::to_string).into_iter().collect();
        args.extend(self.path.iter().map(|p| forward_arg_expr(&p.name, &p.ty_ast)));
        args.extend(self.query.iter().map(|qp| forward_arg_expr(&format!("q.{}", qp.name), &qp.ty_ast)));
        if self.body_struct.is_some() {
            args.push("input".to_string());
        }
        args.extend(self.body_fields.iter().map(|bf| forward_arg_expr(&format!("body.{}", bf.name), &bf.ty_ast)));
        let await_str = if f.is_async { "\n        .await" } else { "" };
        out.push_str(&format!("    {}::{}({}){await_str}\n", svc, f.name, args.join(", ")));
        if f.return_type == "()" {
            out.push_str("        .map(|_| StatusCode::NO_CONTENT)\n");
        } else {
            out.push_str("        .map(Json)\n");
        }
        out.push_str(&format!("        {}\n}}\n\n", err_map(f, config)));
    }

    fn return_type(f: &ApiFn) -> String {
        if f.return_type == "()" {
            ") -> Result<StatusCode, ErrorObject> {\n".to_string()
        } else {
            format!(") -> Result<Json<{}>, ErrorObject> {{\n", f.return_type)
        }
    }
}

fn generate_generic_http_handler(
    out: &mut String,
    routes: &mut Routes,
    module: &ApiModule,
    f: &ApiFn,
    config: &Config,
) {
    let fn_name = &f.name;
    let name = module.name.as_str();
    let svc = module.service_ident(f.surface);
    let is_get = is_read_op(&classify_op(f));
    let action = config.naming.derive_action(name, fn_name);
    let url_plural = config.naming.url_for_module(module);
    let state_type = &config.state_type;
    let params = CustomParams::of(f);

    // Build route path
    let mut route_path = format!("/api/{}", url_plural);
    if !action.is_empty() {
        route_path.push_str(&format!("/{}", action));
    }
    for p in &params.path {
        route_path.push_str(&format!("/{{{}}}", p.name));
    }

    let handler_name = crate::servers::generators::ipc::command_name(name, f, config);
    let method = if is_get { "get" } else { "post" };

    params.emit_structs(out, name, fn_name);

    // Generate handler function. Stateless handlers omit the `State<...>`
    // extractor entirely; the rest of the signature is identical.
    out.push_str(&format!("async fn {}(\n", handler_name));
    if !f.is_stateless {
        out.push_str(&format!("    State(state): State<Arc<{state_type}>>,\n"));
    }

    // Path params
    if params.path.len() == 1 {
        let p = params.path[0];
        out.push_str(&format!("    Path({}): Path<{}>,\n", p.name, path_extract_type(&p.ty)));
    } else if params.path.len() > 1 {
        let types: Vec<&str> = params.path.iter().map(|p| path_extract_type(&p.ty)).collect();
        let names: Vec<&str> = params.path.iter().map(|p| p.name.as_str()).collect();
        out.push_str(&format!("    Path(({}),): Path<({},)>,\n", names.join(", "), types.join(", ")));
    }

    let unwraps = params.emit_extractors(out, name, fn_name);
    out.push_str(&CustomParams::return_type(f));
    out.push_str(&unwraps);

    // Store construction for store-based functions without route_prefix.
    // The constructed `store` is owned; service fns take `&Store`, so borrow it.
    // Stateless handlers pass no state/store and skip construction entirely.
    let first_arg: Option<&str> = if f.is_stateless {
        None
    } else if f.first_param_is_store && config.route_prefix.is_none() {
        let access = unscoped_access(f);
        out.push_str(&access.open);
        Some(access.arg)
    } else {
        Some("&state")
    };

    params.emit_call(out, &svc, f, first_arg, config);
    routes.add(&route_path, method, &handler_name);
}

/// Generate project-scoped handler variants and routes.
///
/// Only generates handlers for store-based modules (functions whose first
/// parameter is `&Store`). These handlers construct a Store via the prefix
/// accessor (e.g., `state.store_for(&project_id)`) and pass it to the
/// service function, providing real project data isolation.
fn generate_scoped_handlers(
    out: &mut String,
    routes: &mut Routes,
    modules: &[ApiModule],
    config: &Config,
    prefix: &RoutePrefix,
) {
    let state_type = &config.state_type;
    let pp = &prefix.params[0];
    let pp_name = &pp.name;
    let pp_type = &pp.rust_type;
    let store_let = scoped_access(prefix).open;

    out.push_str("\n// ── Project-Scoped Handlers ──\n\n");

    for m in modules {
        let module = &m.name;
        let plural = config.naming.module_plural(module);
        let url_plural = config.naming.url_for_module(m);
        let url_sing = config.naming.url_singular(module);
        let scoped_base = format!("/api/{}/{}", axum_path(&prefix.segments), url_plural);

        // Only store-scoped fns get scoped handlers; state-scoped fns of the
        // same module keep their unscoped routes.
        for f in m.functions.iter().filter(|f| f.first_param_is_store) {
            let op = classify_op(f);
            let svc = m.service_ident(f.surface);
            let fn_name = &f.name;
            let ret_type = &f.return_type;
            let pagination = config.pagination_for(module, f.surface);
            let await_str = if f.is_async { "\n        .await" } else { "" };
            let err_map = err_map(f, config);
            let handler_name = match op {
                OpKind::List => format!("list_{plural}_scoped"),
                OpKind::GetById => format!("get_{url_sing}_by_id_scoped"),
                OpKind::Create => format!("create_{url_sing}_handler_scoped"),
                OpKind::Update => format!("update_{url_sing}_handler_scoped"),
                OpKind::Delete => format!("delete_{url_sing}_handler_scoped"),
                _ => String::new(),
            };

            if let Some(resource) = served_resource(m, f, config) {
                let op = ResourceOp { m, f, resource, handler_name: &handler_name, modules, config };
                let (method, path) = resource_handler(out, &op, scoped_access(prefix), Some(prefix));
                routes.add(&format!("{scoped_base}{path}"), method, &handler_name);
                continue;
            }

            match op {
                OpKind::List => {
                    let query_param = f.params.iter().find(|p| p.ty.contains("Query"));
                    let mut extra_extractors = String::new();
                    let mut unwraps = String::new();
                    let query_arg = if let Some(qp) = query_param {
                        let qt = extract_input_type(&qp.ty);
                        extra_extractors
                            .push_str(&format!("\n    query: Result<axum::extract::Query<{qt}>, QueryRejection>,"));
                        unwraps.push_str("    let axum::extract::Query(query) = query.map_err(query_rejection)?;\n");
                        ", query".to_string()
                    } else {
                        String::new()
                    };
                    let page_args = if f.takes_page() { ", None, None" } else { "" };

                    if let Some(pg) = pagination
                        && ret_type.starts_with("Vec<")
                    {
                        let item_type = inner_type(ret_type);
                        let default_limit = pg.default_limit;
                        let max_limit = pg.max_limit;
                        extra_extractors.push_str(
                            "\n    pagination: Result<axum::extract::Query<PaginationParams>, QueryRejection>,",
                        );
                        unwraps.push_str(
                            "    let axum::extract::Query(pagination) = pagination.map_err(query_rejection)?;\n",
                        );
                        out.push_str(&format!(
                            "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path({pp_name}): Path<{pp_type}>,{extra_extractors}
) -> Result<Json<PaginatedResult<{item_type}>>, ErrorObject> {{
{unwraps}{store_let}    let all_items = {svc}::list(&store{query_arg}{page_args}){await_str}
        {err_map}?;
    let total = all_items.len() as u64;
    let limit = pagination.limit.unwrap_or({default_limit}).min({max_limit});
    let offset = pagination.offset.unwrap_or(0);
    let items = all_items.into_iter().skip(offset as usize).take(limit as usize).collect();
    Ok(Json(PaginatedResult {{ items, total, limit, offset }}))
}}

"
                        ));
                    } else {
                        out.push_str(&format!(
                            "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path({pp_name}): Path<{pp_type}>,{extra_extractors}
) -> Result<Json<{ret_type}>, ErrorObject> {{
{unwraps}{store_let}    {svc}::list(&store{query_arg}{page_args}){await_str}
        .map(Json)
        {err_map}
}}

"
                        ));
                    }
                    routes.add(&scoped_base, "get", &handler_name);
                }

                OpKind::GetById => {
                    out.push_str(&format!(
                        "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(({pp_name}, id)): Path<({pp_type}, String)>,
) -> Result<Json<{ret_type}>, ErrorObject> {{
{store_let}    {svc}::{fn_name}(&store, &id){await_str}
        .map(Json)
        {err_map}
}}

"
                    ));
                    routes.add(&format!("{scoped_base}/{{id}}"), "get", &handler_name);
                }

                OpKind::Create => {
                    let input_type = extract_input_type(&f.params[0].ty);
                    out.push_str(&format!(
                        "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path({pp_name}): Path<{pp_type}>,
    input: Result<Json<{input_type}>, JsonRejection>,
) -> Result<(StatusCode, Json<{ret_type}>), ErrorObject> {{
    let Json(input) = input.map_err(json_rejection)?;
{store_let}    {svc}::create(&store, input){await_str}
        .map(|entity| (StatusCode::CREATED, Json(entity)))
        {err_map}
}}

"
                    ));
                    routes.add(&scoped_base, "post", &handler_name);
                }

                OpKind::Update => {
                    let input_type = extract_input_type(&f.params[1].ty);
                    out.push_str(&format!(
                        "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(({pp_name}, id)): Path<({pp_type}, String)>,
    input: Result<Json<{input_type}>, JsonRejection>,
) -> Result<Json<{ret_type}>, ErrorObject> {{
    let Json(input) = input.map_err(json_rejection)?;
{store_let}    {svc}::update(&store, &id, input){await_str}
        .map(Json)
        {err_map}
}}

"
                    ));
                    routes.add(&format!("{scoped_base}/{{id}}"), "put", &handler_name);
                }

                OpKind::Delete => {
                    out.push_str(&format!(
                        "\
async fn {handler_name}(
    State(state): State<Arc<{state_type}>>,
    Path(({pp_name}, id)): Path<({pp_type}, String)>,
) -> Result<StatusCode, ErrorObject> {{
{store_let}    {svc}::delete(&store, &id){await_str}
        .map(|_| StatusCode::NO_CONTENT)
        {err_map}
}}

"
                    ));
                    routes.add(&format!("{scoped_base}/{{id}}"), "delete", &handler_name);
                }

                // Scoped junction ops are action-style routes until phase 3a
                // gives them the `{parent_id}/{child}` form.
                OpKind::JunctionList { .. }
                | OpKind::JunctionAdd { .. }
                | OpKind::JunctionRemove { .. }
                | OpKind::CustomGet
                | OpKind::CustomPost => {
                    generate_generic_http_handler_scoped(out, routes, m, f, config, prefix);
                }

                OpKind::EventStream => continue,
            }
        }
    }

    // Scoped SSE handlers
    for m in modules {
        for ev in &m.events {
            generate_sse_handler(out, routes, m, ev, config, Some(prefix));
        }
    }
}

/// Generate a scoped variant of a custom HTTP handler.
fn generate_generic_http_handler_scoped(
    out: &mut String,
    routes: &mut Routes,
    module: &ApiModule,
    f: &ApiFn,
    config: &Config,
    prefix: &RoutePrefix,
) {
    let fn_name = &f.name;
    let name = module.name.as_str();
    let svc = module.service_ident(f.surface);
    let is_get = is_read_op(&classify_op(f));
    let action = config.naming.derive_action(name, fn_name);
    let url_plural = config.naming.url_for_module(module);
    let state_type = &config.state_type;
    let accessor = &prefix.state_accessor;
    let pp = &prefix.params[0];
    let pp_name = &pp.name;
    let pp_type = &pp.rust_type;
    let params = CustomParams::of(f);

    // Build scoped route path
    let mut route_path = format!("/api/{}/{}", axum_path(&prefix.segments), url_plural);
    if !action.is_empty() {
        route_path.push_str(&format!("/{}", action));
    }
    for p in &params.path {
        route_path.push_str(&format!("/{{{}}}", p.name));
    }

    let handler_name = format!("{}_scoped", fn_name);
    let method = if is_get { "get" } else { "post" };

    // Query/body struct definitions (for store-based modules these are not
    // generated by the unscoped handler since it's skipped)
    params.emit_structs(out, name, fn_name);

    // Generate handler function. Stateless handlers omit the `State<...>`
    // extractor — the prefix path parameter still threads through so the
    // route shape is preserved.
    out.push_str(&format!("async fn {}(\n", handler_name));
    if !f.is_stateless {
        out.push_str(&format!("    State(state): State<Arc<{state_type}>>,\n"));
    }

    // Path params (prefix param + any entity path params)
    if params.path.is_empty() {
        out.push_str(&format!("    Path({pp_name}): Path<{pp_type}>,\n"));
    } else if params.path.len() == 1 {
        let p = params.path[0];
        let path_type = if p.ty == "i32" { "i32" } else { "String" };
        out.push_str(&format!("    Path(({pp_name}, {})): Path<({pp_type}, {})>,\n", p.name, path_type));
    } else {
        let mut names = vec![pp_name.clone()];
        let mut types = vec![pp_type.clone()];
        for p in &params.path {
            names.push(p.name.clone());
            types.push(if p.ty == "i32" { "i32".to_string() } else { "String".to_string() });
        }
        out.push_str(&format!("    Path(({}),): Path<({},)>,\n", names.join(", "), types.join(", ")));
    }

    let unwraps = params.emit_extractors(out, name, fn_name);
    out.push_str(&CustomParams::return_type(f));
    out.push_str(&unwraps);

    // Construct Store / validate prefix for state-bearing functions.
    // Stateless handlers skip the prefix-validation step entirely: they
    // never read state, so there is nothing to validate via the accessor.
    let first_arg: Option<&str> = if f.is_stateless {
        None
    } else if f.first_param_is_store {
        out.push_str(&scoped_access(prefix).open);
        Some("&store")
    } else {
        out.push_str(&format!("    state.{accessor}(&{pp_name}).map_err(internal_error)?;\n"));
        Some("&state")
    };

    params.emit_call(out, &svc, f, first_arg, config);
    routes.add(&route_path, method, &handler_name);
}
