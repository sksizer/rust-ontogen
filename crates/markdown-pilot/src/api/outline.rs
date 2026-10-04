//! Sections as a flat outline: a filtered list in a module no entity stands
//! behind. It takes the store, so the scoped router serves it under the
//! prefix, where the bookmark list (which takes `AppState`) stays unscoped.

use crate::schema::{AppError, Section};
use crate::store::Store;

fn matches(section: &Section, title_contains: Option<&str>) -> bool {
    title_contains.is_none_or(|t| section.title.contains(t))
}

/// One page of the sections whose title contains `title_contains`.
pub async fn list(
    store: &Store,
    title_contains: Option<String>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<Section>, AppError> {
    let sections = store.list_sections(None, None).await?;
    Ok(sections
        .into_iter()
        .filter(|s| matches(s, title_contains.as_deref()))
        .skip(offset.unwrap_or(0) as usize)
        .take(limit.map_or(usize::MAX, |l| l as usize))
        .collect())
}

/// How many sections the same filter selects.
pub async fn count(store: &Store, title_contains: Option<String>) -> Result<u64, AppError> {
    let sections = store.list_sections(None, None).await?;
    Ok(sections.iter().filter(|s| matches(s, title_contains.as_deref())).count() as u64)
}
