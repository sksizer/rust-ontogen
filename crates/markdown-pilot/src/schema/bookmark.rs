use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A saved link. Not an entity: the pilot keeps bookmarks in memory, and
/// the `bookmark` API module serves its CRUD-named fns as custom ops.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, specta::Type)]
pub struct CreateBookmarkInput {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, specta::Type)]
pub struct UpdateBookmarkInput {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

/// The filter of the bookmark list; an absent field matches every bookmark.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct BookmarkQuery {
    pub url_contains: Option<String>,
}
