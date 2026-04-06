//! Operation classification and parameter analysis.

use crate::servers::parse::ApiFn;
use ontogen_core::naming::pluralize;

/// The kind of operation a function represents.
#[derive(Debug)]
pub enum OpKind {
    /// `list()` — returns all entities.
    List,
    /// `get_by_id(id)` — returns a single entity by ID.
    GetById,
    /// `create(input)` — creates a new entity.
    Create,
    /// `update(id, input)` — updates an entity by ID.
    UpdateById,
    /// `delete(id)` — deletes an entity by ID.
    DeleteById,
    /// `list_{children}(parent_id)` — list child entities of a parent.
    /// Generates `GET /api/{parents}/:parent_id/{children}`.
    JunctionList {
        /// URL segment for the child collection, e.g. "roles".
        child_segment: String,
    },
    /// `add_{child}(parent_id, child_id)` — add a child to a parent.
    /// Generates `POST /api/{parents}/:parent_id/{children}`.
    JunctionAdd {
        /// URL segment for the child collection, e.g. "roles".
        child_segment: String,
    },
    /// `remove_{child}(parent_id, child_id)` — remove a child from a parent.
    /// Generates `DELETE /api/{parents}/:parent_id/{children}/:child_id`.
    JunctionRemove {
        /// URL segment for the child collection, e.g. "roles".
        child_segment: String,
    },
    /// Custom read operation (inferred from `get_` prefix or no params).
    CustomGet,
    /// Custom write/mutation operation.
    CustomPost,
}

/// Classify a function into an operation kind.
pub fn classify_op(func: &ApiFn) -> OpKind {
    match func.name.as_str() {
        "list" => OpKind::List,
        "get_by_id" => OpKind::GetById,
        "create" => OpKind::Create,
        "update" => OpKind::UpdateById,
        "delete" => OpKind::DeleteById,
        _ => {
            let name = &func.name;

            // Junction: add_{child}(parent_id, child_id) — exactly 2 string params
            if let Some(rest) = name.strip_prefix("add_") {
                if func.params.len() == 2 {
                    return OpKind::JunctionAdd {
                        child_segment: junction_child_segment(rest, false),
                    };
                }
            }

            // Junction: remove_{child}(parent_id, child_id) — exactly 2 string params
            if let Some(rest) = name.strip_prefix("remove_") {
                if func.params.len() == 2 {
                    return OpKind::JunctionRemove {
                        child_segment: junction_child_segment(rest, false),
                    };
                }
            }

            // Junction: list_{children}(parent_id) — exactly 1 param, not "list" itself
            if let Some(rest) = name.strip_prefix("list_") {
                if func.params.len() == 1 {
                    return OpKind::JunctionList {
                        child_segment: junction_child_segment(rest, true),
                    };
                }
            }

            if name.starts_with("get_") || func.params.is_empty() { OpKind::CustomGet } else { OpKind::CustomPost }
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
