//! Operation classification and parameter analysis.

use crate::servers::parse::ApiFn;

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
            if name.starts_with("get_") || func.params.is_empty() { OpKind::CustomGet } else { OpKind::CustomPost }
        }
    }
}

/// Returns true if a function is a read-only operation (should use GET).
pub fn is_read_operation(name: &str) -> bool {
    name.starts_with("get_") || name == "list" || name.starts_with("detect_")
}
