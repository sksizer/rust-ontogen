//! Generate the API layer: CRUD forwarding functions that delegate to Store methods.
//!
//! For each entity, generates an `api/v1/generated/{entity}.rs` containing:
//! - `list(store) -> Result<Vec<Entity>, AppError>` (paged, plus `count`, when paginated)
//! - `get_by_id(store, id) -> Result<Entity, AppError>`
//! - `create(store, input) -> Result<Entity, AppError>`
//! - `update(store, id, input) -> Result<Entity, AppError>`
//! - `delete(store, id) -> Result<(), AppError>`
//!
//! Also generates `api/v1/generated/mod.rs` re-exporting all modules.
//!
//! `scan_dirs` are scanned before anything is emitted, because a hand-written
//! `list` or `count` in a module of the same name takes the place of the
//! generated one: the store has no filter, so only the author can write a
//! filtered list and the count that matches it. The scan is merged into a
//! unified `ApiOutput` for downstream transport generators.

mod gen_crud;
#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};

use crate::ir::{ApiFnMeta, ApiModule, ApiOutput, OpKind, ParamMeta, Source, StateKind};
use crate::schema::model::EntityDef;
use crate::servers::classify::classify_op;
use crate::servers::parse;
use crate::store::helpers;
use crate::{ApiConfig, CodegenError};

// ─── Public API ──────────────────────────────────────────────────────────────

/// Generate API layer code for the given entities and return merged `ApiOutput`.
///
/// 1. Scans `config.scan_dirs` for hand-written API modules, leaving out
///    everything under `config.output_dir`
/// 2. Generates CRUD forwarding modules to `config.output_dir`, without the
///    `list` or `count` a scanned module of the same name defines
/// 3. Merges scanned modules with generated ones (same-name → fold, new → add)
/// 4. Returns unified `ApiOutput` for downstream `gen_servers`
pub fn generate(entities: &[EntityDef], config: &ApiConfig) -> Result<ApiOutput, CodegenError> {
    let output_dir = &config.output_dir;
    fs::create_dir_all(output_dir)
        .map_err(|e| CodegenError::Api(format!("Failed to create {}: {e}", output_dir.display())))?;

    // Clean stale files from previous entity names
    let expected: std::collections::HashSet<String> = entities
        .iter()
        .filter(|e| !config.exclude.iter().any(|ex| ex == &e.name))
        .map(|e| format!("{}.rs", helpers::to_snake_case(&e.name)))
        .chain(std::iter::once("mod.rs".to_string()))
        .collect();
    crate::clean_generated_dir(output_dir, &expected);

    // The servers and clients stages scan the same directories and print the
    // skip records, so printing them here too would repeat every warning.
    let mut scanned: Vec<(PathBuf, parse::ApiModule)> = Vec::new();
    for scan_dir in &config.scan_dirs {
        if !scan_dir.is_dir() {
            return Err(CodegenError::Api(format!("API scan directory does not exist: {}", scan_dir.display())));
        }
        let result =
            parse::scan_api_dir_excluding(scan_dir, &config.state_type, config.store_type.as_deref(), Some(output_dir));
        scanned.extend(result.modules.into_iter().map(|m| (scan_dir.clone(), m)));
    }

    let mut modules: Vec<ApiModule> = Vec::new();
    let mut mod_names: Vec<String> = Vec::new();

    // Generate CRUD modules from entity metadata
    for entity in entities {
        if config.exclude.iter().any(|ex| ex == &entity.name) {
            continue;
        }

        let snake = helpers::to_snake_case(&entity.name);
        let omit = omissions(&snake, config, scanned.iter().filter(|(_, m)| m.name == snake).map(|(_, m)| m));
        let code = gen_crud::generate_crud_module(entity, config, omit);

        let path = output_dir.join(format!("{snake}.rs"));
        crate::write_and_format(&path, &code)?;

        let module = collect_generated_module_meta(entity);
        modules.push(module);
        mod_names.push(snake);
    }

    // Write mod.rs for generated modules
    mod_names.sort();
    let mod_rs = generate_mod_rs(&mod_names);
    let path = output_dir.join("mod.rs");
    crate::write_and_format(&path, &mod_rs)?;

    for (scan_dir, scanned_module) in &scanned {
        merge_scanned_module(&mut modules, scanned_module, scan_dir);
    }

    Ok(ApiOutput { modules })
}

// ─── Precedence ──────────────────────────────────────────────────────────────

/// The generated functions a module's hand-written counterpart replaces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Omissions {
    pub list: bool,
    pub count: bool,
}

/// Work out what the scanned modules named `snake` replace in its generated module.
///
/// A hand-written `list` brings its own page and filter, so a paginated
/// module's generated `count` (which counts every row) would total different
/// rows than the page and goes with it.
fn omissions<'a>(snake: &str, config: &ApiConfig, scanned: impl Iterator<Item = &'a parse::ApiModule>) -> Omissions {
    let mut has_list = false;
    let mut has_count = false;
    for f in scanned.flat_map(|m| &m.functions) {
        has_list |= f.name == "list" && !f.is_stateless;
        has_count |= f.is_count();
    }
    let paginated = config.paginated.iter().any(|m| m == snake);
    Omissions { list: has_list, count: has_count || (has_list && paginated) }
}

// ─── Scanning → IR conversion ────────────────────────────────────────────────

/// Convert a scanned `parse::ApiFn` into an IR `ApiFnMeta`.
fn convert_scanned_fn(func: &parse::ApiFn, scan_dir: &Path, module_name: &str) -> ApiFnMeta {
    let params: Vec<ParamMeta> =
        func.params.iter().map(|p| ParamMeta { name: p.name.clone(), param_type: p.ty.clone() }).collect();

    let classified_op = classify_op(func);

    ApiFnMeta {
        name: func.name.clone(),
        doc: func.doc.clone(),
        params,
        return_type: func.return_type.clone(),
        source: Source::Scanned {
            module_path: format!("crate::api::v1::{module_name}"),
            file_path: scan_dir.join(format!("{module_name}.rs")),
        },
        classified_op,
        is_stateless: func.is_stateless,
        command_override: func.command_override.clone(),
    }
}

/// Convert a scanned event function into an IR `ApiFnMeta` with `OpKind::EventStream`.
///
/// `params` are the subscription arguments; `return_type` is the item type `T`
/// of the returned `Receiver<T>`.
fn convert_scanned_event(event: &parse::EventFn, scan_dir: &Path, module_name: &str) -> ApiFnMeta {
    ApiFnMeta {
        name: event.name.clone(),
        doc: event.doc.clone(),
        params: event.params.iter().map(|p| ParamMeta { name: p.name.clone(), param_type: p.ty.clone() }).collect(),
        return_type: event.item_type.clone(),
        source: Source::Scanned {
            module_path: format!("crate::api::v1::{module_name}"),
            file_path: scan_dir.join(format!("{module_name}.rs")),
        },
        classified_op: OpKind::EventStream,
        is_stateless: false,
        command_override: None,
    }
}

/// Determine the state type for a scanned module.
///
/// If any function uses Store, the whole module is Store-scoped.
/// If all functions are app-state or event functions, use AppState.
fn determine_state_kind(scanned: &parse::ApiModule) -> StateKind {
    if scanned.functions.iter().any(|f| f.first_param_is_store) { StateKind::Store } else { StateKind::AppState }
}

// ─── Merge logic ─────────────────────────────────────────────────────────────

/// Merge a scanned module into the modules list.
///
/// - If a generated module with the same name exists, fold the scanned functions into it.
///   A scanned `list` or `count` replaces the generated one of that name (the
///   generated file already leaves them out); every other name keeps the generated function.
/// - Otherwise, add the scanned module as a new entry.
fn merge_scanned_module(modules: &mut Vec<ApiModule>, scanned: &parse::ApiModule, scan_dir: &Path) {
    let scanned_fns: Vec<ApiFnMeta> = scanned
        .functions
        .iter()
        .map(|f| convert_scanned_fn(f, scan_dir, &scanned.name))
        .chain(scanned.events.iter().map(|e| convert_scanned_event(e, scan_dir, &scanned.name)))
        .collect();

    if scanned_fns.is_empty() {
        return;
    }

    // Try to find existing generated module with the same name
    if let Some(existing) = modules.iter_mut().find(|m| m.name == scanned.name) {
        for scanned_fn in scanned_fns {
            let replaces = matches!(scanned_fn.name.as_str(), "list" | "count") && !scanned_fn.is_stateless;
            match existing.fns.iter().position(|f| f.name == scanned_fn.name) {
                Some(i) if replaces => existing.fns[i] = scanned_fn,
                Some(_) => {}
                None => existing.fns.push(scanned_fn),
            }
        }
    } else {
        // New purely-custom module
        let state_type = determine_state_kind(scanned);
        modules.push(ApiModule { name: scanned.name.clone(), fns: scanned_fns, state_type });
    }
}

// ─── Generated module metadata ───────────────────────────────────────────────

/// Collect `ApiModule` metadata for a generated CRUD entity module.
fn collect_generated_module_meta(entity: &EntityDef) -> ApiModule {
    let snake = helpers::to_snake_case(&entity.name);
    let source = Source::Generated { module_path: format!("crate::api::v1::generated::{snake}") };

    let fns = vec![
        ApiFnMeta {
            name: "list".to_string(),
            doc: format!("List all {}s", snake),
            params: vec![],
            return_type: format!("Vec<{}>", entity.name),
            source: source.clone(),
            classified_op: OpKind::List,
            is_stateless: false,
            command_override: None,
        },
        ApiFnMeta {
            name: "get_by_id".to_string(),
            doc: format!("Get a single {} by ID", snake),
            params: vec![ParamMeta { name: "id".to_string(), param_type: "&str".to_string() }],
            return_type: entity.name.clone(),
            source: source.clone(),
            classified_op: OpKind::GetById,
            is_stateless: false,
            command_override: None,
        },
        ApiFnMeta {
            name: "create".to_string(),
            doc: format!("Create a new {}", snake),
            params: vec![ParamMeta { name: "input".to_string(), param_type: format!("Create{}Input", entity.name) }],
            return_type: entity.name.clone(),
            source: source.clone(),
            classified_op: OpKind::Create,
            is_stateless: false,
            command_override: None,
        },
        ApiFnMeta {
            name: "update".to_string(),
            doc: format!("Update an existing {}", snake),
            params: vec![
                ParamMeta { name: "id".to_string(), param_type: "&str".to_string() },
                ParamMeta { name: "input".to_string(), param_type: format!("Update{}Input", entity.name) },
            ],
            return_type: entity.name.clone(),
            source: source.clone(),
            classified_op: OpKind::Update,
            is_stateless: false,
            command_override: None,
        },
        ApiFnMeta {
            name: "delete".to_string(),
            doc: format!("Delete a {} by ID", snake),
            params: vec![ParamMeta { name: "id".to_string(), param_type: "&str".to_string() }],
            return_type: "()".to_string(),
            source,
            classified_op: OpKind::Delete,
            is_stateless: false,
            command_override: None,
        },
    ];

    ApiModule { name: snake, fns, state_type: StateKind::Store }
}

// ─── mod.rs generation ───────────────────────────────────────────────────────

fn generate_mod_rs(mod_names: &[String]) -> String {
    let mut code = String::new();
    code.push_str("//! Generated by ontogen. DO NOT EDIT.\n\n");
    for name in mod_names {
        code.push_str(&format!("pub mod {name};\n"));
    }
    code
}
