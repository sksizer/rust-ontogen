#![allow(clippy::doc_markdown, clippy::manual_let_else, clippy::module_name_repetitions)]

//! Parse Rust service API source files into structured metadata.
//!
//! Extracts function signatures, parameters, return types, and event functions
//! from files where public functions take a `&{StateType}` as their first parameter.

use std::fs;
use std::path::{Path, PathBuf};

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
    /// The inner `T` from `Result<T, E>`, as a `syn::Type` AST.
    ///
    /// Carrying the AST forward lets downstream consumers (notably
    /// `collect_type_import`) recurse structurally into generic args
    /// instead of substring-matching the rendered string.
    pub return_type_ast: syn::Type,
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
    /// Parameter type as a `syn::Type` AST.
    ///
    /// Used by `collect_type_import` to walk into generic arguments
    /// instead of relying on substring checks against `ty`.
    pub ty_ast: syn::Type,
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

// ─── Skip records (OF-001) ────────────────────────────────────────────────────

/// A `pub fn` in an API source file that the parser silently dropped.
///
/// The parser only accepts public functions whose first parameter contains the
/// configured `state_type` (or `store_type`) as a substring. Functions that
/// fail this check, take `self`/`&self`, or take no parameters at all are
/// dropped from the generated output. `SkipRecord` makes those drops visible
/// so the build can emit a `cargo:warning=...` line per occurrence.
#[derive(Debug, Clone)]
pub struct SkipRecord {
    /// Source file the function was declared in.
    pub file: PathBuf,
    /// Function name (`fn <name>(...)`).
    pub fn_name: String,
    /// Why this function was dropped.
    pub reason: SkipReason,
}

/// The reason `parse_api_module` dropped a `pub fn`.
#[derive(Debug, Clone)]
pub enum SkipReason {
    /// First parameter's normalized type didn't contain `state_type` or
    /// `store_type` as a substring.
    FirstParamMismatch {
        /// Normalized first-param type string the parser checked.
        first_param_ty: String,
        /// `state_type` that was searched for.
        state_type: String,
        /// `store_type` that was searched for, if any.
        store_type: Option<String>,
    },
    /// First parameter was `self` or `&self`. Free-function API modules can't
    /// host method-shaped signatures.
    SelfReceiver,
    /// Function had no parameters at all. There's no first parameter to match
    /// against `state_type` / `store_type`.
    NoParams,
}

impl std::fmt::Display for SkipRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file = self.file.display();
        let name = &self.fn_name;
        match &self.reason {
            SkipReason::FirstParamMismatch { first_param_ty, state_type, store_type } => {
                let st = match store_type {
                    Some(s) => format!(" or store_type '{s}'"),
                    None => String::new(),
                };
                write!(
                    f,
                    "ontogen: skipped fn `{name}` in `{file}` - first param `{first_param_ty}` does not match state_type '{state_type}'{st}",
                )
            }
            SkipReason::SelfReceiver => {
                write!(f, "ontogen: skipped fn `{name}` in `{file}` - first param is `self`/`&self`")
            }
            SkipReason::NoParams => {
                write!(f, "ontogen: skipped fn `{name}` in `{file}` - fn has no parameters")
            }
        }
    }
}

/// Result of parsing a single API source file.
///
/// `module` is `None` when the file was filtered before reaching the function
/// loop (e.g. `mod.rs`, unreadable, unparseable). `skips` is populated for any
/// `pub fn` the loop dropped.
#[derive(Debug, Default)]
pub struct ModuleParseResult {
    pub module: Option<ApiModule>,
    pub skips: Vec<SkipRecord>,
}

/// Aggregated result of scanning a directory of API source files.
///
/// `modules` contains the parsed `ApiModule`s with kept functions or events.
/// `skips` is the union of every per-file skip across the scan.
#[derive(Debug, Default)]
pub struct ScanResult {
    pub modules: Vec<ApiModule>,
    pub skips: Vec<SkipRecord>,
}

// ─── Parsing ──────────────────────────────────────────────────────────────────

/// Returns true when `source` has a file-level skip marker in its leading
/// comment-and-attribute block.
///
/// The marker is matched anywhere in the run of blank lines, line comments
/// (`//` / `///` / `//!`), and inner attributes (`#![...]`) that prefixes the
/// file. Once a non-comment / non-attribute item is reached, later occurrences
/// of the marker are ignored — opt-out is a file-level decision, not something
/// that should be smuggled in mid-file.
///
/// Two grammars are honoured, both requiring exact trimmed equality:
/// - `// ontogen:skip` (plain line comment)
/// - `//! ontogen:skip` (inner doc comment)
fn has_skip_marker(source: &str) -> bool {
    source
        .lines()
        .take_while(|line| {
            let t = line.trim_start();
            t.is_empty() || t.starts_with("//") || t.starts_with("#!")
        })
        .any(|line| {
            let t = line.trim();
            t == "// ontogen:skip" || t == "//! ontogen:skip"
        })
}

/// Parse a single API source file into a `ModuleParseResult`.
///
/// `module` is populated when the file holds at least one accepted function or
/// event. `skips` records any `pub fn` that was silently dropped — see
/// [`SkipReason`] for the categories.
///
/// Files named `mod.rs` and files that fail to read or parse return
/// `ModuleParseResult::default()` (empty module, empty skips).
///
/// Files that opt out of scanning via a `// ontogen:skip` or
/// `//! ontogen:skip` marker in the leading comment-and-attribute block also
/// return `ModuleParseResult::default()`: the file is not represented in
/// [`ScanResult::modules`] and no [`SkipRecord`] is emitted for any `pub fn`
/// inside (opt-out is intentional, so silencing the per-fn warnings is the
/// whole point of the marker).
pub fn parse_api_module(path: &Path, state_type: &str, store_type: Option<&str>) -> ModuleParseResult {
    let mut result = ModuleParseResult::default();

    let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return result;
    };
    if file_stem == "mod" {
        return result;
    }

    let Ok(source) = fs::read_to_string(path) else {
        return result;
    };
    if has_skip_marker(&source) {
        return result;
    }
    let Ok(syntax) = syn::parse_file(&source) else {
        return result;
    };

    let mut functions = Vec::new();
    let mut events = Vec::new();
    for item in &syntax.items {
        if let syn::Item::Fn(func) = item {
            if !matches!(func.vis, Visibility::Public(_)) {
                continue;
            }

            // Inspect first param. Three drop cases produce a SkipRecord;
            // the accepting case sets `is_store` (false for state-scoped,
            // true for store-scoped) and falls through.
            let is_store = match func.sig.inputs.first() {
                None => {
                    result.skips.push(SkipRecord {
                        file: path.to_path_buf(),
                        fn_name: func.sig.ident.to_string(),
                        reason: SkipReason::NoParams,
                    });
                    continue;
                }
                Some(FnArg::Receiver(_)) => {
                    result.skips.push(SkipRecord {
                        file: path.to_path_buf(),
                        fn_name: func.sig.ident.to_string(),
                        reason: SkipReason::SelfReceiver,
                    });
                    continue;
                }
                Some(FnArg::Typed(pat)) => {
                    let ty = norm_type(&pat.ty);
                    if ty.contains(state_type) {
                        false
                    } else if let Some(st) = store_type
                        && ty.contains(st)
                    {
                        true
                    } else {
                        result.skips.push(SkipRecord {
                            file: path.to_path_buf(),
                            fn_name: func.sig.ident.to_string(),
                            reason: SkipReason::FirstParamMismatch {
                                first_param_ty: ty,
                                state_type: state_type.to_string(),
                                store_type: store_type.map(String::from),
                            },
                        });
                        continue;
                    }
                }
            };

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
                        let ty_ast = (*pat.ty).clone();
                        let name = match pat.pat.as_ref() {
                            syn::Pat::Ident(ident) => ident.ident.to_string(),
                            _ => String::new(),
                        };
                        Some(Param { name, ty, ty_ast })
                    } else {
                        None
                    }
                })
                .collect();

            let (return_type, return_type_ast) = extract_result_ok_type(&func.sig.output);

            functions.push(ApiFn {
                name: func.sig.ident.to_string(),
                is_async: func.sig.asyncness.is_some(),
                doc,
                params,
                return_type,
                return_type_ast,
                first_param_is_store: is_store,
            });
        }
    }

    result.module = Some(ApiModule { name: file_stem.to_string(), functions, events });
    result
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

/// Extract `T` from `Result<T, E>` as both a normalized string and AST.
///
/// When the return type is not a `Result<...>`, returns `("()", syn::Type::Tuple(_))`
/// for the unit type.
fn extract_result_ok_type(ret: &ReturnType) -> (String, Type) {
    if let ReturnType::Type(_, ty) = ret
        && let Type::Path(tp) = ty.as_ref()
    {
        let seg = tp.path.segments.last().unwrap();
        if seg.ident == "Result"
            && let PathArguments::AngleBracketed(args) = &seg.arguments
            && let Some(GenericArgument::Type(t)) = args.args.first()
        {
            return (norm_type(t), t.clone());
        }
    }
    ("()".to_string(), syn::parse_quote!(()))
}

/// Scan a directory for API source files and parse them all.
///
/// Skips files ending in `_impl.rs` and `mod.rs`. The returned `ScanResult`
/// carries both the parsed modules (only those with at least one accepted
/// function or event) and the union of every per-file [`SkipRecord`] so a
/// caller can surface skipped functions through `cargo:warning=`.
pub fn scan_api_dir(api_dir: &Path, state_type: &str, store_type: Option<&str>) -> ScanResult {
    let mut result = ScanResult::default();

    // Collect .rs files from api_dir and its immediate subdirectories (e.g. generated/)
    let mut entries: Vec<_> = collect_rs_files(api_dir);
    entries.sort();

    for path in entries {
        let parsed = parse_api_module(&path, state_type, store_type);
        result.skips.extend(parsed.skips);
        if let Some(m) = parsed.module
            && (!m.functions.is_empty() || !m.events.is_empty())
        {
            result.modules.push(m);
        }
    }

    result
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
