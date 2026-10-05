#![allow(clippy::doc_markdown, clippy::manual_let_else, clippy::module_name_repetitions)]

//! Parse Rust service API source files into structured metadata.
//!
//! Extracts function signatures, parameters, return types, and event functions
//! from files where public functions take a `&{StateType}` as their first parameter.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use syn::{FnArg, GenericArgument, PathArguments, ReturnType, Type, Visibility};

use crate::servers::config::ApiSurface;
use crate::servers::types::{collect_type_import, norm_type};

// ─── Extracted function metadata ──────────────────────────────────────────────

/// Which HTTP method an `#[ontogen::http::*]` attribute forces on a handler.
///
/// `Post` and `Get` are consumed today (via `#[ontogen::http::post]` and
/// `#[ontogen::http::get]`). Further method overrides extend the variant set
/// without touching `ApiFn`'s shape — adding `Put` / `Delete` / `Patch` is
/// purely additive once their consumers and corresponding proc-macros land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedMethod {
    /// Force `OpKind::CustomPost` classification regardless of name or
    /// param shape. Populated from any attribute whose final path segment
    /// is `post` (so `#[ontogen::http::post]`, `#[post]` after a
    /// `use ontogen::http::post;`, and any other path ending in `::post`
    /// all map here).
    Post,
    /// Force `OpKind::CustomGet` classification regardless of name or param
    /// shape. Populated from any attribute whose final path segment is `get`.
    ///
    /// The counterpart to `Post`, and the escape hatch for the asymmetry in
    /// the name heuristic: [`KNOWN_READ_PREFIXES`] is consulted **only** for
    /// functions with no user-facing params, and among functions that do have
    /// params only `get_` has a branch back into `CustomGet`. So a read with
    /// arguments named `count_*`, `exists_*`, `find_*`, `is_*` or `has_*`
    /// routes `CustomPost` however read-only it is, and renaming it to `get_*`
    /// was the only way back. This is the other way back.
    ///
    /// Forcing GET is the author asserting the param shape suits a GET — the
    /// override wins outright, exactly as `Post` does, and does not re-run the
    /// body-carrying-first-param check that the `get_*` branch applies.
    ///
    /// [`KNOWN_READ_PREFIXES`]: crate::servers::classify
    Get,
}

/// A parsed API function signature.
///
/// `Default` is provided so test fixtures can use `..Default::default()`
/// to opt out of naming every field — see the unit tests in
/// `src/servers/tests.rs`. The real parser path in this file populates
/// every field explicitly and never relies on the default. Default values
/// model an empty stateful fn (`name: ""`, `is_async: false`, no params,
/// `return_type: "()"`, `force_method: None`, no command override).
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
    /// The `E` from `Result<T, E>`, as a path resolved through the file's
    /// `use` items (see `resolve_through_uses`). `None` when the return type
    /// is not a two-argument `Result`. It is only ever compared as a path,
    /// so no AST is kept.
    pub error_type: Option<String>,
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
    /// HTTP method to force on the emitted route, if the source carried
    /// an `#[ontogen::http::*]` attribute.
    ///
    /// When `Some(ForcedMethod::Post)`, the classifier returns
    /// `OpKind::CustomPost` unconditionally, overriding the name/param
    /// heuristic. This is the user-driven escape hatch for action-verb
    /// functions whose zero-user-param shape would otherwise route as GET
    /// (e.g. `pause(state)`, `resume(state)`, `reset_all(state)`).
    ///
    /// The enum exists so future HTTP-method overrides
    /// (`#[ontogen::http::get]`, `#[ontogen::http::put]`, etc.) can extend
    /// the variant set without changing `ApiFn`'s shape. A `#[…::get]`
    /// override is anticipated by the companion classifier-reverse-default
    /// task — when the zero-param default flips to POST, any false-positive
    /// reads will need an explicit GET opt-in.
    pub force_method: Option<ForcedMethod>,
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
    /// Index of the [`ApiSurface`](crate::servers::ApiSurface) this function
    /// was scanned from: `0` is the primary surface.
    pub surface: usize,
    /// State method a store-scoped handler calls to obtain the store
    /// (`state.{store_accessor}().await`). Copied from the surface.
    pub store_accessor: String,
}

/// A single function parameter.
///
/// `Default` is provided so test fixtures can use `..Default::default()`
/// — the real parser path always populates every field explicitly.
/// `ty_ast` defaults to the unit type (`()`) because `syn::Type` does
/// not impl `Default` directly.
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

impl Default for ApiFn {
    fn default() -> Self {
        Self {
            name: String::new(),
            is_async: false,
            doc: String::new(),
            params: Vec::new(),
            return_type: "()".to_string(),
            // `syn::Type` does not impl `Default`; the unit type is the
            // most neutral stand-in and matches what `extract_result_types`
            // produces for fns with no `Result<_, _>` return.
            return_type_ast: syn::parse_quote!(()),
            error_type: None,
            first_param_is_store: false,
            is_stateless: false,
            force_method: None,
            command_override: None,
            surface: 0,
            store_accessor: crate::servers::config::DEFAULT_STORE_ACCESSOR.to_string(),
        }
    }
}

impl Default for Param {
    fn default() -> Self {
        Self {
            name: String::new(),
            ty: "()".to_string(),
            // See `ApiFn`'s Default impl for why we hand-write this instead
            // of deriving — `syn::Type` does not impl `Default`.
            ty_ast: syn::parse_quote!(()),
        }
    }
}

impl Param {
    /// True for an `Option<…>` argument, which a caller may leave out.
    pub fn is_option(&self) -> bool {
        self.ty.starts_with("Option<")
    }

    /// True for an `*Input` argument: the type it names, under any `&` and
    /// one `Option`, is a single path whose last segment ends in `Input`
    /// (`CreateTaskInput`, `&crate::schema::UpdateTaskInput`). A type that
    /// only contains the word (`InputMode`, `Vec<TaskInput>`) is not one.
    pub fn is_input(&self) -> bool {
        let unref = |ty: &str| ty.trim_start_matches('&').trim_start_matches("mut ").to_string();
        let mut ty = unref(&self.ty);
        if let Some(inner) = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
            ty = unref(inner);
        }
        let name = ty.rsplit("::").next().unwrap_or(&ty);
        !name.contains(['<', '(', '[']) && name.ends_with("Input")
    }

    /// True for a list's `*Query` filter struct: the type it names, under any
    /// `&` and one `Option`, is a single path whose last segment ends in
    /// `Query` (`ListTasksQuery`, `crate::api::TaskQuery`). Each of its fields
    /// is one `filter[…]` member on HTTP (wire contract §7.3).
    pub fn is_filter_struct(&self) -> bool {
        let unref = |ty: &str| ty.trim_start_matches('&').trim_start_matches("mut ").to_string();
        let mut ty = unref(&self.ty);
        if let Some(inner) = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
            ty = unref(inner);
        }
        let name = ty.rsplit("::").next().unwrap_or(&ty);
        !name.contains(['<', '(', '[']) && name.ends_with("Query")
    }

    /// For an order parameter, `&[OrderBy<F>]`, the last path segment of
    /// `F` (`TaskSortField`). `OrderBy` and `F` may be path-qualified
    /// (`&[ontogen_core::order::OrderBy<crate::store::task::TaskSortField>]`).
    /// The shape is checked on `ty_ast`; the name is sliced from `ty`, its
    /// normalized form.
    pub fn order_sort_field(&self) -> Option<&str> {
        let syn::Type::Reference(reference) = &self.ty_ast else { return None };
        if reference.mutability.is_some() {
            return None;
        }
        let syn::Type::Slice(slice) = &*reference.elem else { return None };
        let syn::Type::Path(outer) = &*slice.elem else { return None };
        let order_by = outer.path.segments.last().filter(|s| outer.qself.is_none() && s.ident == "OrderBy")?;
        let syn::PathArguments::AngleBracketed(args) = &order_by.arguments else { return None };
        let mut args = args.args.iter();
        let (Some(syn::GenericArgument::Type(syn::Type::Path(field))), None) = (args.next(), args.next()) else {
            return None;
        };
        let field_segment = field.path.segments.last().filter(|s| field.qself.is_none() && s.arguments.is_none())?;
        let ident = field_segment.ident.to_string();
        let name = self.ty.strip_suffix(">]")?.rsplit([':', '<']).next()?;
        (name == ident).then_some(name)
    }
}

/// An event function: returns `broadcast::Receiver<T>` or
/// `Result<broadcast::Receiver<T>, E>`, sync or async.
///
/// Parameters after the state are subscription arguments. A parameter named
/// [`RESUME_PARAM`] of type `Option<String>` makes the op resumable: the SSE
/// handler fills it from `Last-Event-ID`, and the item type must implement
/// `ontogen_core::events::EventSeq`.
#[derive(Debug, Clone)]
pub struct EventFn {
    /// Function name (e.g., `graph_updated`).
    pub name: String,
    /// Doc comment text (joined from `///` lines).
    pub doc: String,
    /// Whether the function is async.
    pub is_async: bool,
    /// Parameters after the state parameter.
    pub params: Vec<Param>,
    /// The `T` of the returned `Receiver<T>`, normalized.
    pub item_type: String,
    /// The `T` of the returned `Receiver<T>`, as an AST.
    pub item_type_ast: syn::Type,
    /// Whether the receiver comes wrapped in a `Result`. A failed subscribe
    /// is an HTTP error response or a rejected IPC command.
    pub returns_result: bool,
    /// The `E` of a `Result<Receiver<T>, E>`, resolved as
    /// [`ApiFn::error_type`] is.
    pub error_type: Option<String>,
    /// Index of the surface this event was scanned from; see [`ApiFn::surface`].
    pub surface: usize,
}

impl Default for EventFn {
    fn default() -> Self {
        Self {
            name: String::new(),
            doc: String::new(),
            is_async: false,
            params: Vec::new(),
            item_type: "()".to_string(),
            item_type_ast: syn::parse_quote!(()),
            returns_result: false,
            error_type: None,
            surface: 0,
        }
    }
}

/// The parameter name that makes an event op resumable.
pub const RESUME_PARAM: &str = "resume";

/// True for an event fn's `resume: Option<String>` parameter.
pub fn is_resume_param(p: &Param) -> bool {
    p.name == RESUME_PARAM && p.ty == "Option<String>"
}

impl EventFn {
    /// True when the fn declares `resume: Option<String>`.
    pub fn is_resumable(&self) -> bool {
        self.params.iter().any(is_resume_param)
    }

    /// Required parameters: path segments on the SSE route, as a `CustomGet`
    /// takes them.
    pub fn path_params(&self) -> Vec<&Param> {
        self.params.iter().filter(|p| !p.ty.starts_with("Option<")).collect()
    }

    /// Optional parameters, `resume` included: query parameters on the SSE route.
    pub fn query_params(&self) -> Vec<&Param> {
        self.params.iter().filter(|p| p.ty.starts_with("Option<")).collect()
    }

    /// True for the shape that predates event parameters: sync, infallible,
    /// no parameters. Only this shape gets the global IPC forwarding and the
    /// TS `onX(callback)` methods.
    pub fn is_legacy(&self) -> bool {
        self.params.is_empty() && !self.is_async && !self.returns_result
    }

    /// The unscoped SSE route in `:name` form: the override from
    /// `sse_route_overrides`, or `/api/events/{event-name}`, followed by any
    /// path param the route does not already name.
    pub fn sse_route(&self, overrides: &HashMap<String, String>) -> String {
        let mut path = overrides
            .get(&self.name)
            .cloned()
            .unwrap_or_else(|| format!("/api/events/{}", crate::servers::types::event_name(&self.name)));
        for p in self.path_params() {
            let segment = format!(":{}", p.name);
            if !path.split('/').any(|s| s == segment) {
                path.push('/');
                path.push_str(&segment);
            }
        }
        path
    }

    /// The route-prefixed SSE route in `:name` form, for `segments` such as
    /// `projects/:project_id`.
    pub fn sse_route_scoped(&self, overrides: &HashMap<String, String>, segments: &str) -> String {
        let route = self.sse_route(overrides);
        match route.strip_prefix("/api/") {
            Some(rest) => format!("/api/{segments}/{rest}"),
            None => format!("/api/{segments}{route}"),
        }
    }
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
    /// the post-parse `apply_singleton_overlay` step. Downstream generators
    /// (HTTP today; admin / doc-gen in the future) branch on this rather than
    /// re-deriving from naming rules.
    pub is_singleton: bool,
    /// True when the module defines `count(store) -> Result<u64, _>`: the
    /// total behind a paginated `list`. It stays an operation of its own on
    /// `functions` until [`check_paginated_lists`] finds the module paginated,
    /// at which point the generated page handlers call it and it is taken off.
    pub has_count: bool,
}

/// The parameter names a paginated `list` ends with.
pub const PAGE_PARAMS: [&str; 2] = ["limit", "offset"];

/// The type each page parameter must have.
pub const PAGE_PARAM_TYPE: &str = "Option<u64>";

/// The function names that make up a module's CRUD surface.
pub const CRUD_FN_NAMES: [&str; 5] = ["list", "get_by_id", "create", "update", "delete"];

impl ApiFn {
    /// True when the fn ends with `limit: Option<u64>, offset: Option<u64>`:
    /// the shape a paginated `list` must have for the page to reach the store.
    pub fn takes_page(&self) -> bool {
        let n = self.params.len();
        n >= 2
            && PAGE_PARAMS.iter().zip(&self.params[n - 2..]).all(|(name, p)| p.name == *name && p.ty == PAGE_PARAM_TYPE)
    }

    /// True for the `count` that backs a paginated `list`. It takes the store
    /// or the state, then whatever filter the list takes — `check_paginated_lists`
    /// is what holds the two parameter lists to each other.
    pub fn is_count(&self) -> bool {
        self.name == "count" && !self.is_stateless
    }

    /// The page parameters ([`PAGE_PARAMS`]) when the fn takes a page, else
    /// none.
    pub fn page(&self) -> &[Param] {
        let n = self.params.len();
        if self.takes_page() { &self.params[n - PAGE_PARAMS.len()..] } else { &[] }
    }

    /// The parameter a `list` takes its order in: the first whose type is
    /// `&[OrderBy<F>]` ([`Param::order_sort_field`]), whatever its name.
    pub fn order_param(&self) -> Option<&Param> {
        self.params.iter().find(|p| p.order_sort_field().is_some())
    }

    /// True when this function takes an order ([`Self::order_param`]).
    pub fn takes_order(&self) -> bool {
        self.order_param().is_some()
    }

    /// The entity the order parameter sorts: its `{Entity}SortField` type's
    /// last path segment without `SortField` (`Task`). `None` without an
    /// order parameter, or when the type is not named `{Entity}SortField`.
    pub fn sort_entity(&self) -> Option<&str> {
        let field = self.order_param()?.order_sort_field()?;
        field.strip_suffix("SortField").filter(|entity| !entity.is_empty())
    }

    /// This function's filter parameters, as `name: type` — the filter a
    /// paginated `list` applies and its `count` must apply too.
    pub fn filter_params(&self) -> Vec<String> {
        self.filter().iter().map(|p| format!("{}: {}", p.name, p.ty)).collect()
    }

    /// This function's parameters other than the page and the order: the
    /// filter of a `list` (wire contract §7.3), in declaration order.
    pub fn filter(&self) -> Vec<&Param> {
        let n = self.params.len();
        let end = if self.takes_page() { n - PAGE_PARAMS.len() } else { n };
        self.params[..end].iter().filter(|p| p.order_sort_field().is_none()).collect()
    }

    /// True when this function, a `list`, takes a filter: any parameter but
    /// its page and its order.
    pub fn takes_filter(&self) -> bool {
        !self.filter().is_empty()
    }

    /// The list's `*Query` filter struct ([`Param::is_filter_struct`]), if it
    /// takes one.
    pub fn filter_struct(&self) -> Option<&Param> {
        self.filter().into_iter().find(|p| p.is_filter_struct())
    }

    /// The list's bare filter parameters (`skill_id: &str`): every filter
    /// parameter that is not a `*Query` struct, each one `filter[{name}]` on
    /// HTTP.
    pub fn bare_filters(&self) -> Vec<&Param> {
        self.filter().into_iter().filter(|p| !p.is_filter_struct()).collect()
    }
}

impl ApiModule {
    /// Returns true if this module has a complete CRUD surface
    /// (list, get_by_id, create, update, delete).
    pub fn is_crud(&self) -> bool {
        CRUD_FN_NAMES.iter().all(|name| self.functions.iter().any(|f| f.name == *name))
    }

    /// The lowest surface index any of this module's functions or events came
    /// from. That surface's service module is imported under the module's own
    /// name; the same-named module of every other surface is aliased.
    pub fn base_surface(&self) -> usize {
        self.functions.iter().map(|f| f.surface).chain(self.events.iter().map(|e| e.surface)).min().unwrap_or(0)
    }

    /// The identifier generated handlers call this module's functions
    /// through, for a function scanned from `surface`, written as Rust: the
    /// module name for the base surface (`r#match` for `match`),
    /// `{name}_{surface}` for any other.
    pub fn service_ident(&self, surface: usize) -> String {
        if surface == self.base_surface() {
            crate::ident::rust_ident(&self.name)
        } else {
            format!("{}_{}", self.name, surface)
        }
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

/// Inspect attributes on `func` and return the forced HTTP method, if any
/// `#[ontogen::http::*]` marker is present.
///
/// Matched on the final path segment of each attribute path. The recognized
/// forms are `post` → `Some(ForcedMethod::Post)` and `get` →
/// `Some(ForcedMethod::Get)`, each accepting the canonical
/// `#[ontogen::http::<method>]`, the bare `#[<method>]` (after
/// `use ontogen::http::<method>;`), or any other path ending in
/// `::<method>`. The
/// match is purely syntactic; the proc-macro itself is a no-op pass-through,
/// so the parser is the only consumer of the marker. A foreign `post`
/// attribute from an unrelated crate would also match; users hitting that
/// collision should rename the foreign attribute or omit it from API
/// modules.
///
/// Returns `None` when no recognized HTTP-method-override attribute is
/// present. Further method overrides extend this function by adding a
/// match arm for each new ident (`put` → `Some(ForcedMethod::Put)`, etc.).
fn parse_force_method(func: &syn::ItemFn) -> Option<ForcedMethod> {
    for attr in &func.attrs {
        if !matches!(attr.meta, syn::Meta::Path(_) | syn::Meta::List(_)) {
            continue;
        }
        let Some(last) = attr.path().segments.last() else {
            continue;
        };
        if last.ident == "post" {
            return Some(ForcedMethod::Post);
        }
        if last.ident == "get" {
            return Some(ForcedMethod::Get);
        }
    }
    None
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
    let mut has_count = false;
    let Ok(syntax) = syn::parse_file(&source) else {
        return result;
    };

    let uses = use_bindings(&syntax.items);
    let mut functions = Vec::new();
    let mut events = Vec::new();
    for item in &syntax.items {
        if let syn::Item::Fn(func) = item {
            if !matches!(func.vis, Visibility::Public(_)) {
                continue;
            }

            let is_stateless = has_stateless_attr(func);
            let force_method = parse_force_method(func);

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

            // State-bearing fns: skip the leading state/store param.
            // Stateless fns: keep every declared input — there's no state arg
            // to drop.
            let skip_first = if is_stateless { 0 } else { 1 };

            if let Some(receiver) = receiver_item_type(&func.sig.output) {
                events.push(EventFn {
                    name: func.sig.ident.to_string(),
                    doc,
                    is_async: func.sig.asyncness.is_some(),
                    params: parse_params(func, skip_first),
                    item_type: norm_type(&receiver.item),
                    item_type_ast: receiver.item,
                    returns_result: receiver.is_result,
                    error_type: receiver.error.map(|e| resolve_through_uses(&e, &uses)),
                    surface: 0,
                });
                continue;
            }

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

            let params = parse_params(func, skip_first);

            let (return_type, return_type_ast, error_type) = extract_result_types(&func.sig.output);
            let error_type = error_type.map(|e| resolve_through_uses(&e, &uses));

            // A paginated list's companion. `count` takes the state or the
            // store (the first param is already known to be one of those;
            // anything else was skipped above) and then the same filter the
            // list takes, so that the total describes the same rows as the
            // page. A stateless `count()` is never the companion: the
            // generators would call it with an argument it does not declare.
            // It is recorded here but stays a function: only a paginated
            // module (known once the config is in hand) folds it into its
            // page handler — see `check_paginated_lists`.
            has_count |= fn_ident == "count" && !is_stateless;

            functions.push(ApiFn {
                name: fn_ident,
                is_async: func.sig.asyncness.is_some(),
                doc,
                params,
                return_type,
                return_type_ast,
                error_type,
                first_param_is_store: is_store,
                is_stateless,
                force_method,
                command_override,
                surface: 0,
                store_accessor: crate::servers::config::DEFAULT_STORE_ACCESSOR.to_string(),
            });
        }
    }

    result.module = Some(ApiModule { name: file_stem.to_string(), functions, events, is_singleton, has_count });
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

/// A fn's typed parameters after the first `skip_first`.
fn parse_params(func: &syn::ItemFn, skip_first: usize) -> Vec<Param> {
    func.sig
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
        .collect()
}

/// An event fn's return type: `Receiver<T>` or `Result<Receiver<T>, E>`.
struct ReceiverReturn {
    /// The `T`.
    item: Type,
    /// Whether the receiver comes wrapped in a `Result`.
    is_result: bool,
    /// The `E`, when the `Result` names one (`anyhow::Result<Receiver<T>>`
    /// does not).
    error: Option<Type>,
}

fn receiver_item_type(ret: &ReturnType) -> Option<ReceiverReturn> {
    let ReturnType::Type(_, ty) = ret else {
        return None;
    };
    if let Some(item) = receiver_item(ty) {
        return Some(ReceiverReturn { item, is_result: false, error: None });
    }
    let seg = last_segment(ty)?;
    if seg.ident != "Result" {
        return None;
    }
    let item = receiver_item(first_type_arg(seg)?)?;
    let PathArguments::AngleBracketed(args) = &seg.arguments else { return None };
    let error = args
        .args
        .iter()
        .filter_map(|a| match a {
            GenericArgument::Type(t) => Some(t.clone()),
            _ => None,
        })
        .nth(1);
    Some(ReceiverReturn { item, is_result: true, error })
}

/// The `T` of `Receiver<T>` (any path ending in `Receiver`).
fn receiver_item(ty: &Type) -> Option<Type> {
    let seg = last_segment(ty)?;
    if seg.ident != "Receiver" {
        return None;
    }
    first_type_arg(seg).cloned()
}

fn last_segment(ty: &Type) -> Option<&syn::PathSegment> {
    match ty {
        Type::Path(tp) => tp.path.segments.last(),
        _ => None,
    }
}

fn first_type_arg(seg: &syn::PathSegment) -> Option<&Type> {
    match &seg.arguments {
        PathArguments::AngleBracketed(args) => args.args.iter().find_map(|a| match a {
            GenericArgument::Type(t) => Some(t),
            _ => None,
        }),
        _ => None,
    }
}

/// Extract `T` from `Result<T, E>` as both a normalized string and AST, and
/// `E` as an AST.
///
/// `E` is `None` unless the `Result` names both arguments: a one-argument
/// alias such as `anyhow::Result<T>` hides its error type. When the return
/// type is not a `Result<...>` at all, the result is `("()", syn::Type::Tuple(_))`
/// for the unit type.
fn extract_result_types(ret: &ReturnType) -> (String, Type, Option<Type>) {
    if let ReturnType::Type(_, ty) = ret
        && let Type::Path(tp) = ty.as_ref()
    {
        let seg = tp.path.segments.last().unwrap();
        if seg.ident == "Result"
            && let PathArguments::AngleBracketed(args) = &seg.arguments
        {
            let mut types = args.args.iter().filter_map(|a| match a {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            });
            if let Some(t) = types.next() {
                return (norm_type(t), t.clone(), types.next().cloned());
            }
        }
    }
    ("()".to_string(), syn::parse_quote!(()), None)
}

/// The names a file's `use` items bind, each to the path it names
/// (`use a::b::{AppError, X as Y}` binds `AppError` to `a::b::AppError` and
/// `Y` to `a::b::X`; `use a::b::{self}` binds `b` to `a::b`). Globs bind no
/// name: what `use a::*` brings in is not known without reading `a`.
fn use_bindings(items: &[syn::Item]) -> HashMap<String, Vec<String>> {
    fn walk(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut HashMap<String, Vec<String>>) {
        let bind = |ident: &syn::Ident, prefix: &[String]| {
            let mut path = prefix.to_vec();
            if ident != "self" {
                path.push(ident.to_string());
            }
            path
        };
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                walk(&p.tree, prefix, out);
                prefix.pop();
            }
            syn::UseTree::Name(n) => {
                let path = bind(&n.ident, prefix);
                if let Some(name) = path.last() {
                    out.insert(name.clone(), path);
                }
            }
            syn::UseTree::Rename(r) if r.rename != "_" => {
                out.insert(r.rename.to_string(), bind(&r.ident, prefix));
            }
            syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
            syn::UseTree::Group(g) => g.items.iter().for_each(|t| walk(t, prefix, out)),
        }
    }
    let mut out = HashMap::new();
    for item in items {
        if let syn::Item::Use(u) = item {
            walk(&u.tree, &mut Vec::new(), &mut out);
        }
    }
    out
}

/// `ty` as a path, its leading segment replaced by what the file's `use`
/// items bind it to, repeatedly: `use crate::schema; use schema::AppError;`
/// makes `AppError` `crate::schema::AppError`. A leading `::` is dropped.
///
/// Only the file's `use` items are read. A name a glob brings in, a type
/// alias, or a local item stays as written, and `self::`/`super::` paths
/// stay relative: the module the file is mounted at is not known here.
/// A type with generic arguments is rendered unresolved; it is never
/// compared equal to an `AppError` path.
fn resolve_through_uses(ty: &Type, uses: &HashMap<String, Vec<String>>) -> String {
    let Type::Path(tp) = ty else { return norm_type(ty) };
    if tp.qself.is_some() || tp.path.segments.iter().any(|s| !s.arguments.is_none()) {
        return norm_type(ty);
    }
    let mut segments: Vec<String> = tp.path.segments.iter().map(|s| s.ident.to_string()).collect();
    if tp.path.leading_colon.is_none() {
        // Bounded so a binding that names itself (`use a::a;`) cannot loop.
        for _ in 0..=uses.len() {
            match uses.get(&segments[0]) {
                Some(target) if target.len() > 1 || target[0] != segments[0] => {
                    segments.splice(0..1, target.iter().cloned());
                }
                _ => break,
            }
        }
    }
    segments.join("::")
}

/// Scan a directory for API source files and parse them all.
///
/// Skips files ending in `_impl.rs` and `mod.rs`. The returned `ScanResult`
/// carries both the parsed modules (only those with at least one accepted
/// function or event) and the union of every per-file [`SkipRecord`] so a
/// caller can surface skipped functions through `cargo:warning=`.
pub fn scan_api_dir(api_dir: &Path, state_type: &str, store_type: Option<&str>) -> ScanResult {
    scan_api_dir_excluding(api_dir, state_type, store_type, None)
}

/// [`scan_api_dir`] that leaves out every file under `exclude`.
///
/// `gen_api` scans before it writes, and its output directory normally sits
/// inside a scan directory, so the previous run's generated forwarders must
/// not be read back as hand-written modules.
pub fn scan_api_dir_excluding(
    api_dir: &Path,
    state_type: &str,
    store_type: Option<&str>,
    exclude: Option<&Path>,
) -> ScanResult {
    let mut result = ScanResult::default();

    // Collect .rs files from api_dir and its immediate subdirectories (e.g. generated/)
    let mut entries: Vec<_> = collect_rs_files(api_dir);
    if let Some(exclude) = exclude {
        let exclude = fs::canonicalize(exclude).unwrap_or_else(|_| exclude.to_path_buf());
        entries.retain(|p| !fs::canonicalize(p).unwrap_or_else(|_| p.clone()).starts_with(&exclude));
    }
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

/// Scan every surface's `api_dir` and merge the results into one module list.
///
/// Each surface is scanned with its own `store_type`; every function and
/// event is stamped with the surface index and the surface's store accessor.
/// Same-named modules across surfaces merge under [`merge_surfaces`]'s rules.
pub fn scan_surfaces(surfaces: &[ApiSurface], state_type: &str) -> Result<ScanResult, String> {
    let mut result = ScanResult::default();
    let mut per_surface = Vec::with_capacity(surfaces.len());

    for (i, surface) in surfaces.iter().enumerate() {
        if !surface.api_dir.exists() {
            return Err(format!("API directory does not exist: {}", surface.api_dir.display()));
        }
        let mut scanned = scan_api_dir(&surface.api_dir, state_type, surface.store_type.as_deref());
        let accessor = surface.store_accessor();
        for m in &mut scanned.modules {
            for f in &mut m.functions {
                f.surface = i;
                f.store_accessor = accessor.to_string();
            }
            for ev in &mut m.events {
                ev.surface = i;
            }
        }
        result.skips.extend(scanned.skips);
        per_surface.push(scanned.modules);
    }

    result.modules = merge_surfaces(per_surface, surfaces)?;
    Ok(result)
}

/// The error for a fn name defined by two files of one surface's directory.
///
/// The usual cause is a generated `list`/`count` beside a hand-written one the
/// api stage did not scan, so the message states that rule.
fn same_surface_duplicate(module: &str, name: &str, dir: &str) -> String {
    if matches!(name, "list" | "count") {
        format!(
            "ontogen: fn `{module}::{name}` is defined twice in API directory `{dir}` (the generated module and a \
             hand-written one). A hand-written `list` or `count` replaces the generated one only when the api stage \
             scans this directory (`Pipeline::api_scan_dirs`, or `ApiConfig::scan_dirs` when calling `gen_api` \
             directly; `Pipeline` already scans the transports' `api_dir`s), and a `#[ontogen::stateless]` `{name}` \
             never replaces it"
        )
    } else {
        format!(
            "ontogen: fn `{module}::{name}` is defined twice in API directory `{dir}`; a function name may be defined once per module"
        )
    }
}

/// Merge per-surface module lists into one, folding same-named modules together.
///
/// Module order is the primary surface's, with modules only later surfaces
/// define appended in surface order. Within a merged module, functions and
/// events keep surface order and `is_singleton` comes from the first surface
/// that defines the module. Errors when a function or event name appears in
/// more than one surface, or when the CRUD five are split across surfaces.
pub fn merge_surfaces(per_surface: Vec<Vec<ApiModule>>, surfaces: &[ApiSurface]) -> Result<Vec<ApiModule>, String> {
    let dir = |i: usize| surfaces[i].api_dir.display();
    let mut merged: Vec<ApiModule> = Vec::new();

    for (surface, modules) in per_surface.into_iter().enumerate() {
        for incoming in modules {
            let Some(existing) = merged.iter_mut().find(|m| m.name == incoming.name) else {
                merged.push(incoming);
                continue;
            };
            let module = &incoming.name;
            existing.has_count |= incoming.has_count;
            for f in incoming.functions {
                if let Some(prior) = existing.functions.iter().find(|p| p.name == f.name) {
                    if prior.surface == surface {
                        return Err(same_surface_duplicate(module, &f.name, &dir(surface).to_string()));
                    }
                    return Err(format!(
                        "ontogen: fn `{}::{}` is defined by both API surfaces `{}` and `{}`; a function name may come \
                         from one surface only",
                        module,
                        f.name,
                        dir(prior.surface),
                        dir(surface),
                    ));
                }
                existing.functions.push(f);
            }
            for ev in incoming.events {
                if let Some(prior) = existing.events.iter().find(|p| p.name == ev.name) {
                    if prior.surface == surface {
                        return Err(format!(
                            "ontogen: event `{}::{}` is defined twice in API directory `{}`",
                            module,
                            ev.name,
                            dir(surface),
                        ));
                    }
                    return Err(format!(
                        "ontogen: event `{}::{}` is defined by both API surfaces `{}` and `{}`",
                        module,
                        ev.name,
                        dir(prior.surface),
                        dir(surface),
                    ));
                }
                existing.events.push(ev);
            }
        }
    }

    for m in &merged {
        let crud: Vec<&ApiFn> = m.functions.iter().filter(|f| CRUD_FN_NAMES.contains(&f.name.as_str())).collect();
        if let Some(first) = crud.first()
            && let Some(other) = crud.iter().find(|f| f.surface != first.surface)
        {
            return Err(format!(
                "ontogen: the CRUD functions of module `{}` must come from one API surface, but `{}` is in `{}` and \
                 `{}` is in `{}`",
                m.name,
                first.name,
                dir(first.surface),
                other.name,
                dir(other.surface),
            ));
        }
    }

    Ok(merged)
}

/// Rewrite type names that two surfaces import from different paths.
///
/// A bare name (`Workout`) referenced by functions of several surfaces would
/// need one `use` line per surface and clash. The surface with the lowest
/// index keeps the bare name; every other surface's references become the
/// fully qualified path (`determined_fitness::schema::Workout`), which the
/// import collector leaves alone. Surfaces sharing a `types_import_path`
/// share the import and are not rewritten.
pub fn qualify_shared_types(modules: &mut [ApiModule], surfaces: &[ApiSurface]) {
    // name -> the types path that keeps the bare import (lowest surface index).
    let mut owner: HashMap<String, (usize, &str)> = HashMap::new();
    for m in modules.iter() {
        for f in &m.functions {
            let path = surfaces[f.surface].types_import_path.as_str();
            for name in fn_type_imports(f) {
                let entry = owner.entry(name).or_insert((f.surface, path));
                if f.surface < entry.0 {
                    *entry = (f.surface, path);
                }
            }
        }
    }

    for m in modules.iter_mut() {
        for f in &mut m.functions {
            let path = surfaces[f.surface].types_import_path.as_str();
            let shared: Vec<String> = fn_type_imports(f)
                .into_iter()
                .filter(|name| owner.get(name).is_some_and(|(_, owner_path)| *owner_path != path))
                .collect();
            if shared.is_empty() {
                continue;
            }
            for name in &shared {
                qualify_type(&mut f.return_type_ast, name, path);
                for p in &mut f.params {
                    qualify_type(&mut p.ty_ast, name, path);
                }
            }
            f.return_type = norm_type(&f.return_type_ast);
            for p in &mut f.params {
                p.ty = norm_type(&p.ty_ast);
            }
        }
    }
}

/// Bare type names a function's signature imports.
fn fn_type_imports(f: &ApiFn) -> Vec<String> {
    let mut names = Vec::new();
    collect_type_import(&f.return_type_ast, &mut names);
    // An order's types are the store's and never imported from a surface.
    for p in f.params.iter().filter(|p| p.order_sort_field().is_none()) {
        collect_type_import(&p.ty_ast, &mut names);
    }
    names
}

/// Replace every single-segment path `name` inside `ty` with `{path}::{name}`.
fn qualify_type(ty: &mut Type, name: &str, path: &str) {
    match ty {
        Type::Reference(r) => qualify_type(&mut r.elem, name, path),
        Type::Paren(p) => qualify_type(&mut p.elem, name, path),
        Type::Group(g) => qualify_type(&mut g.elem, name, path),
        Type::Slice(s) => qualify_type(&mut s.elem, name, path),
        Type::Array(a) => qualify_type(&mut a.elem, name, path),
        Type::Tuple(t) => {
            for elem in &mut t.elems {
                qualify_type(elem, name, path);
            }
        }
        Type::Path(tp) => {
            if tp.qself.is_none() && tp.path.segments.len() == 1 && tp.path.segments[0].ident == name {
                let qualified: syn::Path =
                    syn::parse_str(&format!("{path}::{name}")).expect("types_import_path parses");
                let args = std::mem::replace(&mut tp.path.segments[0].arguments, PathArguments::None);
                tp.path = qualified;
                tp.path.segments.last_mut().expect("non-empty path").arguments = args;
            }
            for seg in &mut tp.path.segments {
                if let PathArguments::AngleBracketed(ab) = &mut seg.arguments {
                    for arg in &mut ab.args {
                        if let GenericArgument::Type(inner) = arg {
                            qualify_type(inner, name, path);
                        }
                    }
                }
            }
        }
        _ => {}
    }
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

/// A paginated `list` pushes its page down to the data it reads: it must take
/// `limit`/`offset` as its last two parameters and sit beside a `count`.
/// Anything else would make the handler load the whole table to slice it.
/// The `count` takes the same first parameter the module's other functions
/// take — the store or the state — and then the same filter the `list` takes,
/// so the total counts the rows the page is drawn from. A `list` that filters
/// beside a `count` that does not would report the whole table as the total of
/// a filtered page.
///
/// The `count` of a paginated module is what the page handlers call for the
/// total, so it is taken off `functions` here; on a module no surface
/// paginates it stays an operation of its own. `gen_servers` and
/// `gen_clients` both run this on the modules they generate from, so no
/// client calls a `count` route, command or tool the servers do not serve.
///
/// `pagination` and `extra_surfaces` are the config's, as
/// [`pagination_for`](crate::servers::config::pagination_for) reads them.
pub fn check_paginated_lists(
    modules: &mut [ApiModule],
    pagination: &Option<crate::servers::config::PaginationConfig>,
    extra_surfaces: &[crate::servers::config::ApiSurface],
) -> Result<(), String> {
    for m in modules {
        let mut paginated = false;
        for f in &m.functions {
            if f.name != "list"
                || !f.return_type.starts_with("Vec<")
                || crate::servers::config::pagination_for(pagination, extra_surfaces, &m.name, f.surface).is_none()
            {
                continue;
            }
            paginated = true;
            if !f.takes_page() || !m.has_count {
                return Err(format!(
                    "ontogen: module `{}` is paginated, so `{}::list` must take `limit: Option<u64>, offset: Option<u64>` as \
                     its last two parameters and the module must define `count(store)` or `count(state)` returning \
                     `Result<u64, _>` and taking nothing else; a generated CRUD module gets both from \
                     `ApiConfig::paginated`{}",
                    m.name,
                    m.name,
                    if f.takes_page() {
                        "; a hand-written `list` replaces the generated `count`, so the module needs a `count` taking \
                         the same filter as the list"
                    } else {
                        ""
                    }
                ));
            }
            // The total has to describe the rows the page is drawn from, so
            // whatever the list filters by, the count filters by too.
            let want = f.filter_params();
            let got = m.functions.iter().find(|c| c.is_count()).map(ApiFn::filter_params).unwrap_or_default();
            if want != got {
                let render = |ps: &[String]| if ps.is_empty() { "nothing".to_string() } else { ps.join(", ") };
                return Err(format!(
                    "ontogen: module `{}` is paginated, so `{}::count` must take the same filter `{}::list` takes, \
                     or the total describes different rows than the page; `list` filters by {}, `count` by {}",
                    m.name,
                    m.name,
                    m.name,
                    render(&want),
                    render(&got)
                ));
            }
        }
        if paginated {
            m.functions.retain(|f| !f.is_count());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result_types(sig: &str) -> (String, Option<String>) {
        let func: syn::ItemFn = syn::parse_str(&format!("{sig} {{ todo!() }}")).unwrap();
        let (ok, _, err) = extract_result_types(&func.sig.output);
        (ok, err.as_ref().map(norm_type))
    }

    #[test]
    fn error_type_is_the_second_result_argument() {
        assert_eq!(
            result_types("fn f() -> Result<Vec<Task>, AppError>"),
            ("Vec<Task>".to_string(), Some("AppError".to_string()))
        );
        assert_eq!(
            result_types("fn f() -> std::result::Result<(), crate::schema::AppError>"),
            ("()".to_string(), Some("crate::schema::AppError".to_string()))
        );
    }

    #[test]
    fn error_type_is_none_without_a_two_argument_result() {
        assert_eq!(result_types("fn f() -> anyhow::Result<Task>"), ("Task".to_string(), None));
        assert_eq!(result_types("fn f() -> u64"), ("()".to_string(), None));
        assert_eq!(result_types("fn f()"), ("()".to_string(), None));
    }

    fn resolved(uses: &str, ty: &str) -> String {
        let file = syn::parse_file(uses).unwrap();
        resolve_through_uses(&syn::parse_str(ty).unwrap(), &use_bindings(&file.items))
    }

    #[test]
    fn an_error_type_resolves_through_the_files_use_items() {
        let uses = "use fitness::schema::{AppError, Workout};\nuse crate::schema::{self as api, AppError as ApiError};\n\
                    use crate::{store::{self, Store}};\nuse ::other::Error;\n";
        assert_eq!(resolved(uses, "AppError"), "fitness::schema::AppError");
        assert_eq!(resolved(uses, "ApiError"), "crate::schema::AppError");
        assert_eq!(resolved(uses, "api::AppError"), "crate::schema::AppError");
        assert_eq!(resolved(uses, "store::StoreError"), "crate::store::StoreError");
        assert_eq!(resolved(uses, "Error"), "other::Error");
        assert_eq!(resolved(uses, "crate::schema::AppError"), "crate::schema::AppError");
        assert_eq!(resolved(uses, "::fitness::schema::AppError"), "fitness::schema::AppError");
    }

    #[test]
    fn a_use_may_name_another_uses_binding() {
        assert_eq!(resolved("use crate::schema;\nuse schema::AppError;\n", "AppError"), "crate::schema::AppError");
        // A binding that names itself stops rather than loops.
        resolved("use a::a;\n", "a::E");
    }

    #[test]
    fn what_the_use_items_do_not_bind_stays_as_written() {
        let uses = "use crate::schema::*;\nuse crate::schema::AppError as _;\nuse self::local::Thing;\n";
        assert_eq!(resolved(uses, "AppError"), "AppError");
        assert_eq!(resolved(uses, "super::schema::AppError"), "super::schema::AppError");
        assert_eq!(resolved(uses, "Thing"), "self::local::Thing");
        assert_eq!(resolved("use fitness::schema::AppError;\n", "Box<AppError>"), "Box<AppError>");
    }

    #[test]
    fn parsed_functions_carry_their_error_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task.rs");
        fs::write(
            &path,
            "pub async fn get_by_id(store: &Store, id: String) -> Result<Task, crate::schema::AppError> { todo!() }\n\
             pub fn summary(state: &AppState) -> Result<String, String> { todo!() }\n\
             pub fn ping(state: &AppState) -> anyhow::Result<()> { todo!() }\n\
             use fitness::schema::AppError;\n\
             pub fn pong(state: &AppState) -> Result<(), AppError> { todo!() }\n",
        )
        .unwrap();
        let module = parse_api_module(&path, "AppState", Some("Store")).module.expect("module parses");
        let error_of = |name: &str| module.functions.iter().find(|f| f.name == name).and_then(|f| f.error_type.clone());
        assert_eq!(error_of("get_by_id").as_deref(), Some("crate::schema::AppError"));
        assert_eq!(error_of("summary").as_deref(), Some("String"));
        assert_eq!(error_of("ping"), None);
        assert_eq!(error_of("pong").as_deref(), Some("fitness::schema::AppError"));
    }

    #[test]
    fn parsed_event_fns_carry_their_error_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("activity.rs");
        fs::write(
            &path,
            "use crate::schema::{self as api};\n\
             pub fn changed(state: &AppState) -> Receiver<Task> { todo!() }\n\
             pub async fn for_kind(state: &AppState, kind: String) -> Result<Receiver<Activity>, api::AppError> { todo!() }\n\
             pub fn plain(state: &AppState) -> anyhow::Result<Receiver<Activity>> { todo!() }\n",
        )
        .unwrap();
        let module = parse_api_module(&path, "AppState", None).module.expect("module parses");
        let event = |name: &str| module.events.iter().find(|e| e.name == name).expect("an event fn");
        assert_eq!((event("changed").returns_result, event("changed").error_type.clone()), (false, None));
        assert_eq!(event("for_kind").error_type.as_deref(), Some("crate::schema::AppError"));
        assert!(event("for_kind").returns_result);
        assert_eq!((event("plain").returns_result, event("plain").error_type.clone()), (true, None));
    }

    #[test]
    fn a_paginated_filtered_list_without_a_count_says_the_hand_written_list_replaced_the_generated_one() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("task.rs"),
            "pub async fn list(store: &Store, status: Option<String>, limit: Option<u64>, offset: Option<u64>) \
             -> Result<Vec<Task>, AppError> { todo!() }\n",
        )
        .unwrap();
        let mut modules = scan_api_dir(tmp.path(), "AppState", Some("Store")).modules;
        let pagination = Some(crate::servers::config::PaginationConfig { default_limit: 20, max_limit: 100 });
        let err = check_paginated_lists(&mut modules, &pagination, &[]).unwrap_err();
        assert!(err.contains("module `task` is paginated"), "{err}");
        assert!(err.contains("a hand-written `list` replaces the generated `count`"), "{err}");
        assert!(err.contains("a `count` taking the same filter as the list"), "{err}");
    }

    fn one_surface(dir: &Path) -> Vec<crate::servers::ApiSurface> {
        vec![crate::servers::ApiSurface {
            api_dir: dir.to_path_buf(),
            service_import_path: "crate::api".to_string(),
            types_import_path: "crate::schema".to_string(),
            store_accessor: None,
            store_type: Some("Store".to_string()),
            pagination: None,
            paginated_modules: Vec::new(),
            schema_dir: None,
        }]
    }

    fn write_task_files(dir: &Path, generated: &str, hand_written: &str) {
        std::fs::create_dir_all(dir.join("generated")).unwrap();
        std::fs::write(dir.join("generated/task.rs"), generated).unwrap();
        std::fs::write(dir.join("task.rs"), hand_written).unwrap();
    }

    const LIST: &str = "pub async fn list(store: &Store) -> Result<Vec<Task>, AppError> { todo!() }\n";

    #[test]
    fn a_list_in_the_generated_and_the_hand_written_file_names_the_scan_rule() {
        let tmp = tempfile::tempdir().unwrap();
        write_task_files(tmp.path(), LIST, LIST);
        let err = scan_surfaces(&one_surface(tmp.path()), "AppState").unwrap_err();
        assert!(err.contains("fn `task::list` is defined twice in API directory"), "{err}");
        assert!(err.contains("`Pipeline::api_scan_dirs`") && err.contains("`ApiConfig::scan_dirs`"), "{err}");
        assert!(err.contains("`#[ontogen::stateless]` `list` never replaces it"), "{err}");
        assert!(!err.contains("both API surfaces"), "{err}");
    }

    #[test]
    fn another_duplicated_fn_in_one_surface_says_it_is_defined_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let publish = "pub async fn publish(store: &Store) -> Result<(), AppError> { todo!() }\n";
        write_task_files(tmp.path(), publish, publish);
        let err = scan_surfaces(&one_surface(tmp.path()), "AppState").unwrap_err();
        assert!(err.contains("fn `task::publish` is defined twice in API directory"), "{err}");
        assert!(!err.contains("api_scan_dirs"), "{err}");
    }
}

#[cfg(test)]
mod order_param_tests {
    use super::*;

    /// A `list` with the parameters of `sig` after its first, as the parser
    /// reads them.
    fn list(sig: &str) -> ApiFn {
        let func: syn::ItemFn = syn::parse_str(&format!("pub async fn list({sig}) {{ todo!() }}")).unwrap();
        ApiFn { name: "list".into(), params: parse_params(&func, 1), ..Default::default() }
    }

    fn names(params: &[&Param]) -> Vec<String> {
        params.iter().map(|p| p.name.clone()).collect()
    }

    #[test]
    fn an_order_param_is_recognised_by_type_under_any_name() {
        for (sig, field) in [
            ("store: &Store, order: &[OrderBy<TaskSortField>]", "TaskSortField"),
            ("store: &Store, sorting: &[OrderBy<TaskSortField>]", "TaskSortField"),
            ("store: &Store, o: &[ontogen_core::order::OrderBy<crate::store::task::TaskSortField>]", "TaskSortField"),
            ("store: &Store, order: &'a [OrderBy<store::Epic_SortField>]", "Epic_SortField"),
        ] {
            let f = list(sig);
            let order = f.order_param().unwrap_or_else(|| panic!("{sig}"));
            assert_eq!(order.order_sort_field(), Some(field), "{sig}");
            assert!(f.takes_order(), "{sig}");
        }
        let f = list("store: &Store, order: &[ontogen_core::order::OrderBy<crate::store::task::TaskSortField>]");
        assert_eq!(f.order_param().map(|p| p.name.as_str()), Some("order"));
        assert_eq!(f.sort_entity(), Some("Task"));
    }

    #[test]
    fn other_shapes_are_not_an_order() {
        for sig in [
            "store: &Store",
            "store: &Store, order: Vec<OrderBy<TaskSortField>>",
            "store: &Store, order: &Vec<OrderBy<TaskSortField>>",
            "store: &Store, order: &mut [OrderBy<TaskSortField>]",
            "store: &Store, order: [OrderBy<TaskSortField>; 2]",
            "store: &Store, order: &[TaskSortField]",
            "store: &Store, order: &[Order<TaskSortField>]",
            "store: &Store, order: &[OrderBy]",
            "store: &Store, order: &[OrderBy<TaskSortField, Extra>]",
            "store: &Store, order: &[OrderBy<Vec<TaskSortField>>]",
            "store: &Store, order: &[OrderBy<&TaskSortField>]",
            "store: &Store, order: Option<&[OrderBy<TaskSortField>]>",
            "store: &Store, order: &str",
        ] {
            let f = list(sig);
            assert!(f.order_param().is_none() && !f.takes_order() && f.sort_entity().is_none(), "{sig}");
        }
    }

    #[test]
    fn the_sort_entity_needs_a_sort_field_suffix() {
        assert_eq!(list("s: &Store, order: &[OrderBy<TaskSortField>]").sort_entity(), Some("Task"));
        assert_eq!(list("s: &Store, order: &[OrderBy<WorkoutSetSortField>]").sort_entity(), Some("WorkoutSet"));
        assert_eq!(list("s: &Store, order: &[OrderBy<TaskOrder>]").sort_entity(), None);
        assert_eq!(list("s: &Store, order: &[OrderBy<SortField>]").sort_entity(), None);
    }

    #[test]
    fn the_first_order_param_is_the_order() {
        let f = list("s: &Store, a: &[OrderBy<TaskSortField>], b: &[OrderBy<EpicSortField>]");
        assert_eq!(f.order_param().map(|p| p.name.as_str()), Some("a"));
        assert_eq!(f.sort_entity(), Some("Task"));
    }

    #[test]
    fn the_filter_excludes_the_order_wherever_it_sits() {
        let paged = list(
            "store: &Store, status: &str, query: TaskQuery, order: &[OrderBy<TaskSortField>], limit: Option<u64>, \
             offset: Option<u64>",
        );
        assert!(paged.takes_page() && paged.takes_order());
        assert_eq!(names(&paged.filter()), ["status", "query"]);
        assert_eq!(paged.filter_params(), ["status: &str", "query: TaskQuery"]);
        assert_eq!(paged.filter_struct().map(|p| p.name.as_str()), Some("query"));
        assert_eq!(names(&paged.bare_filters()), ["status"]);
        assert!(paged.takes_filter());
        assert_eq!(paged.page().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), PAGE_PARAMS);

        let unpaged = list("store: &Store, status: &str, order: &[OrderBy<TaskSortField>]");
        assert!(!unpaged.takes_page() && unpaged.takes_order());
        assert_eq!(names(&unpaged.filter()), ["status"]);
        assert!(unpaged.page().is_empty());

        let first = list("store: &Store, order: &[OrderBy<TaskSortField>], status: &str");
        assert_eq!(names(&first.filter()), ["status"]);
    }

    #[test]
    fn an_order_alone_is_no_filter() {
        for sig in [
            "store: &Store, order: &[OrderBy<TaskSortField>]",
            "store: &Store, order: &[OrderBy<TaskSortField>], limit: Option<u64>, offset: Option<u64>",
        ] {
            let f = list(sig);
            assert!(f.filter().is_empty() && !f.takes_filter() && f.filter_params().is_empty(), "{sig}");
            assert!(f.filter_struct().is_none() && f.bare_filters().is_empty(), "{sig}");
        }
    }
}
