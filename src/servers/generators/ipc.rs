#![allow(clippy::too_many_lines, clippy::format_push_string)]

//! Generate Tauri IPC command handlers from API modules.
//!
//! The generated file names the consumer's types and API modules bare, so
//! every runtime item in it is a `::`-rooted path and every binding a
//! command makes of its own is `ontogen_`-prefixed: no consumer name can
//! collide with one or shadow it.

use std::fs;
use std::path::Path;

use ontogen_core::ir::OpKind;

use crate::servers::classify::classify_op;
use crate::servers::config::{ApiSurface, Config, PaginationConfig, RoutePrefix, pagination_for};
use crate::servers::generators::{filter_arg, surface_use_stmts};
use crate::servers::parse::{ApiFn, ApiModule, EventFn, Param};
use crate::servers::types::{
    NamingConfig, capitalize, event_name, extract_input_type, forward_arg_expr, inner_type, ipc_arg_key,
    param_to_owned_type,
};

/// Returns the generated prefix param line for IPC commands (e.g., `project_id: Option<String>,`).
/// Empty string if no route_prefix configured.
fn prefix_param_line(config: &Config) -> String {
    match &config.route_prefix {
        Some(prefix) => {
            let pp = &prefix.params[0];
            format!("    {}: Option<String>,\n", pp.name)
        }
        None => String::new(),
    }
}

/// Returns the validation code for state-based IPC commands (validate-only).
fn prefix_validation_line(config: &Config) -> String {
    match &config.route_prefix {
        Some(prefix) => {
            let pp = &prefix.params[0];
            let accessor = &prefix.state_accessor;
            format!(
                "    if let Some(ref ontogen_pid) = {} {{\n\
                 \x20       let ontogen_uuid = ::uuid::Uuid::parse_str(ontogen_pid).map_err(|ontogen_e| ontogen_e.to_string())?;\n\
                 \x20       ontogen_state.{}(&ontogen_uuid).map_err(|ontogen_e| ontogen_e.to_string())?;\n\
                 \x20   }}\n",
                pp.name, accessor,
            )
        }
        None => String::new(),
    }
}

/// Returns Store construction code for a store-based IPC command.
///
/// When project_id is provided, constructs Store via store_for().
/// When not provided, falls back to the fn's surface accessor
/// (`ontogen_state.{store_accessor}().await`).
fn store_construction_line(config: &Config, f: &ApiFn) -> String {
    let store_accessor = &f.store_accessor;
    match &config.route_prefix {
        Some(prefix) => {
            let pp = &prefix.params[0];
            let accessor = &prefix.state_accessor;
            format!(
                "    let ontogen_store = if let Some(ref ontogen_pid) = {} {{\n\
                 \x20       let ontogen_uuid = ::uuid::Uuid::parse_str(ontogen_pid).map_err(|ontogen_e| ontogen_e.to_string())?;\n\
                 \x20       ontogen_state.{}(&ontogen_uuid).map_err(|ontogen_e| ontogen_e.to_string())?\n\
                 \x20   }} else {{\n\
                 \x20       ontogen_state.{store_accessor}().await.map_err(|ontogen_e| ontogen_e.to_string())?\n\
                 \x20   }};\n",
                pp.name, accessor,
            )
        }
        None => format!(
            "    let ontogen_store = ontogen_state.{store_accessor}().await.map_err(|ontogen_e| ontogen_e.to_string())?;\n"
        ),
    }
}

/// Derive the IPC/TS command name for any function.
///
/// Uses entity-first naming with singular entity prefix:
///   CRUD:     `{entity}_list`, `{entity}_get_by_id`, `{entity}_create`, etc.
///   Junction: `{entity}_add_role`, `{entity}_list_skills`, etc.
///   Custom:   `{entity}_publish`, `{entity}_refresh`, etc.
///
/// If the function carries a per-function override (set via the source-side
/// `#[ontogen(rename = "...")]` attribute or via
/// [`NamingConfig::command_overrides`](crate::servers::types::NamingConfig::command_overrides)),
/// that value is returned verbatim and the default scheme is skipped.
pub fn command_name(module: &str, f: &ApiFn, config: &Config) -> String {
    command_name_in(&config.naming, module, f)
}

/// [`command_name`] under `naming`, for the stages that name commands
/// without the servers' [`Config`].
pub(crate) fn command_name_in(naming: &NamingConfig, module: &str, f: &ApiFn) -> String {
    f.command_override.clone().unwrap_or_else(|| format!("{}_{}", naming.url_singular(module), f.name))
}

/// What the IPC wire-key rules ([`check_wire_keys`]) read of a stage's
/// configuration. The servers stage checks the commands it generates; the
/// clients stage checks the ones its TS transport invokes, which may be
/// served by a build that is not this one.
pub(crate) struct WireKeyScope<'a> {
    pub naming: &'a NamingConfig,
    pub pagination: &'a Option<PaginationConfig>,
    pub extra_surfaces: &'a [ApiSurface],
    pub route_prefix: Option<&'a RoutePrefix>,
    /// Commands the stage does not emit (the clients' `ts_skip_commands`),
    /// which cannot collide.
    pub skip_commands: &'a [String],
}

impl WireKeyScope<'_> {
    /// The servers stage's scope: every command it emits.
    pub(crate) fn of(config: &Config) -> WireKeyScope<'_> {
        WireKeyScope {
            naming: &config.naming,
            pagination: &config.pagination,
            extra_surfaces: &config.extra_surfaces,
            route_prefix: config.route_prefix.as_ref(),
            skip_commands: &[],
        }
    }

    fn paginated(&self, m: &ApiModule, f: &ApiFn) -> bool {
        pagination_for(self.pagination, self.extra_surfaces, &m.name, f.surface).is_some()
    }
}

/// Refuses a fn whose IPC command would take two arguments under one invoke
/// key. Tauri reads each command parameter from the invoke payload under
/// its [`ipc_arg_key`], and the TS transport invokes with the same keys, so
/// two parameters whose keys match (`sort`, `_sort` and `sort_`;
/// `project_id` and `_project_id`) would share one key. The command's own
/// parameters cannot be renamed in the generated code:
///
/// - `id` and `input` for a CRUD op's record id and input;
/// - `query` for a list's `*Query` struct, `sort` for the sort keys of a
///   list that takes an order, and `limit` and `offset` for the page of a
///   paginated list or junction list;
/// - `channel` for an event subscription's channel;
/// - under a route prefix, the prefix parameter (`project_id`) for every
///   fn's command but an event subscription, which is not project-scoped.
///
/// Every other parameter carries a fn argument under the argument's own
/// name, so an argument may collide with one of these or with another
/// argument, and the prefix parameter with one of the command's own.
pub(crate) fn check_wire_keys(modules: &[ApiModule], scope: &WireKeyScope<'_>) -> Result<(), String> {
    for m in modules {
        for f in &m.functions {
            let command = command_name_in(scope.naming, &m.name, f);
            if command.is_empty() || scope.skip_commands.contains(&command) {
                continue;
            }
            let (own, args) = command_wire_keys(m, f, scope.paginated(m, f));
            let prefix = scope.route_prefix.map(|prefix| prefix.params[0].name.as_str());
            check_command_keys(m, &f.name, &command, own, prefix, &args)?;
        }
        for ev in &m.events {
            let command = format!("{}_subscribe", ev.name);
            let args: Vec<&str> = ev.params.iter().map(|p| p.name.as_str()).collect();
            check_command_keys(
                m,
                &ev.name,
                &command,
                vec![("channel", "the subscription's event channel")],
                None,
                &args,
            )?;
        }
    }
    Ok(())
}

/// Refuses the first parameter of a command whose invoke key one before it
/// already took: `own` (key, what the command takes under it), then the
/// route prefix parameter, then the fn's arguments, in order.
fn check_command_keys(
    m: &ApiModule,
    fn_name: &str,
    command: &str,
    own: Vec<(&str, &str)>,
    prefix: Option<&str>,
    args: &[&str],
) -> Result<(), String> {
    let collide = |named: &str, key: &str, use_: &str, rename: &str| {
        Err(format!(
            "ontogen: the IPC command `{command}` cannot be generated: `{}::{fn_name}` {named}, which the command \
             takes under the IPC wire key `{key}`, the key it uses for {use_}, so the two would collide. Rename the \
             {rename}.",
            m.name
        ))
    };
    let mut taken: Vec<(String, String)> =
        own.into_iter().map(|(key, use_)| (key.to_string(), use_.to_string())).collect();
    if let Some(param) = prefix {
        let key = ipc_arg_key(param);
        if let Some((_, use_)) = taken.iter().find(|(taken, _)| *taken == key) {
            let named = format!("is called with the route prefix parameter `{param}`");
            return collide(&named, &key, use_, "route prefix parameter");
        }
        taken.push((key, "the route prefix parameter".into()));
    }
    for arg in args {
        let key = ipc_arg_key(arg);
        if let Some((_, use_)) = taken.iter().find(|(taken, _)| *taken == key) {
            return collide(&format!("takes an argument named `{arg}`"), &key, use_, "argument");
        }
        taken.push((key, format!("the argument `{arg}`")));
    }
    Ok(())
}

/// The invoke keys of a fn's command, as the generator emits its
/// parameters: the ones the command takes of its own, as (key, what it
/// takes under it), and the names of the fn arguments it takes each under
/// its own name. A CRUD op's arguments travel only as `id` and `input`.
fn command_wire_keys<'a>(
    m: &ApiModule,
    f: &'a ApiFn,
    paginated: bool,
) -> (Vec<(&'static str, &'static str)>, Vec<&'a str>) {
    const PAGE: [(&str, &str); 2] =
        [("limit", "the page's `limit` and `offset`"), ("offset", "the page's `limit` and `offset`")];
    let args = || f.params.iter().map(|p| p.name.as_str()).collect();
    match classify_op(m, f) {
        OpKind::GetById | OpKind::Delete => (vec![("id", "the record's id")], vec![]),
        OpKind::Create => (vec![("input", "the record's input")], vec![]),
        OpKind::Update => (vec![("id", "the record's id"), ("input", "the record's input")], vec![]),
        OpKind::List => {
            let mut own = Vec::new();
            if f.filter_struct().is_some() {
                own.push(("query", "the list's `*Query` filter struct"));
            }
            if f.takes_order() {
                own.push(("sort", "the list's sort keys"));
            }
            if paginated && f.return_type.starts_with("Vec<") {
                own.extend(PAGE);
            }
            (own, f.bare_filters().into_iter().map(|p| p.name.as_str()).collect())
        }
        OpKind::JunctionList { .. } if paginated && f.return_type.starts_with("Vec<") => (PAGE.to_vec(), args()),
        _ => (vec![], args()),
    }
}

/// Generate IPC command handlers and write to the output file.
///
/// Each command parameter that carries a fn argument is named after it,
/// since Tauri derives the IPC wire key the TS transport invokes with from
/// the parameter's name ([`ipc_arg_key`]). Every other binding a command
/// makes of its own is `ontogen_`-prefixed (`ontogen_state`,
/// `ontogen_store`, `ontogen_limit`, …), so an argument named `state`,
/// `store` or `limit` neither collides with one nor is shadowed by one.
/// Tauri's `State` extractor is matched by type, not by name, so its
/// parameter is not a wire key.
///
/// The only names the file brings into scope bare are the consumer's: its
/// API modules, the types its fns name, its state and store. Every runtime
/// item is written by its `::`-rooted path, so no consumer name (an entity
/// `State`, a module `log`) collides with one or shadows its crate.
pub fn generate(output: &Path, modules: &[ApiModule], config: &Config) {
    let mut out = String::new();
    let state_type = &config.state_type;

    out.push_str(
        "\
#![allow(dead_code, unused_imports, clippy::pedantic)]
//! Auto-generated Tauri IPC command handlers. DO NOT EDIT.
//!
//! Generated by ontogen from API source files. Every runtime item is named
//! by its `::`-rooted path and every binding a command makes of its own is
//! `ontogen_`-prefixed, so besides `PaginatedResult` the only types and
//! modules it names bare are your own types and API modules.

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

    out.push('\n');

    if config.any_pagination() {
        out.push_str(
            "\
#[derive(::serde::Serialize)]
pub struct PaginatedResult<T: ::serde::Serialize> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

",
        );
    }

    let pp_line = prefix_param_line(config);
    let pp_validate = prefix_validation_line(config);

    let mut command_names: Vec<String> = Vec::new();

    for m in modules {
        let module = &m.name;

        if m.functions.is_empty() {
            continue;
        }

        out.push_str(&format!("// ── {} IPC Commands ──\n\n", capitalize(module)));

        for f in &m.functions {
            let op = classify_op(m, f);
            let svc = m.service_ident(f.surface);
            let is_async = f.is_async;
            let ret_type = &f.return_type;
            let pagination = config.pagination_for(module, f.surface);

            // Determine per-function prefix and first-arg behavior
            let (fn_pp_line, fn_pp_body, first_arg) = if f.first_param_is_store {
                (pp_line.clone(), store_construction_line(config, f), "&ontogen_store")
            } else {
                (pp_line.clone(), pp_validate.clone(), "&ontogen_state")
            };

            match op {
                OpKind::List => {
                    let cmd_name = command_name(module, f, config);
                    let await_str = if is_async { ".await" } else { "" };
                    let paginated = pagination.is_some() && ret_type.starts_with("Vec<");
                    // Each filter is a command argument: a `*Query` struct as
                    // `query`, a bare filter under its own name, both as their
                    // owned types. A list that takes the page owns its
                    // limit/offset: they are never caller params.
                    let filter_struct = f.filter_struct();
                    let binding = |p: &Param| if p.is_filter_struct() { "query".to_string() } else { p.name.clone() };
                    let mut param_lines = String::new();
                    if let Some(qp) = filter_struct {
                        param_lines.push_str(&format!("    query: {},\n", extract_input_type(&qp.ty)));
                    }
                    for pp in f.bare_filters() {
                        param_lines.push_str(&format!("    {}: {},\n", pp.name, param_to_owned_type(&pp.ty_ast)));
                    }
                    // A list that takes an order reads it from `sort`, as the
                    // other transports do, and passes it after its filter.
                    let (sort, order_arg) = if f.takes_order() {
                        param_lines.push_str("    sort: Option<Vec<String>>,\n");
                        (ORDER_FROM_SORT, ", &ontogen_order")
                    } else {
                        ("", "")
                    };
                    // A filtered page calls `count` with the same filter after
                    // `list`, which gets a clone of whatever it would consume.
                    let filter_args = |counted: bool| -> String {
                        f.filter().iter().map(|p| format!(", {}", filter_arg(p, &binding(p), counted))).collect()
                    };
                    let count_args = filter_args(false);
                    let extra_args = format!("{count_args}{order_arg}");
                    let list_args = format!("{}{order_arg}", filter_args(true));
                    if let Some(pg) = pagination
                        && paginated
                    {
                        let item_type = inner_type(ret_type);
                        let default_limit = pg.default_limit;
                        let max_limit = pg.max_limit;
                        param_lines.push_str("    limit: Option<u32>,\n");
                        param_lines.push_str("    offset: Option<u32>,\n");
                        out.push_str(&format!(
                            "\
#[::tauri::command]
pub async fn {cmd_name}(
{param_lines}{fn_pp_line}    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,
) -> Result<PaginatedResult<{item_type}>, String> {{
{sort}{fn_pp_body}    let ontogen_limit = limit.unwrap_or({default_limit}).min({max_limit});
    let ontogen_offset = offset.unwrap_or(0);
    let ontogen_items = {svc}::list({first_arg}{list_args}, Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset))){await_str}
        .map_err(|ontogen_e| ontogen_e.to_string())?;
    let ontogen_total = {svc}::count({first_arg}{count_args}){await_str}
        .map_err(|ontogen_e| ontogen_e.to_string())?;
    {PAGE_RESULT}
}}

"
                        ));
                    } else {
                        // This surface does not paginate: a list that takes the page gets the whole table.
                        let page_args = if f.takes_page() { ", None, None" } else { "" };
                        out.push_str(&format!(
                            "\
#[::tauri::command]
pub async fn {cmd_name}(
{param_lines}{fn_pp_line}    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,
) -> Result<{ret_type}, String> {{
{sort}{fn_pp_body}    {svc}::list({first_arg}{extra_args}{page_args}){await_str}
        .map_err(|ontogen_e| ontogen_e.to_string())
}}

"
                        ));
                    }
                    command_names.push(cmd_name);
                }

                OpKind::GetById => {
                    let cmd_name = command_name(module, f, config);
                    let fn_name = &f.name;
                    if is_async {
                        out.push_str(&format!(
                            "\
#[::tauri::command]
pub async fn {cmd_name}(
    id: String,
{fn_pp_line}    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,
) -> Result<{ret_type}, String> {{
{fn_pp_body}    {svc}::{fn_name}({first_arg}, &id)
        .await
        .map_err(|ontogen_e| ontogen_e.to_string())
}}

"
                        ));
                    } else {
                        out.push_str(&format!(
                            "\
#[::tauri::command]
pub async fn {cmd_name}(
    id: String,
{fn_pp_line}    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,
) -> Result<{ret_type}, String> {{
{fn_pp_body}    {svc}::{fn_name}({first_arg}, &id)
        .map_err(|ontogen_e| ontogen_e.to_string())
}}

"
                        ));
                    }
                    command_names.push(cmd_name);
                }

                OpKind::Create => {
                    let cmd_name = command_name(module, f, config);
                    let input_type = extract_input_type(&f.params[0].ty);
                    let await_str = if is_async { "\n        .await" } else { "" };
                    out.push_str(&format!(
                        "\
#[::tauri::command]
pub async fn {cmd_name}(
    input: {input_type},
{fn_pp_line}    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,
) -> Result<{ret_type}, String> {{
{fn_pp_body}    {svc}::create({first_arg}, input){await_str}
        .map_err(|ontogen_e| ontogen_e.to_string())
}}

"
                    ));
                    command_names.push(cmd_name);
                }

                OpKind::Update => {
                    let cmd_name = command_name(module, f, config);
                    let input_type = extract_input_type(&f.params[1].ty);
                    let await_str = if is_async { "\n        .await" } else { "" };
                    out.push_str(&format!(
                        "\
#[::tauri::command]
pub async fn {cmd_name}(
    id: String,
    input: {input_type},
{fn_pp_line}    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,
) -> Result<{ret_type}, String> {{
{fn_pp_body}    {svc}::update({first_arg}, &id, input){await_str}
        .map_err(|ontogen_e| ontogen_e.to_string())
}}

"
                    ));
                    command_names.push(cmd_name);
                }

                OpKind::Delete => {
                    let cmd_name = command_name(module, f, config);
                    let await_str = if is_async { "\n        .await" } else { "" };
                    out.push_str(&format!(
                        "\
#[::tauri::command]
pub async fn {cmd_name}(
    id: String,
{fn_pp_line}    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,
) -> Result<(), String> {{
{fn_pp_body}    {svc}::delete({first_arg}, &id){await_str}
        .map_err(|ontogen_e| ontogen_e.to_string())
}}

"
                    ));
                    command_names.push(cmd_name);
                }

                OpKind::JunctionList { .. } => {
                    let cmd_name = command_name(module, f, config);
                    if let Some(pg) = pagination
                        && ret_type.starts_with("Vec<")
                    {
                        generate_paginated_ipc_handler(&mut out, m, f, config, pg);
                    } else {
                        generate_generic_ipc_handler(&mut out, m, f, config);
                    }
                    command_names.push(cmd_name);
                }

                OpKind::JunctionAdd { .. } | OpKind::JunctionRemove { .. } => {
                    let cmd_name = command_name(module, f, config);
                    generate_generic_ipc_handler(&mut out, m, f, config);
                    command_names.push(cmd_name);
                }

                OpKind::CustomGet | OpKind::CustomPost => {
                    let cmd_name = command_name(module, f, config);
                    generate_generic_ipc_handler(&mut out, m, f, config);
                    command_names.push(cmd_name);
                }

                OpKind::EventStream => continue,
            }
        }
    }

    // Per-subscriber event subscriptions: a subscribe/unsubscribe pair per event fn.
    if modules.iter().any(|m| !m.events.is_empty()) {
        out.push_str(
            "\
// ── Event Subscriptions ──

/// Live IPC event subscriptions, by id. A process-wide static rather than
/// consumer state: an id only means something to the process that issued it,
/// and the consumer has nothing to wire. A forwarding task leaves it when its
/// receiver closes, when a `Channel::send` fails (the webview is gone; a
/// `Channel` has no close callback), or on an explicit unsubscribe.
static EVENT_SUBSCRIPTIONS: ::ontogen_core::events::Subscriptions = ::ontogen_core::events::Subscriptions::new();

",
        );
        for m in modules {
            for ev in &m.events {
                generate_event_subscription(&mut out, m, ev, config);
                command_names.push(format!("{}_subscribe", ev.name));
                command_names.push(format!("{}_unsubscribe", ev.name));
            }
        }
    }

    // Global forwarding for the parameterless event shape, kept so consumers
    // that call `start_event_forwarding` at setup keep working. Parameterized
    // event ops have no global form: a subscription names what it wants.
    let has_legacy_events = modules.iter().any(|m| m.events.iter().any(EventFn::is_legacy));
    if has_legacy_events {
        out.push_str(&format!(
            "\
// ── Event Forwarding (global) ──

/// Emit every parameterless event stream to all windows as a Tauri event.
/// Call this during app setup. Prefer the per-subscriber `*_subscribe`
/// commands, which report lag to the subscriber and end with it.
pub fn start_event_forwarding(app_handle: ::tauri::AppHandle, state: &{}) {{
",
            config.state_import.split("::").last().unwrap_or(&config.state_type),
        ));

        let surfaces = config.surfaces();
        for m in modules {
            for ev in m.events.iter().filter(|ev| ev.is_legacy()) {
                let fn_name = &ev.name;
                let ev_name = event_name(fn_name);
                let svc = crate::ident::rust_ident(&m.name);
                let service = &surfaces[ev.surface].service_import_path;
                out.push_str(&format!(
                    "\
    // {fn_name}
    {{
        let handle = app_handle.clone();
        let mut rx = {service}::{svc}::{fn_name}(state);
        ::tauri::async_runtime::spawn(async move {{
            loop {{
                match rx.recv().await {{
                    Ok(delta) => {{
                        if let Err(e) = ::tauri::Emitter::emit(&handle, \"{ev_name}\", &delta) {{
                            ::log::error!(\"Failed to forward {ev_name} to IPC: {{:?}}\", e);
                        }}
                    }}
                    Err(::tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {{
                        ::log::warn!(\"{ev_name} forwarding lagged; {{}} events dropped\", skipped);
                    }}
                    Err(::tokio::sync::broadcast::error::RecvError::Closed) => break,
                }}
            }}
        }});
    }}
",
                ));
            }
        }

        out.push_str("}\n");
    }

    // Generate ipc_handler() wrapper with tauri::generate_handler!
    out.push_str(
        "/// Generated IPC handler. Wire this into `tauri::Builder::invoke_handler()`.\n\
         pub fn ipc_handler() -> impl Fn(::tauri::ipc::Invoke) -> bool + Send + Sync + 'static {\n\
         \x20   ::tauri::generate_handler![\n",
    );
    for name in &command_names {
        out.push_str(&format!("        {},\n", name));
    }
    out.push_str("    ]\n}\n");

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("Failed to create output directory");
    }
    crate::write_and_format(output, out).expect("Failed to write IPC generated file");
}

/// Generate the `{fn}_subscribe` / `{fn}_unsubscribe` command pair for one
/// event fn.
///
/// Subscribe calls the event fn with the invoke args, spawns a task that
/// forwards `EventFrame`s into the caller's `Channel`, and returns the
/// subscription id. The route prefix, if any, does not apply: like the
/// global forwarding it replaces, an IPC subscription is not project-scoped.
fn generate_event_subscription(out: &mut String, m: &ApiModule, ev: &EventFn, config: &Config) {
    let fn_name = &ev.name;
    let svc = m.service_ident(ev.surface);
    let state_type = &config.state_type;
    let item_type = &ev.item_type;

    // A block doc comment (`/** … */`) reaches here as one string with line
    // breaks, each of which must open its own `///` line.
    for line in ev.doc.lines().map(str::trim) {
        if line.is_empty() {
            out.push_str("///\n");
        } else {
            out.push_str(&format!("/// {line}\n"));
        }
    }
    out.push_str(&format!("#[::tauri::command]\npub async fn {fn_name}_subscribe(\n"));
    for p in &ev.params {
        out.push_str(&format!("    {}: {},\n", p.name, param_to_owned_type(&p.ty_ast)));
    }
    out.push_str(&format!(
        "    channel: ::tauri::ipc::Channel<::ontogen_core::events::EventFrame<{item_type}>>,\n\
         \x20   ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,\n\
         ) -> Result<u64, String> {{\n"
    ));
    let mut args = vec!["&ontogen_state".to_string()];
    args.extend(ev.params.iter().map(|p| forward_arg_expr(&p.name, &p.ty_ast)));
    let await_str = if ev.is_async { ".await" } else { "" };
    let map_err = if ev.returns_result { ".map_err(|ontogen_e| ontogen_e.to_string())?" } else { "" };
    let id_fn = if ev.is_resumable() { "::ontogen_core::events::seq_id" } else { "::ontogen_core::events::no_id" };
    out.push_str(&format!(
        "    let ontogen_rx = {svc}::{fn_name}({}){await_str}{map_err};\n\
         \x20   Ok(EVENT_SUBSCRIPTIONS.spawn(::ontogen_core::events::forward(ontogen_rx, {id_fn}, move |ontogen_frame| {{\n\
         \x20       channel.send(ontogen_frame)\n\
         \x20   }})))\n\
         }}\n\n",
        args.join(", ")
    ));

    out.push_str(&format!(
        "/// End a `{fn_name}_subscribe` subscription. Returns `false` when it already ended.\n\
         #[::tauri::command]\n\
         pub fn {fn_name}_unsubscribe(id: u64) -> bool {{\n\
         \x20   EVENT_SUBSCRIPTIONS.cancel(id)\n\
         }}\n\n"
    ));
}

fn generate_generic_ipc_handler(out: &mut String, m: &ApiModule, f: &ApiFn, config: &Config) {
    let module = m.name.as_str();
    let fn_name = &f.name;
    let svc = m.service_ident(f.surface);
    let is_async = f.is_async;
    let ret_type = &f.return_type;
    let await_str = if is_async { "\n        .await" } else { "" };
    let state_type = &config.state_type;
    let pp_line = prefix_param_line(config);

    // Stateless handlers omit the `ontogen_state: State<...>` extractor, the prefix
    // validation / store-construction body, and the positional state/store
    // argument when forwarding to the service function. The route-prefix
    // parameter (e.g. `:project_id`) is still threaded through if configured.
    let (fn_pp_body, first_arg) = if f.is_stateless {
        (String::new(), None)
    } else if f.first_param_is_store {
        (store_construction_line(config, f), Some("&ontogen_store"))
    } else {
        (prefix_validation_line(config), Some("&ontogen_state"))
    };

    let cmd_fn_name = command_name(module, f, config);
    out.push_str(&format!("#[::tauri::command]\npub async fn {}(\n", cmd_fn_name));

    for p in &f.params {
        let owned_ty = param_to_owned_type(&p.ty_ast);
        out.push_str(&format!("    {}: {},\n", p.name, owned_ty));
    }

    out.push_str(&pp_line);
    if !f.is_stateless {
        out.push_str(&format!("    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,\n"));
    }
    out.push_str(&format!(") -> Result<{}, String> {{\n", ret_type));
    out.push_str(&fn_pp_body);

    out.push_str(&format!("    {}::{}(", svc, fn_name));
    let mut first = true;
    if let Some(arg) = first_arg {
        out.push_str(arg);
        first = false;
    }
    for p in &f.params {
        if !first {
            out.push_str(", ");
        }
        out.push_str(&forward_arg_expr(&p.name, &p.ty_ast));
        first = false;
    }
    out.push(')');
    out.push_str(await_str);
    out.push_str("\n        .map_err(|ontogen_e| ontogen_e.to_string())\n");
    out.push_str("}\n\n");
}

/// Generate a paginated IPC handler for JunctionList operations.
///
/// Wraps the service call result in `PaginatedResult<T>` with limit/offset params.
fn generate_paginated_ipc_handler(
    out: &mut String,
    m: &ApiModule,
    f: &ApiFn,
    config: &Config,
    pg: &crate::servers::config::PaginationConfig,
) {
    let module = m.name.as_str();
    let fn_name = &f.name;
    let svc = m.service_ident(f.surface);
    let is_async = f.is_async;
    let ret_type = &f.return_type;
    let item_type = inner_type(ret_type);
    let await_str = if is_async { ".await" } else { "" };
    let state_type = &config.state_type;
    let pp_line = prefix_param_line(config);
    let default_limit = pg.default_limit;
    let max_limit = pg.max_limit;

    let (fn_pp_body, first_arg) = if f.is_stateless {
        (String::new(), None)
    } else if f.first_param_is_store {
        (store_construction_line(config, f), Some("&ontogen_store"))
    } else {
        (prefix_validation_line(config), Some("&ontogen_state"))
    };

    let cmd_fn_name = command_name(module, f, config);
    out.push_str(&format!("#[::tauri::command]\npub async fn {}(\n", cmd_fn_name));

    for p in &f.params {
        let owned_ty = param_to_owned_type(&p.ty_ast);
        out.push_str(&format!("    {}: {},\n", p.name, owned_ty));
    }

    out.push_str("    limit: Option<u32>,\n");
    out.push_str("    offset: Option<u32>,\n");
    out.push_str(&pp_line);
    if !f.is_stateless {
        out.push_str(&format!("    ontogen_state: ::tauri::State<'_, ::std::sync::Arc<{state_type}>>,\n"));
    }
    out.push_str(&format!(") -> Result<PaginatedResult<{}>, String> {{\n", item_type));
    out.push_str(&fn_pp_body);

    out.push_str(&format!("    let ontogen_all = {}::{}(", svc, fn_name));
    let mut first = true;
    if let Some(arg) = first_arg {
        out.push_str(arg);
        first = false;
    }
    for p in &f.params {
        if !first {
            out.push_str(", ");
        }
        out.push_str(&forward_arg_expr(&p.name, &p.ty_ast));
        first = false;
    }
    out.push(')');
    out.push_str(await_str);
    out.push_str("\n        .map_err(|ontogen_e| ontogen_e.to_string())?;\n");
    out.push_str("    let ontogen_total = ontogen_all.len() as u64;\n");
    out.push_str(&format!("    let ontogen_limit = limit.unwrap_or({default_limit}).min({max_limit});\n"));
    out.push_str("    let ontogen_offset = offset.unwrap_or(0);\n");
    out.push_str(
        "    let ontogen_items = ontogen_all.into_iter().skip(ontogen_offset as usize).take(ontogen_limit as usize).collect();\n",
    );
    out.push_str(PAGE_RESULT);
    out.push('\n');
    out.push_str("}\n\n");
}

/// A sorted list command's read of its `sort` argument into the order its
/// list takes, with the parser HTTP and MCP use: absent is the default
/// order, and a bad key is the parser's error text.
const ORDER_FROM_SORT: &str = "    let ontogen_order = ::ontogen_core::order::parse_sort(sort.unwrap_or_default())
        .map_err(|ontogen_e| ontogen_e.to_string())?;
";

/// The closing of a paginated command: the page and its total, from the
/// command's `ontogen_`-prefixed bindings.
const PAGE_RESULT: &str = "    Ok(PaginatedResult {
        items: ontogen_items,
        total: ontogen_total,
        limit: ontogen_limit,
        offset: ontogen_offset,
    })";
