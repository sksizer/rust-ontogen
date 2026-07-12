//! iron-log-md — iron-log's exact schema on the markdown store backend.

pub mod api;
pub mod persistence;
pub mod schema;
pub mod store;

use schema::AppError;
use store::Store;

pub struct AppState {
    store: Store,
}

impl AppState {
    pub fn new(vault: markdown_store::VaultHandle) -> Self {
        Self { store: Store::new(vault) }
    }

    /// Accessor the generated HTTP handlers call.
    pub async fn store(&self) -> Result<&Store, AppError> {
        Ok(&self.store)
    }
}
