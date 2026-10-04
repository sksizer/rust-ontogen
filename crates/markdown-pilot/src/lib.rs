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
//! no entity, filtered lists, and event streams. The same API is generated
//! a second time under a route prefix (`api::transport::http_scoped`).

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

    /// Store accessor of the scoped router (`route_prefix`
    /// `projects/:project_id`): [`PROJECT`] is served from the one store,
    /// and any other project is an error.
    pub fn store_for(&self, project_id: &str) -> Result<&Store, schema::AppError> {
        if project_id == PROJECT {
            Ok(&self.store)
        } else {
            Err(schema::AppError::Md(format!("no such project: {project_id}")))
        }
    }

    /// The scoped router's `bookmark_feed`: every project shares the feed.
    pub fn subscribe_bookmark_feed_for(&self, _project_id: &str) -> broadcast::Receiver<schema::Bookmark> {
        self.bookmark_feed.subscribe()
    }

    /// The scoped router's `task_feed`: every project shares the feed.
    pub fn subscribe_task_feed_for(&self, _project_id: &str) -> broadcast::Receiver<schema::Task> {
        self.task_feed.subscribe()
    }

    /// The scoped router's `watch_task`: the task must exist in the
    /// project's store.
    pub async fn subscribe_watch_task_for(
        &self,
        project_id: &str,
        id: String,
    ) -> Result<broadcast::Receiver<schema::Task>, schema::AppError> {
        self.store_for(project_id)?.get_task(&id).await?;
        Ok(self.task_feed.subscribe())
    }
}

/// The one project the scoped router serves.
pub const PROJECT: &str = "pilot";
