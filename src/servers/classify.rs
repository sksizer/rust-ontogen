//! Operation classification and parameter analysis.

use ontogen_core::ir::OpKind;
use ontogen_core::naming::pluralize;
use syn::{PathArguments, Type};

use crate::servers::parse::{ApiFn, Param};

/// Classify a function into an operation kind.
///
/// Source-side `#[ontogen::post]` short-circuits the heuristic and returns
/// `OpKind::CustomPost` unconditionally. The escape hatch lets consumers
/// force POST routing on action-verb functions whose zero-user-param shape
/// would otherwise route as GET (e.g. `pause(state)`, `reset_all(state)`).
/// No-op for any function without the attribute — the default classifier
/// behaviour is unchanged.
pub fn classify_op(func: &ApiFn) -> OpKind {
    if func.force_post {
        return OpKind::CustomPost;
    }
    classify_by_name_and_params(&func.name, &func.params)
}

/// Classify a function by name and parameters.
///
/// Lower-level entry point used when an `ApiFn` is not available
/// (e.g., the API layer's IR conversion).
///
/// # `get_*` handling
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
///
/// Zero-param `get_*` functions stay `CustomGet` (no body needed).
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

            // Zero-param custom fns are always read-shaped (no body to carry).
            if params.is_empty() {
                return OpKind::CustomGet;
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

/// Returns true if a classified op should use HTTP `GET`.
///
/// Drives method selection in the HTTP server emitter, the TS client
/// emitter, and the api-layer IR. Single source of truth — replaces the
/// older name-based `is_read_operation` heuristic that diverged from
/// classification once the classifier became AST-aware (OF-016).
pub fn is_read_op(op: &OpKind) -> bool {
    matches!(op, OpKind::List | OpKind::GetById | OpKind::CustomGet | OpKind::JunctionList { .. })
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
