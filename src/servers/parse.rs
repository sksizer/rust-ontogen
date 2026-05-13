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
    /// Whether the function was marked `#[ontogen::stateless]`.
    ///
    /// Stateless functions take no state/store parameter and are emitted
    /// with handler shapes that omit the `State<...>` extractor and the
    /// positional state/store forward. `params` then contains every
    /// declared input rather than skipping a leading state argument.
    pub is_stateless: bool,
    /// Optional override for the emitted IPC command / TS method name.
    ///
    /// Populated by either the source-side `#[ontogen(rename = "...")]`
    /// attribute or by [`NamingConfig::command_overrides`](crate::servers::types::NamingConfig::command_overrides).
    /// When `Some`, the IPC generator uses this value verbatim (and the TS
    /// client camel-cases it). When `None`, the default
    /// `{entity}_{fn_name}` scheme applies.
    ///
    /// Precedence: if the source attribute is present, it wins; the config
    /// map only fills in entries that were absent on the source side.
    pub command_override: Option<String>,
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
    /// True when this module represents a singleton (a single entity, not a
    /// collection — `database`, `autostart`, `vault`, …).
    ///
    /// The flag is set from either a source-side `// ontogen:singleton` /
    /// `//! ontogen:singleton` marker in the file's leading
    /// comment-and-attribute block (parser side), or from
    /// [`NamingConfig::singleton_modules`](crate::servers::NamingConfig) via
    /// the post-parse [`apply_singleton_overlay`] step. Downstream generators
    /// (HTTP today; admin / doc-gen in the future) branch on this rather than
    /// re-deriving from naming rules.
    pub is_singleton: bool,
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
    /// Function carried `#[ontogen(rename = ...)]` with a non-string-literal
    /// value (e.g., `rename = 42`). Dropping the function makes the mistake
    /// visible at build time rather than silently falling back to the default
    /// `{entity}_{fn}` command name.
    InvalidRenameValue,
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
                    "ontogen: skipped fn `{name}` in `{file}` - first param `{first_param_ty}` does not match state_type '{state_type}'{st}; add `#[ontogen::stateless]` if this fn intentionally takes no state",
                )
            }
            SkipReason::SelfReceiver => {
                write!(f, "ontogen: skipped fn `{name}` in `{file}` - first param is `self`/`&self`")
            }
            SkipReason::NoParams => {
                write!(
                    f,
                    "ontogen: skipped fn `{name}` in `{file}` - fn has no parameters; add `#[ontogen::stateless]` if this fn intentionally takes no state",
                )
            }
            SkipReason::InvalidRenameValue => {
                write!(
                    f,
                    "ontogen: skipped fn `{name}` in `{file}` - `#[ontogen(rename = ...)]` value must be a string literal"
                )
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

/// Returns true when `source` carries a file-level marker `name` (e.g. `skip`,
/// `singleton`) in its leading comment-and-attribute block.
///
/// The marker is matched anywhere in the run of blank lines, line comments
/// (`//` / `///` / `//!`), and inner attributes (`#![...]`) that prefixes the
/// file. Once a non-comment / non-attribute item is reached, later occurrences
/// of the marker are ignored — file-level markers are file-level decisions,
/// not something that should be smuggled in mid-file.
///
/// Two grammars are honoured per marker, both requiring exact trimmed equality:
/// - `// ontogen:<name>` (plain line comment)
/// - `//! ontogen:<name>` (inner doc comment, including inside a multi-line
///   `//!` block)
fn has_top_level_marker(source: &str, name: &str) -> bool {
    let line_form = format!("// ontogen:{name}");
    let doc_form = format!("//! ontogen:{name}");
    source
        .lines()
        .take_while(|line| {
            let t = line.trim_start();
            t.is_empty() || t.starts_with("//") || t.starts_with("#!")
        })
        .any(|line| {
            let t = line.trim();
            t == line_form || t == doc_form
        })
}

/// Returns true when `source` has a file-level skip marker
/// (`// ontogen:skip` / `//! ontogen:skip`) in its leading
/// comment-and-attribute block. See [`has_top_level_marker`] for the
/// placement rule.
fn has_skip_marker(source: &str) -> bool {
    has_top_level_marker(source, "skip")
}

/// Returns true when `source` has a file-level singleton marker
/// (`// ontogen:singleton` / `//! ontogen:singleton`) in its leading
/// comment-and-attribute block. See [`has_top_level_marker`] for the
/// placement rule.
fn has_singleton_marker(source: &str) -> bool {
    has_top_level_marker(source, "singleton")
}

/// Returns true when any attribute on `func` is the `stateless` attribute
/// from `ontogen-macros` — matched by the final path segment ident.
///
/// Accepts `#[stateless]`, `#[ontogen::stateless]`, or any other path that
/// ends in `::stateless`. The match is purely syntactic; the proc-macro
/// itself is a no-op pass-through, so the parser is the only consumer of
/// the marker. A foreign `stateless` attribute from an unrelated crate
/// would also match; users hitting that collision should rename the
/// foreign attribute or omit it from API modules.
fn has_stateless_attr(func: &syn::ItemFn) -> bool {
    func.attrs.iter().any(|attr| {
        matches!(attr.meta, syn::Meta::Path(_) | syn::Meta::List(_))
            && attr.path().segments.last().is_some_and(|seg| seg.ident == "stateless")
    })
}

/// Parse a single API source file into a `ModuleParseResult`.
///
/// `module` is populated when the file holds at least one accepted function or
/// event. `skips` records any `pub fn` that was silently dropped — see
/// [`SkipReason`] for the categories.
///
/// Functions annotated with `#[ontogen::stateless]` bypass the first-param
/// state/store check entirely and are included with `ApiFn::is_stateless`
/// set, so downstream generators can emit handler shapes without the
/// `State<...>` extractor or any positional state forward. `self`/`&self`
/// receivers are still rejected — stateless or not, method signatures
/// don't fit free-function API modules.
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
    let is_singleton = has_singleton_marker(&source);
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

            let is_stateless = has_stateless_attr(func);

            // For state-bearing fns, inspect the first param. Three drop cases
            // produce a SkipRecord; the accepting case sets `is_store` (false
            // for state-scoped, true for store-scoped) and falls through.
            //
            // For `#[ontogen::stateless]` fns the first-param check is
            // bypassed: zero params is fine, any param shape is fine. The
            // only retained guard is `self`/`&self`, since free-function API
            // modules can't host method signatures regardless of state.
            let is_store = if is_stateless {
                if let Some(FnArg::Receiver(_)) = func.sig.inputs.first() {
                    result.skips.push(SkipRecord {
                        file: path.to_path_buf(),
                        fn_name: func.sig.ident.to_string(),
                        reason: SkipReason::SelfReceiver,
                    });
                    continue;
                }
                false
            } else {
                match func.sig.inputs.first() {
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

            // State-bearing fns: skip the leading state/store param.
            // Stateless fns: keep every declared input — there's no state arg
            // to drop.
            let skip_first = if is_stateless { 0 } else { 1 };

            // Parse the per-function `#[ontogen(...)]` attribute, if present.
            // Today only `rename = "..."` is interpreted. A malformed value
            // (e.g., a non-string literal) drops the function entirely so the
            // mistake is visible at build time rather than silently falling
            // back to the default command name.
            let fn_ident = func.sig.ident.to_string();
            let command_override = match parse_ontogen_rename(&func.attrs) {
                OntogenAttr::None | OntogenAttr::OtherDirective => None,
                OntogenAttr::Rename(value) => Some(value),
                OntogenAttr::InvalidValue => {
                    result.skips.push(SkipRecord {
                        file: path.to_path_buf(),
                        fn_name: fn_ident,
                        reason: SkipReason::InvalidRenameValue,
                    });
                    continue;
                }
            };

            let params: Vec<Param> = func
                .sig
                .inputs
                .iter()
                .skip(skip_first)
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
                name: fn_ident,
                is_async: func.sig.asyncness.is_some(),
                doc,
                params,
                return_type,
                return_type_ast,
                first_param_is_store: is_store,
                is_stateless,
                command_override,
            });
        }
    }

    result.module = Some(ApiModule { name: file_stem.to_string(), functions, events, is_singleton });
    result
}

/// OR a config-side singleton declaration onto each parsed [`ApiModule`].
///
/// The parser only sees the source-side marker, because it has no access to
/// [`NamingConfig`](crate::servers::NamingConfig). This overlay merges the
/// `naming.singleton_modules` set in after-the-fact so the IR reaches every
/// downstream generator with the effective bit set. If either side flagged the
/// module, it stays a singleton (no double-effect — just a logical OR).
pub fn apply_singleton_overlay(modules: &mut [ApiModule], naming: &crate::servers::types::NamingConfig) {
    for m in modules {
        if naming.singleton_modules.contains(&m.name) {
            m.is_singleton = true;
        }
    }
}

/// Apply per-function command-name overrides from
/// [`NamingConfig::command_overrides`](crate::servers::types::NamingConfig)
/// onto parsed [`ApiModule`]s.
///
/// Keys are `"module::fn_name"`. Source-side `#[ontogen(rename = "...")]`
/// attributes always win: if [`ApiFn::command_override`] is already `Some`,
/// the config entry is silently ignored. The config map is treated as an
/// escape hatch for cases where the source can't be modified.
pub fn apply_command_overrides(modules: &mut [ApiModule], naming: &crate::servers::types::NamingConfig) {
    if naming.command_overrides.is_empty() {
        return;
    }
    for m in modules.iter_mut() {
        for f in &mut m.functions {
            if f.command_override.is_some() {
                continue;
            }
            let key = format!("{}::{}", m.name, f.name);
            if let Some(value) = naming.command_overrides.get(&key) {
                f.command_override = Some(value.clone());
            }
        }
    }
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

/// Result of looking for the `#[ontogen(...)]` attribute on a function.
#[derive(Debug)]
enum OntogenAttr {
    /// No `#[ontogen(...)]` attribute is present.
    None,
    /// `#[ontogen(rename = "value")]` with a valid string literal.
    Rename(String),
    /// `#[ontogen(...)]` is present but contains directives we do not yet
    /// recognize (e.g., a future `stateless` marker). Today this is treated
    /// the same as `None` for naming purposes.
    OtherDirective,
    /// `#[ontogen(rename = ...)]` is present but the value is not a string
    /// literal. The caller should drop the function and surface a diagnostic.
    InvalidValue,
}

/// Walk `attrs` looking for `#[ontogen(rename = "...")]`.
///
/// This is intentionally lenient about unknown directives so the umbrella
/// `#[ontogen(...)]` attribute can host future per-function flags without
/// breaking older versions. Only the `rename` arm has strict validation: a
/// non-string-literal value yields [`OntogenAttr::InvalidValue`] so the parser
/// can drop the function.
fn parse_ontogen_rename(attrs: &[syn::Attribute]) -> OntogenAttr {
    use syn::{Expr, ExprLit, Lit, Meta};

    let mut result = OntogenAttr::None;

    for attr in attrs {
        if !attr.path().is_ident("ontogen") {
            continue;
        }

        // Expect `#[ontogen(<nested>)]`. Anything else (e.g., `#[ontogen]`
        // or `#[ontogen = "..."]`) is treated as an unknown directive.
        let list = match &attr.meta {
            Meta::List(list) => list,
            _ => {
                if matches!(result, OntogenAttr::None) {
                    result = OntogenAttr::OtherDirective;
                }
                continue;
            }
        };

        // Parse the nested meta list (`rename = "...", other = ...`).
        let parsed = list.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated);
        let nested = match parsed {
            Ok(n) => n,
            Err(_) => return OntogenAttr::InvalidValue,
        };

        for meta in nested {
            if let Meta::NameValue(nv) = &meta
                && nv.path.is_ident("rename")
            {
                match &nv.value {
                    Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => {
                        result = OntogenAttr::Rename(s.value());
                    }
                    _ => return OntogenAttr::InvalidValue,
                }
            } else if matches!(result, OntogenAttr::None) {
                // Unknown directive - leave the field untouched but mark
                // that the attribute existed so the caller can distinguish
                // "no attribute" from "attribute with future directive".
                result = OntogenAttr::OtherDirective;
            }
        }
    }

    result
}
