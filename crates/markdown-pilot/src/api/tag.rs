//! The tag list filtered by two optional bare parameters. It replaces the
//! generated `list` and `count`; the rest of the module is the generated one.
//! With neither filter it lists every tag, as the generated list did.

pub use super::generated::tag::*;

use crate::schema::{AppError, Tag};
use crate::store::Store;

fn matches(tag: &Tag, title_prefix: Option<&str>, min_title_len: Option<u32>) -> bool {
    title_prefix.is_none_or(|p| tag.title.starts_with(p))
        && min_title_len.is_none_or(|n| tag.title.chars().count() >= n as usize)
}

/// One page of the tags whose title starts with `title_prefix` and is at
/// least `min_title_len` characters long.
pub async fn list(
    store: &Store,
    title_prefix: Option<&str>,
    min_title_len: Option<u32>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<Tag>, AppError> {
    let tags = store.list_tags(None, None).await?;
    Ok(tags
        .into_iter()
        .filter(|t| matches(t, title_prefix, min_title_len))
        .skip(offset.unwrap_or(0) as usize)
        .take(limit.map_or(usize::MAX, |l| l as usize))
        .collect())
}

/// How many tags the same filter selects.
pub async fn count(store: &Store, title_prefix: Option<&str>, min_title_len: Option<u32>) -> Result<u64, AppError> {
    let tags = store.list_tags(None, None).await?;
    Ok(tags.iter().filter(|t| matches(t, title_prefix, min_title_len)).count() as u64)
}
