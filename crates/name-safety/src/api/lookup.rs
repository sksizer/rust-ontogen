//! Custom ops whose arguments are named after Rust keywords (`r#type`,
//! `r#in`), JavaScript reserved words (`class`, `new`, `default`) or start
//! with an underscore (`_kind`): every transport and client must still name
//! them on the wire as Tauri, serde and the HTTP query do.

use crate::schema::{AppError, Doc};
use crate::store::Store;

/// The docs whose title contains `r#in` (when given) and starts with
/// `class` (when given). `r#type` and `_kind` are accepted and ignored.
pub async fn find_docs(
    store: &Store,
    r#type: Option<String>,
    r#in: Option<String>,
    class: Option<String>,
    _kind: Option<String>,
) -> Result<Vec<Doc>, AppError> {
    let _ = r#type;
    Ok(store
        .list_docs(&[], None, None)
        .await?
        .into_iter()
        .filter(|d| r#in.as_deref().is_none_or(|s| d.title.contains(s)))
        .filter(|d| class.as_deref().is_none_or(|s| d.title.starts_with(s)))
        .collect())
}

/// Retitle a doc to `new`, or to `default` when `new` is empty.
pub async fn retitle(store: &Store, id: &str, new: String, default: Option<String>) -> Result<Doc, AppError> {
    let title = if new.is_empty() { default.unwrap_or_default() } else { new };
    let updates = crate::store::doc::DocUpdate { title: Some(title) };
    store.update_doc(id, updates).await
}
