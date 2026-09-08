//! Server-side code generators - HTTP (Axum), IPC (Tauri), MCP.
//!
//! The client-side TypeScript + admin-registry generators are siblings under
//! [`crate::clients::generators`].

pub mod http;
pub mod ipc;
pub mod mcp;

use std::collections::{BTreeMap, BTreeSet};

use crate::servers::config::Config;
use crate::servers::parse::ApiModule;
use crate::servers::types::collect_type_import;

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
    let surfaces = config.surfaces();
    let mut services: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut types: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();

    for m in modules {
        let entry = |surface: usize| {
            let ident = m.service_ident(surface);
            if ident == m.name { ident } else { format!("{} as {}", m.name, ident) }
        };
        for f in &m.functions {
            let surface = &surfaces[f.surface];
            services.entry(&surface.service_import_path).or_default().insert(entry(f.surface));
            let mut names = Vec::new();
            collect_type_import(&f.return_type_ast, &mut names);
            for p in &f.params {
                collect_type_import(&p.ty_ast, &mut names);
            }
            types.entry(&surface.types_import_path).or_default().extend(names);
        }
        for ev in &m.events {
            services.entry(&surfaces[ev.surface].service_import_path).or_default().insert(entry(ev.surface));
        }
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
