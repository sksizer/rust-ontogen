//! The section list filtered by a query struct and a required parent. It
//! replaces the generated `list` and `count`; the rest of the module is the
//! generated one.

pub use super::generated::section::*;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::schema::{AppError, Section};
use crate::store::Store;

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
}

fn matches(section: &Section, query: &ListSectionsQuery, parent_id: &str) -> bool {
    let children = section.children.len();
    section.parent_id == parent_id
        && query.title_contains.as_deref().is_none_or(|t| section.title.contains(t))
        && query.min_children.is_none_or(|n| children >= n as usize)
        && query.max_children.is_none_or(|n| children <= n as usize)
}

/// One page of the sections under `parent_id` that `query` selects.
pub async fn list(
    store: &Store,
    query: ListSectionsQuery,
    parent_id: &str,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<Section>, AppError> {
    let sections = store.list_sections(&[], None, None).await?;
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
