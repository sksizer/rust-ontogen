//! The API crate: owns the handlers, the app state and the types. It has no
//! Tauri dependency; the shell crate generates the IPC transport over it.

pub mod api;
pub mod schema;
pub mod store;

/// Shared application state the handlers and the IPC commands take.
pub struct AppState {
    store: store::Store,
    /// Where `api::v1::feed::note_changed` subscribers listen.
    pub notes: tokio::sync::broadcast::Sender<schema::Note>,
}

impl Default for AppState {
    fn default() -> Self {
        Self { store: store::Store, notes: tokio::sync::broadcast::channel(16).0 }
    }
}

impl AppState {
    /// The store the entity-scoped handlers operate on.
    pub async fn store(&self) -> Result<store::Store, String> {
        Ok(self.store.clone())
    }
}
