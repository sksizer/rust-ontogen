use serde::{Deserialize, Serialize};

/// A saved link. Not an entity: the pilot keeps bookmarks in memory, and
/// the `bookmark` API module serves its CRUD-named fns as custom ops.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct CreateBookmarkInput {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct UpdateBookmarkInput {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}
