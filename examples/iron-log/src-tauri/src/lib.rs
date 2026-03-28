pub mod api;
pub mod persistence;
pub mod schema;
pub mod store;

use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::schema::AppError;
use crate::store::Store;

/// Application state shared across all handlers.
pub struct AppState {
    store: Store,
}

impl AppState {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self {
            store: Store::new(db),
        }
    }

    /// Access the store. Generated IPC handlers call this.
    pub async fn store(&self) -> Result<&Store, AppError> {
        Ok(&self.store)
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // TODO: Initialize SQLite database and AppState
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
