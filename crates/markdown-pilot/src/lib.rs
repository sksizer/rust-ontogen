//! markdown-pilot — the in-workspace pilot consumer for ADR 0001's markdown
//! store backend.
//!
//! Exists so the root workspace's CI **compiles and executes** the generated
//! markdown store on every run (the `examples/` apps live outside the
//! workspace and are invisible to CI). `build.rs` runs the full pipeline —
//! schema → markdown_io → dtos → store → api — and the committed
//! `generated/` trees are exactly what it produces; the smoke tests in
//! `tests/` drive the generated CRUD over a real temp vault. The
//! hand-written modules in `src/api` give the generated HTTP router one of
//! every other route kind: custom ops, junction ops, CRUD in a module with
//! no entity, and event streams.

pub mod api;
pub mod persistence;
pub mod schema;
pub mod store;

use std::sync::Mutex;

use tokio::sync::broadcast;

pub use store::Store;

/// The state the API layer and its HTTP handlers run against.
pub struct AppState {
    pub store: Store,
    /// The `bookmark` API module's data: it has no entity, so no store.
    pub bookmarks: Mutex<Vec<schema::Bookmark>>,
    /// What `api::task::task_feed` subscribers receive.
    pub task_feed: broadcast::Sender<schema::Task>,
    /// What `api::bookmark::bookmark_feed` subscribers receive.
    pub bookmark_feed: broadcast::Sender<schema::Bookmark>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        AppState {
            store,
            bookmarks: Mutex::new(Vec::new()),
            task_feed: broadcast::channel(16).0,
            bookmark_feed: broadcast::channel(16).0,
        }
    }

    /// Store accessor the generated HTTP handlers call (`state.store().await`).
    pub async fn store(&self) -> Result<&Store, schema::AppError> {
        Ok(&self.store)
    }
}
