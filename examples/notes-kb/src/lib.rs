//! notes-kb — an Obsidian-vault-shaped knowledge base on the markdown
//! backend: wikilinked notes, the generated API, and a graph view.

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

    pub async fn store(&self) -> Result<&Store, AppError> {
        Ok(&self.store)
    }
}
