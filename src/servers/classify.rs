//! Operation classification and parameter analysis.

use ontogen_core::ir::OpKind;
use ontogen_core::naming::pluralize;
use syn::{PathArguments, Type};

use crate::resource::ResourceModel;
use crate::servers::parse::{ApiFn, ApiModule, ForcedMethod, Param};

/// Classify a function into an operation kind.
///
/// Source-side `#[ontogen::http::*]` attributes short-circuit the heuristic
/// and return the forced classification unconditionally.
///
/// `#[ontogen::http::post]` forces `OpKind::CustomPost`. It overlaps with the
/// default for zero-user-param functions outside the known-read prefix
/// allowlist — since those default to `CustomPost` already, an explicit
/// annotation on such a function is redundant but harmless. It remains useful
/// for forcing POST on a `get_*`/`list_*`/`is_*`-prefixed handler that
/// actually mutates state.
///
/// `#[ontogen::http::get]` forces `OpKind::CustomGet`, and exists because the
/// name heuristic is asymmetric: [`KNOWN_READ_PREFIXES`] is consulted **only**
/// in the zero-user-param branch, and among functions that carry params only
/// `get_` has a path back into `CustomGet`. A read *with arguments* named
/// `count_*`, `exists_*`, `find_*`, `is_*` or `has_*` therefore classifies
/// `CustomPost` no matter how side-effect-free it is. Before this override the
/// only remedy was renaming to `get_*`, which is a real cost: it bends a
/// handler's name to satisfy the router rather than to describe what it does,
/// and the resulting `get_matching_file_count`-style names read as
/// workarounds without a comment explaining why.
///
/// Both overrides win outright. Forcing GET is the author asserting the
/// param shape suits a GET; it deliberately does **not** re-run the
/// body-carrying-first-param check the `get_*` branch applies, for the same
/// reason `Post` does not re-run anything — an explicit override that
/// second-guesses the author is not an override.
pub fn classify_op(func: &ApiFn) -> OpKind {
    match func.force_method {
        Some(ForcedMethod::Post) => OpKind::CustomPost,
        Some(ForcedMethod::Get) => OpKind::CustomGet,
        None => classify_by_name_and_params(&func.name, &func.params),
    }
}

/// Allowlist of name prefixes that classify a custom function as a read
/// (`OpKind::CustomGet`). The list is intentionally conservative — only
/// English verbs whose canonical sense is retrieval-without-side-effect.
///
/// Used by `name_implies_read` to opt zero-user-param functions back into
/// `CustomGet` after the classifier's default flipped to `CustomPost`
/// (RFC 7231 §4.2.1: GET is for retrieval, not action). Named-CRUD
/// (`list`, `get_by_id`) is matched earlier and does not need to live here.
///
/// **Scope: zero-user-param functions only.** `classify_by_name_and_params`
/// consults this list in the `params.is_empty()` branch and nowhere else, so
/// among functions that carry params only `get_` routes as a read — the other
/// six prefixes here have no effect at all once a handler takes an argument.
/// That is deliberate rather than an oversight (a read with arguments may want
/// a body, which GET cannot carry), but it does mean the list is narrower than
/// its name suggests. `#[ontogen::http::get]` is how a params-carrying read
/// opts in explicitly.
///
/// Extension policy: add a prefix here only if it unambiguously denotes a
/// read in every plausible domain. Borderline cases (`load_`, `read_`,
/// `fetch_`) should ship behind an explicit `#[ontogen::http::get]`
/// override rather than be inferred — the cost of a false positive (a
/// mutating handler routed as a cacheable, retried GET) is higher than the
/// cost of one annotation.
const KNOWN_READ_PREFIXES: &[&str] = &["get_", "list_", "count_", "exists_", "find_", "is_", "has_"];

/// Returns true if `name` starts with one of the [`KNOWN_READ_PREFIXES`].
///
/// Used by [`classify_by_name_and_params`] to decide whether a function with
/// no user-facing params should still classify as `CustomGet`. Anything
/// outside the allowlist defaults to `CustomPost` — the RFC-7231-safe
/// default — and can opt back into GET routing either by renaming to a
/// read prefix or via an explicit `#[ontogen::http::get]` override, which
/// [`classify_op`] short-circuits on before this function is consulted.
fn name_implies_read(name: &str) -> bool {
    KNOWN_READ_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// Classify a function by name and parameters.
///
/// Lower-level entry point used when an `ApiFn` is not available
/// (e.g., the API layer's IR conversion).
///
/// # Default for zero-user-param functions
///
/// RFC 7231 §4.2.1 defines GET as a safe method: it is for retrieval and
/// MUST NOT carry semantics that mutate state. A function with no
/// user-facing parameters carries no body, but that does not make it a
/// read — `pause(state)`, `backup(state)`, `reset_all(state)` and friends
/// all mutate state with no params. We default zero-param custom fns to
/// `CustomPost` and opt them back into `CustomGet` only when the function
/// name starts with one of the [`KNOWN_READ_PREFIXES`] (`get_`, `list_`,
/// `count_`, `exists_`, `find_`, `is_`, `has_`).
///
/// Functions whose name happens to look mutating but are actually reads
/// (`stats::workout`, `dashboard::snapshot`) should either rename to a
/// known-read prefix or carry an explicit `#[ontogen::http::get]`
/// annotation — see [`ForcedMethod::Get`].
///
/// [`ForcedMethod::Get`]: crate::servers::parse::ForcedMethod::Get
///
/// # `get_*` with body-carrying first param
///
/// A function whose name starts with `get_` is *intended* to be a read
/// operation, but the HTTP transport can only route it as `GET` if its
/// parameters are path/query-extractable. When the first user-facing
/// parameter is a custom struct (anything that isn't an id-like primitive,
/// `Option<…>`, or a slice), the function needs a JSON body — which `GET`
/// can't carry. In that case we classify it as `CustomPost` so the HTTP
/// generator emits `Json(...)` body extraction instead of `Path(...)`
/// extraction with `String`. The IPC and MCP transports are unaffected;
/// they don't distinguish GET from POST.
pub fn classify_by_name_and_params(name: &str, params: &[Param]) -> OpKind {
    match name {
        "list" => OpKind::List,
        "get_by_id" => OpKind::GetById,
        "create" => OpKind::Create,
        "update" => OpKind::Update,
        "delete" => OpKind::Delete,
        _ => {
            // Junction: add_{child}(parent_id, child_id) - exactly 2 params
            if let Some(rest) = name.strip_prefix("add_")
                && params.len() == 2
            {
                return OpKind::JunctionAdd { child_segment: junction_child_segment(rest, false) };
            }

            // Junction: remove_{child}(parent_id, child_id) - exactly 2 params
            if let Some(rest) = name.strip_prefix("remove_")
                && params.len() == 2
            {
                return OpKind::JunctionRemove { child_segment: junction_child_segment(rest, false) };
            }

            // Junction: list_{children}(parent_id) - exactly 1 param, not "list" itself
            if let Some(rest) = name.strip_prefix("list_")
                && params.len() == 1
            {
                return OpKind::JunctionList { child_segment: junction_child_segment(rest, true) };
            }

            // Zero-user-param custom fns default to CustomPost (RFC-7231-safe).
            // Opt back into CustomGet only when the name matches a known-read
            // prefix (see `KNOWN_READ_PREFIXES`).
            if params.is_empty() {
                return if name_implies_read(name) { OpKind::CustomGet } else { OpKind::CustomPost };
            }

            // `get_*` with body-carrying first param: classify as CustomPost so
            // the HTTP generator emits Json body extraction instead of trying
            // to stuff the struct into a URL path segment as Path<String>.
            if name.starts_with("get_") {
                return if first_param_wants_body(&params[0].ty_ast) { OpKind::CustomPost } else { OpKind::CustomGet };
            }

            OpKind::CustomPost
        }
    }
}

/// The rules every HTTP route sets on an op, checked for the server and the
/// clients alike, so that neither generates a route the other refuses. Only
/// a build with an HTTP server or HTTP client runs them: IPC and MCP read
/// every argument from one flat payload, so none of these limits are theirs.
///
/// - A singleton module has no collection and no resource type, so an op in
///   it that classifies as CRUD or junction is a mistake (wire contract
///   §10.3): ops named for a collection belong in a module that is not a
///   singleton.
/// - A `GET` or `DELETE` carries no body, so an op served with either cannot
///   take an `*Input` argument (§10.2). Nor can a `list`, which is a `GET`.
/// - A CRUD-named op in a module with no resource behind it is served at
///   its collection's route (§10.4), which carries the route's own
///   arguments and no others, and the TypeScript clients call it with
///   exactly those. A `list` is the exception: one that takes more than its
///   page is a list that takes a filter (§7.3).
/// - An `opArg[…]` value is one query-string value, so an optional argument
///   of a `GET` must read from one: a type with no generic arguments that
///   names no schema entity.
/// - A list's filter is read from `filter[…]` (§7.3): each field of at most
///   one `*Query` struct, taken by value, is a member, and so is every
///   other filter argument, which must read from one value as an `opArg`
///   does, optional or not.
///
/// # Errors
///
/// The first op that breaks a rule, named `module::fn`.
pub(crate) fn check_http_ops(modules: &[ApiModule], resources: &ResourceModel) -> Result<(), String> {
    for m in modules {
        for f in &m.functions {
            let op = classify_op(f);
            let crud_or_junction = matches!(
                op,
                OpKind::List
                    | OpKind::GetById
                    | OpKind::Create
                    | OpKind::Update
                    | OpKind::Delete
                    | OpKind::JunctionList { .. }
                    | OpKind::JunctionAdd { .. }
                    | OpKind::JunctionRemove { .. }
            );
            if m.is_singleton && crud_or_junction {
                return Err(format!(
                    "ontogen: `{}::{}` is a collection op in the singleton module `{}`, which has no collection; \
                     rename it or move it to a module that is not a singleton",
                    m.name, f.name, m.name
                ));
            }
            let bodyless = matches!(op, OpKind::CustomGet | OpKind::GetById | OpKind::JunctionList { .. })
                || matches!(op, OpKind::List | OpKind::Delete | OpKind::JunctionRemove { .. });
            if bodyless && let Some(input) = f.params.iter().find(|p| p.is_input()) {
                let instead = if op == OpKind::List {
                    "take its filter as a `*Query` struct or as plain arguments, each read from `filter[…]`"
                } else {
                    "serve it as a POST (`#[ontogen::http::post]`) or pass the input's fields as arguments"
                };
                return Err(format!(
                    "ontogen: `{}::{}` is served without a request body, so it cannot take `{}: {}`; {instead}",
                    m.name, f.name, input.name, input.ty
                ));
            }
            if resources.by_module(&m.name).is_none()
                && let Some((route, takes)) = entityless_crud_route(&op)
                && f.params.len() != takes.len()
            {
                let given: Vec<String> = f.params.iter().map(|p| format!("`{}: {}`", p.name, p.ty)).collect();
                let given = if given.is_empty() { "nothing".to_string() } else { given.join(", ") };
                return Err(format!(
                    "ontogen: `{}::{}` is served at `{route}`, since the module `{}` has no schema entity behind \
                     it, and that route passes {} after the state or store; the fn takes {given}. Take exactly \
                     those arguments, or rename the fn so it is served as a custom op",
                    m.name,
                    f.name,
                    m.name,
                    takes.join(" and "),
                ));
            }
            if op == OpKind::List {
                check_list_filter(m, f, resources)?;
            }
            if op == OpKind::CustomGet
                && let Some(p) = f.params.iter().find(|p| p.is_option() && !reads_from_one_value(p, resources))
            {
                return Err(format!(
                    "ontogen: `{}::{}` reads `{}: {}` from the query parameter `opArg[{}]`, which carries one \
                     string, number, bool or unit enum value; serve it as a POST (`#[ontogen::http::post]`), \
                     which reads it from `meta.args`, or take a type one value can carry",
                    m.name, f.name, p.name, p.ty, p.name
                ));
            }
        }
    }
    Ok(())
}

/// The route a CRUD-named op of a module with no resource is served at, and
/// the arguments it passes (§10.4). `None` for a `list`, which may take a
/// filter, and for every other op.
fn entityless_crud_route(op: &OpKind) -> Option<(&'static str, &'static [&'static str])> {
    match op {
        OpKind::GetById => Some(("GET …/{id}", &["`id`"])),
        OpKind::Create => Some(("POST …", &["its input"])),
        OpKind::Update => Some(("PATCH …/{id}", &["`id`", "its input"])),
        OpKind::Delete => Some(("DELETE …/{id}", &["`id`"])),
        _ => None,
    }
}

/// The `filter[…]` rules of a `list` (§7.3): at most one `*Query` struct,
/// taken by value, and every other filter argument read from one value.
fn check_list_filter(m: &ApiModule, f: &ApiFn, resources: &ResourceModel) -> Result<(), String> {
    let structs: Vec<&Param> = f.filter().iter().filter(|p| p.is_filter_struct()).collect();
    if let [first, second, ..] = structs.as_slice() {
        return Err(format!(
            "ontogen: `{}::{}` takes two `*Query` filter structs, `{}: {}` and `{}: {}`, but every `filter[…]` \
             member is read into one struct; merge them into one, or take `{}`'s fields as plain arguments",
            m.name, f.name, first.name, first.ty, second.name, second.ty, second.name
        ));
    }
    if let Some(p) = structs.first()
        && (p.ty.starts_with('&') || p.is_option())
    {
        return Err(format!(
            "ontogen: `{}::{}` takes its filter struct as `{}: {}`, but the struct is read from `filter[…]` into a \
             value the list owns; take it by value (`{}: {}`), with an `Option` field for each optional member",
            m.name,
            f.name,
            p.name,
            p.ty,
            p.name,
            inner_struct_type(&p.ty)
        ));
    }
    if let Some(p) = f.bare_filters().into_iter().find(|p| !filter_reads_from_one_value(p, resources)) {
        return Err(format!(
            "ontogen: `{}::{}` reads `{}: {}` from the query parameter `filter[{}]`, which carries one string, \
             number, bool or unit enum value; take a type one value can carry, optional or not",
            m.name, f.name, p.name, p.ty, p.name
        ));
    }
    Ok(())
}

/// The struct a filter struct type names, without its `&` or `Option`.
fn inner_struct_type(ty: &str) -> &str {
    fn unref(ty: &str) -> &str {
        ty.trim_start_matches('&').trim_start_matches("mut ").trim_start()
    }
    let ty = unref(ty);
    ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')).map_or(ty, unref)
}

/// True when the `Option` argument `p` can be read from one query-string
/// value: its inner type passes [`is_one_value`].
fn reads_from_one_value(p: &Param, resources: &ResourceModel) -> bool {
    let Ok(Type::Path(outer)) = syn::parse_str::<Type>(&p.ty) else { return false };
    let Some(PathArguments::AngleBracketed(args)) = outer.path.segments.last().map(|s| &s.arguments) else {
        return false;
    };
    let Some(syn::GenericArgument::Type(inner)) = args.args.first() else { return false };
    is_one_value(inner, resources)
}

/// True when the bare filter `p` can be read from one query-string value:
/// its type, under any `&` and one `Option`, passes [`is_one_value`].
fn filter_reads_from_one_value(p: &Param, resources: &ResourceModel) -> bool {
    let Ok(ty) = syn::parse_str::<Type>(&p.ty) else { return false };
    let ty = match ty {
        Type::Reference(r) => *r.elem,
        ty => ty,
    };
    if let Type::Path(outer) = &ty
        && outer.qself.is_none()
        && let Some(last) = outer.path.segments.last()
        && last.ident == "Option"
        && let PathArguments::AngleBracketed(args) = &last.arguments
        && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
    {
        return is_one_value(inner, resources);
    }
    is_one_value(&ty, resources)
}

/// True when `ty`, under any `&`, is a path with no generic arguments that
/// names no schema entity: a type serde reads from one string. Whether such
/// a path is a struct cannot be told from its name, so only an entity is
/// refused.
fn is_one_value(ty: &Type, resources: &ResourceModel) -> bool {
    let ty = match ty {
        Type::Reference(r) => &*r.elem,
        ty => ty,
    };
    let Type::Path(tp) = ty else { return false };
    tp.qself.is_none() && tp.path.segments.iter().all(|s| s.arguments.is_none()) && resources.by_item_type(ty).is_none()
}

/// Returns true when the param type carries a body (JSON-extractable struct
/// shape) rather than fitting in a URL path segment or query string.
///
/// Mirrors the body/path/query partition used by the HTTP generator:
///
/// - `Option<T>` → false (lands in the query-string slot)
/// - id-like primitives (`String`, `&str`, integers, `Uuid`) → false (path)
/// - slices / arrays / tuples / non-Path shapes → false (current emitter
///   has no extraction story for these; flag for future work but don't
///   route them as bodies today)
/// - everything else (single-segment custom struct, qualified path,
///   `Vec<T>`, `HashMap<K, V>`, …) → true (body)
fn first_param_wants_body(ty: &Type) -> bool {
    let inner = match ty {
        Type::Reference(r) => &*r.elem,
        _ => ty,
    };
    let Type::Path(tp) = inner else { return false };

    // Qualified paths (`crate::schema::Foo`, `mod::Bar`) — assume custom.
    if tp.qself.is_some() || tp.path.segments.len() > 1 {
        return true;
    }

    let Some(seg) = tp.path.segments.last() else { return false };

    // `Option<…>` lands in the query slot, not the body slot.
    if seg.ident == "Option" && matches!(seg.arguments, PathArguments::AngleBracketed(_)) {
        return false;
    }

    let name = seg.ident.to_string();
    !is_id_like_primitive(&name)
}

/// Allowlist of single-segment ident names that the HTTP path-extractor
/// knows how to handle as a URL path segment.
///
/// Mirrors the `match` table at `src/servers/generators/http.rs:603-625`
/// which currently picks the extractor type. Keep these two lists in sync
/// — any ident added here must also have a path-extractor mapping there,
/// or the generator falls back to `String` extraction and produces wrong
/// code at runtime.
fn is_id_like_primitive(name: &str) -> bool {
    matches!(
        name,
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
            | "String"
            | "str"
            | "Uuid"
    )
}

/// Derive the child URL segment from the function name suffix.
///
/// `list_skills` → "skills" (already plural), `add_role` → "roles", `remove_runtime_target` → "runtime-targets".
/// Snake_case is converted to kebab-case for URL segments.
fn junction_child_segment(suffix: &str, already_plural: bool) -> String {
    let plural = if already_plural { suffix.to_string() } else { pluralize(suffix) };
    plural.replace('_', "-")
}
