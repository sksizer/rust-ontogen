//! The section list filtered by a query struct and a required parent, and
//! sorted. It replaces the generated `list` and `count`; the rest of the
//! module is the generated one.

pub use super::generated::section::*;
use ontogen_core::order::OrderBy;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::schema::{AppError, Section};
use crate::store::Store;
use crate::store::section::SectionSortField;

/// Which sections `list` and `count` select; an absent field matches every
/// section. Declared out of byte order, so a reader that walked the fields
/// in declaration order would show in the tests. It refuses a field it does
/// not declare, so a transport must hand it only its own members.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSectionsQuery {
    pub title_contains: Option<String>,
    pub min_children: Option<u32>,
    pub max_children: Option<u32>,
    /// Sections whose title is one of these. `None` matches every section,
    /// and `Some([])` none.
    pub title_in: Option<Vec<String>>,
    /// Sections with one of these numbers of children; empty matches every
    /// section.
    #[serde(default)]
    pub children_in: Vec<u32>,
}

fn matches(section: &Section, query: &ListSectionsQuery, parent_id: &str) -> bool {
    let children = section.children.len();
    section.parent_id == parent_id
        && query.title_contains.as_deref().is_none_or(|t| section.title.contains(t))
        && query.min_children.is_none_or(|n| children >= n as usize)
        && query.max_children.is_none_or(|n| children <= n as usize)
        && query.title_in.as_ref().is_none_or(|titles| titles.contains(&section.title))
        && (query.children_in.is_empty() || query.children_in.iter().any(|&n| children == n as usize))
}

/// One page of the sections under `parent_id` that `query` selects, in
/// `order`.
pub async fn list(
    store: &Store,
    query: ListSectionsQuery,
    parent_id: &str,
    order: &[OrderBy<SectionSortField>],
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<Section>, AppError> {
    // Filtering keeps the store's order (the sort keys, then id), so
    // pages are cut from one stable order.
    let sections = store.list_sections(order, None, None).await?;
    Ok(sections
        .into_iter()
        .filter(|s| matches(s, &query, parent_id))
        .skip(offset.unwrap_or(0) as usize)
        .take(limit.map_or(usize::MAX, |l| l as usize))
        .collect())
}

/// How many sections the same filter selects.
pub async fn count(store: &Store, query: ListSectionsQuery, parent_id: &str) -> Result<u64, AppError> {
    let sections = store.list_sections(&[], None, None).await?;
    Ok(sections.iter().filter(|s| matches(s, &query, parent_id)).count() as u64)
}
