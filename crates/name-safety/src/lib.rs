//! name-safety: a compile fixture whose schema names entities after items
//! the generated code imports or defines (`Document`, `Request`, `Response`,
//! `Relationship`, `Links`, `Method`, `Endpoint`, `Event`, `State`, `Value`,
//! `Doc`, `Map`), one entity after a Rust keyword (`Match`, whose modules are
//! `r#match`), and its relationships after Rust keywords (`match_id`,
//! `ref_id`, `r#loop`, `r#in`). `build.rs` runs the whole pipeline (markdown
//! store, API, HTTP and MCP servers, TypeScript clients): building the crate
//! is the Rust test, and `just test-ts-clients` type-checks the emitted
//! TypeScript.

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
