//! Server-side code generators - HTTP (Axum), IPC (Tauri), MCP.
//!
//! The client-side TypeScript + admin-registry generators are siblings under
//! [`crate::clients::generators`].

pub mod http;
pub mod ipc;
pub mod mcp;

use std::collections::{BTreeMap, BTreeSet};

use crate::servers::config::Config;
use crate::servers::parse::{ApiModule, Param};
use crate::servers::types::{collect_type_import, forward_arg_expr, param_to_owned_type};

/// The argument a list's filter parameter `p`, held as its owned type in
/// `binding`, is passed as. With `counted`, `count` takes the same filter
/// after `list`, so `list` gets a clone of a filter it would otherwise
/// consume: one passed by value that is not a number, `bool` or `char`.
/// Every transport decides this one way, so they hand `list` and `count`
/// the same filter.
pub(crate) fn filter_arg(p: &Param, binding: &str, counted: bool) -> String {
    let arg = forward_arg_expr(binding, &p.ty_ast);
    let consumed = arg == binding && !is_copy_primitive(&param_to_owned_type(&p.ty_ast));
    if counted && consumed { format!("{arg}.clone()") } else { arg }
}

/// True for a number, `bool` or `char`, alone or in an `Option`.
fn is_copy_primitive(ty: &str) -> bool {
    let ty = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')).unwrap_or(ty);
    matches!(
        ty,
        "bool"
            | "char"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
    )
}

/// The `use` lines for the service modules and the types the handlers
/// reference: one `use {path}::{...};` block per distinct import path.
///
/// Surfaces are grouped by path, so surfaces sharing a `service_import_path`
/// or `types_import_path` share a block. A module that appears in more than
/// one surface is imported under its own name for its base surface and as
/// `{module}_{surface}` for the others (see [`ApiModule::service_ident`]).
/// Same-named types were already qualified in place by
/// `parse::qualify_shared_types`, so each name reaches one block only.
pub(crate) fn surface_use_stmts(modules: &[ApiModule], config: &Config) -> Vec<String> {
    surface_use_stmts_where(modules, config, &|_| true)
}

/// [`surface_use_stmts`], importing only the type names `keep` accepts: a
/// generator whose handlers name few of the types its fns mention imports
/// just those.
pub(crate) fn surface_use_stmts_where(
    modules: &[ApiModule],
    config: &Config,
    keep: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let surfaces = config.surfaces();
    let mut services: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut types: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();

    for m in modules {
        let entry = |surface: usize| {
            let module = crate::ident::rust_ident(&m.name);
            let ident = m.service_ident(surface);
            if ident == module { ident } else { format!("{module} as {ident}") }
        };
        for f in &m.functions {
            let surface = &surfaces[f.surface];
            services.entry(&surface.service_import_path).or_default().insert(entry(f.surface));
            let mut names = Vec::new();
            collect_type_import(&f.return_type_ast, &mut names);
            // An order's types live in the store, not the types module, and
            // no handler names them: the order is inferred from `list`.
            for p in f.params.iter().filter(|p| p.order_sort_field().is_none()) {
                collect_type_import(&p.ty_ast, &mut names);
            }
            types.entry(&surface.types_import_path).or_default().extend(names);
        }
        for ev in &m.events {
            let surface = &surfaces[ev.surface];
            services.entry(&surface.service_import_path).or_default().insert(entry(ev.surface));
            let mut names = Vec::new();
            collect_type_import(&ev.item_type_ast, &mut names);
            for p in &ev.params {
                collect_type_import(&p.ty_ast, &mut names);
            }
            types.entry(&surface.types_import_path).or_default().extend(names);
        }
    }

    for names in types.values_mut() {
        names.retain(|name| keep(name));
    }
    let render = |path: &str, items: &BTreeSet<String>| {
        format!("use {}::{{\n{}}};\n", path, items.iter().map(|i| format!("    {i},\n")).collect::<String>())
    };
    services
        .iter()
        .map(|(path, items)| render(path, items))
        .chain(types.iter().filter(|(_, items)| !items.is_empty()).map(|(path, items)| render(path, items)))
        .collect()
}
