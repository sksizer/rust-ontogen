#![allow(clippy::doc_markdown, clippy::manual_let_else, clippy::module_name_repetitions)]

//! Parse Rust service API source files into structured metadata.
//!
//! Extracts function signatures, parameters, return types, and event functions
//! from files where public functions take a `&{StateType}` as their first parameter.

use std::fs;
use std::path::Path;

use syn::{FnArg, GenericArgument, PathArguments, ReturnType, Type, Visibility};

use crate::servers::types::norm_type;

// ─── Extracted function metadata ──────────────────────────────────────────────

/// A parsed API function signature.
#[derive(Debug, Clone)]
pub struct ApiFn {
    /// Function name (e.g., `list`, `get_by_id`, `create`).
    pub name: String,
    /// Whether the function is async.
    pub is_async: bool,
    /// Doc comment text (joined from `///` lines).
    pub doc: String,
    /// Parameters AFTER skipping the state parameter.
    pub params: Vec<Param>,
    /// The inner `T` from `Result<T, E>`, as a normalized string.
    pub return_type: String,
    /// Whether the first parameter is a store type (vs app state type).
    ///
    /// When true, generated handlers construct a Store from the AppState
    /// and pass it to the service function. When false, handlers pass
    /// the AppState directly.
    pub first_param_is_store: bool,
}

/// A single function parameter.
#[derive(Debug, Clone)]
pub struct Param {
    /// Parameter name.
    pub name: String,
    /// Normalized type string (no extra spaces).
    pub ty: String,
}

/// An event function that returns a `broadcast::Receiver<T>`.
#[derive(Debug, Clone)]
pub struct EventFn {
    /// Function name (e.g., `graph_updated`).
    pub name: String,
}

/// A parsed API module with its functions and events.
#[derive(Debug, Clone)]
pub struct ApiModule {
    /// Module name (derived from file stem).
    pub name: String,
    /// Regular API functions.
    pub functions: Vec<ApiFn>,
    /// Event broadcast functions.
    pub events: Vec<EventFn>,
}

impl ApiModule {
    /// Returns true if this module has a complete CRUD surface
    /// (list, get_by_id, create, update, delete).
    pub fn is_crud(&self) -> bool {
        let fns: Vec<&str> = self.functions.iter().map(|f| f.name.as_str()).collect();
        fns.contains(&"list")
            && fns.contains(&"get_by_id")
            && fns.contains(&"create")
            && fns.contains(&"update")
            && fns.contains(&"delete")
    }
}

// ─── Parsing ──────────────────────────────────────────────────────────────────

/// Parse a single API source file into an `ApiModule`.
///
/// Returns `None` if the file is `mod.rs`, can't be read, or can't be parsed.
/// Only public functions whose first parameter type contains `state_type`
/// (or `store_type` when configured) are included.
pub fn parse_api_module(path: &Path, state_type: &str, store_type: Option<&str>) -> Option<ApiModule> {
    let file_stem = path.file_stem()?.to_str()?;
    if file_stem == "mod" {
        return None;
    }

    let source = fs::read_to_string(path).ok()?;
    let syntax = syn::parse_file(&source).ok()?;

    let mut functions = Vec::new();
    let mut events = Vec::new();
    for item in &syntax.items {
        if let syn::Item::Fn(func) = item {
            if !matches!(func.vis, Visibility::Public(_)) {
                continue;
            }

            // Check if first param matches state_type or store_type
            let first_param = func.sig.inputs.first();
            let (is_accepted, is_store) = match first_param {
                Some(FnArg::Typed(pat)) => {
                    let ty = norm_type(&pat.ty);
                    if ty.contains(state_type) {
                        (true, false)
                    } else if let Some(st) = store_type {
                        if ty.contains(st) { (true, true) } else { (false, false) }
                    } else {
                        (false, false)
                    }
                }
                _ => (false, false),
            };
            if !is_accepted {
                continue;
            }

            let doc = func
                .attrs
                .iter()
                .filter_map(|attr| {
                    if attr.path().is_ident("doc")
                        && let syn::Meta::NameValue(nv) = &attr.meta
                        && let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value
                    {
                        return Some(s.value().trim().to_string());
                    }
                    None
                })
                .collect::<Vec<_>>()
                .join(" ");

            // Check if this is an event function (returns broadcast::Receiver<T>)
            if is_receiver_return_type(&func.sig.output) {
                events.push(EventFn { name: func.sig.ident.to_string() });
                continue;
            }

            let params: Vec<Param> = func
                .sig
                .inputs
                .iter()
                .skip(1) // skip state
                .filter_map(|arg| {
                    if let FnArg::Typed(pat) = arg {
                        let ty = norm_type(&pat.ty);
                        let name = match pat.pat.as_ref() {
                            syn::Pat::Ident(ident) => ident.ident.to_string(),
                            _ => String::new(),
                        };
                        Some(Param { name, ty })
                    } else {
                        None
                    }
                })
                .collect();

            let return_type = extract_result_ok_type(&func.sig.output);

            functions.push(ApiFn {
                name: func.sig.ident.to_string(),
                is_async: func.sig.asyncness.is_some(),
                doc,
                params,
                return_type,
                first_param_is_store: is_store,
            });
        }
    }

    Some(ApiModule { name: file_stem.to_string(), functions, events })
}

/// Check if the return type is `broadcast::Receiver<T>` or `Receiver<T>`.
fn is_receiver_return_type(ret: &ReturnType) -> bool {
    if let ReturnType::Type(_, ty) = ret
        && let Type::Path(tp) = ty.as_ref()
        && let Some(seg) = tp.path.segments.last()
    {
        return seg.ident == "Receiver";
    }
    false
}

/// Extract `T` from `Result<T, E>`.
fn extract_result_ok_type(ret: &ReturnType) -> String {
    if let ReturnType::Type(_, ty) = ret
        && let Type::Path(tp) = ty.as_ref()
    {
        let seg = tp.path.segments.last().unwrap();
        if seg.ident == "Result"
            && let PathArguments::AngleBracketed(args) = &seg.arguments
            && let Some(GenericArgument::Type(t)) = args.args.first()
        {
            return norm_type(t);
        }
    }
    "()".to_string()
}

/// Scan a directory for API source files and parse them all.
///
/// Skips files ending in `_impl.rs` and `mod.rs`.
pub fn scan_api_dir(api_dir: &Path, state_type: &str, store_type: Option<&str>) -> Vec<ApiModule> {
    let mut modules = Vec::new();

    // Collect .rs files from api_dir and its immediate subdirectories (e.g. generated/)
    let mut entries: Vec<_> = collect_rs_files(api_dir);
    entries.sort();

    for path in entries {
        if let Some(m) = parse_api_module(&path, state_type, store_type)
            && (!m.functions.is_empty() || !m.events.is_empty())
        {
            modules.push(m);
        }
    }

    modules
}

/// Collect `.rs` files from a directory and its immediate subdirectories.
/// Skips `_impl` suffixed files. Does not recurse deeper than one level.
fn collect_rs_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).unwrap_or_else(|_| panic!("Failed to read {}", dir.display()));

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Scan one level of subdirectories (e.g. generated/)
            if let Ok(sub_entries) = fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    let sub_path = sub_entry.path();
                    if is_scannable_rs_file(&sub_path) {
                        files.push(sub_path);
                    }
                }
            }
        } else if is_scannable_rs_file(&path) {
            files.push(path);
        }
    }

    files
}

/// Check if a path is a scannable `.rs` file (not `mod.rs`, not `_impl` suffix).
fn is_scannable_rs_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "rs")
        && !path.file_stem().is_some_and(|s| s.to_str().is_some_and(|s| s == "mod" || s.ends_with("_impl")))
}
