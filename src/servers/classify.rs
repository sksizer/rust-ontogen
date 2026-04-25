//! Operation classification and parameter analysis.

use ontogen_core::ir::OpKind;
use ontogen_core::naming::pluralize;

use crate::servers::parse::{ApiFn, Param};

/// Classify a function into an operation kind.
pub fn classify_op(func: &ApiFn) -> OpKind {
    classify_by_name_and_params(&func.name, &func.params)
}

/// Classify a function by name and parameters.
///
/// Lower-level entry point used when an `ApiFn` is not available
/// (e.g., the API layer's IR conversion).
pub fn classify_by_name_and_params(name: &str, params: &[Param]) -> OpKind {
    match name {
        "list" => OpKind::List,
        "get_by_id" => OpKind::GetById,
        "create" => OpKind::Create,
        "update" => OpKind::Update,
        "delete" => OpKind::Delete,
        _ => {
            // Junction: add_{child}(parent_id, child_id) — exactly 2 params
            if let Some(rest) = name.strip_prefix("add_")
                && params.len() == 2
            {
                return OpKind::JunctionAdd { child_segment: junction_child_segment(rest, false) };
            }

            // Junction: remove_{child}(parent_id, child_id) — exactly 2 params
            if let Some(rest) = name.strip_prefix("remove_")
                && params.len() == 2
            {
                return OpKind::JunctionRemove { child_segment: junction_child_segment(rest, false) };
            }

            // Junction: list_{children}(parent_id) — exactly 1 param, not "list" itself
            if let Some(rest) = name.strip_prefix("list_")
                && params.len() == 1
            {
                return OpKind::JunctionList { child_segment: junction_child_segment(rest, true) };
            }

            if name.starts_with("get_") || params.is_empty() { OpKind::CustomGet } else { OpKind::CustomPost }
        }
    }
}

/// Returns true if a function is a read-only operation (should use GET).
pub fn is_read_operation(name: &str) -> bool {
    name.starts_with("get_") || name == "list" || name.starts_with("list_") || name.starts_with("detect_")
}

/// Derive the child URL segment from the function name suffix.
///
/// `list_skills` → "skills" (already plural), `add_role` → "roles", `remove_runtime_target` → "runtime-targets".
/// Snake_case is converted to kebab-case for URL segments.
fn junction_child_segment(suffix: &str, already_plural: bool) -> String {
    let plural = if already_plural { suffix.to_string() } else { pluralize(suffix) };
    plural.replace('_', "-")
}
