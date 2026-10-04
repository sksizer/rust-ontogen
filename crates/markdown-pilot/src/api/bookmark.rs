//! Bookmarks: CRUD-named fns over the in-memory list in `AppState`, in a
//! module no entity stands behind.

use tokio::sync::broadcast;

use crate::AppState;
use crate::schema::{AppError, Bookmark, CreateBookmarkInput, UpdateBookmarkInput};

/// One page of bookmarks, oldest first.
pub async fn list(state: &AppState, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Bookmark>, AppError> {
    let bookmarks = state.bookmarks.lock().expect("bookmarks lock");
    let offset = offset.unwrap_or(0) as usize;
    let limit = limit.map_or(usize::MAX, |l| l as usize);
    Ok(bookmarks.iter().skip(offset).take(limit).cloned().collect())
}

/// How many bookmarks there are: the total behind a page of `list`.
pub async fn count(state: &AppState) -> Result<u64, AppError> {
    Ok(state.bookmarks.lock().expect("bookmarks lock").len() as u64)
}

pub async fn get_by_id(state: &AppState, id: &str) -> Result<Bookmark, AppError> {
    let bookmarks = state.bookmarks.lock().expect("bookmarks lock");
    bookmarks.iter().find(|b| b.id == id).cloned().ok_or_else(|| AppError::BookmarkNotFound(id.to_string()))
}

/// Save a bookmark under the next free `bm-{n}` id and publish it.
pub async fn create(state: &AppState, input: CreateBookmarkInput) -> Result<Bookmark, AppError> {
    let mut bookmarks = state.bookmarks.lock().expect("bookmarks lock");
    // Ids only grow, so the last one is the largest.
    let next = bookmarks.last().and_then(|b| b.id.strip_prefix("bm-")?.parse::<u64>().ok()).map_or(1, |n| n + 1);
    let bookmark = Bookmark { id: format!("bm-{next}"), url: input.url, title: input.title };
    bookmarks.push(bookmark.clone());
    // No subscriber is not an error.
    let _ = state.bookmark_feed.send(bookmark.clone());
    Ok(bookmark)
}

pub async fn update(state: &AppState, id: &str, input: UpdateBookmarkInput) -> Result<Bookmark, AppError> {
    let mut bookmarks = state.bookmarks.lock().expect("bookmarks lock");
    let bookmark =
        bookmarks.iter_mut().find(|b| b.id == id).ok_or_else(|| AppError::BookmarkNotFound(id.to_string()))?;
    if let Some(url) = input.url {
        bookmark.url = url;
    }
    if let Some(title) = input.title {
        bookmark.title = title;
    }
    Ok(bookmark.clone())
}

pub async fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let mut bookmarks = state.bookmarks.lock().expect("bookmarks lock");
    let before = bookmarks.len();
    bookmarks.retain(|b| b.id != id);
    if bookmarks.len() == before { Err(AppError::BookmarkNotFound(id.to_string())) } else { Ok(()) }
}

/// Every bookmark as it is created.
pub fn bookmark_feed(state: &AppState) -> broadcast::Receiver<Bookmark> {
    state.bookmark_feed.subscribe()
}
