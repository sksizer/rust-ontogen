//! name-safety: a compile fixture whose schema names entities after items
//! the generated code imports or defines (`Document`, `Request`, `Response`,
//! `Relationship`, `Links`, `Method`, `Endpoint`, `Event`, `State`, `Value`,
//! `Doc`, `Map`, `Path`), after crates it names (`Serde`, `Axum`,
//! `MarkdownStore`, whose modules are `serde`, `axum`, `markdown_store`), one
//! entity after a Rust keyword (`Match`, whose modules are `r#match`), and its
//! relationships after Rust keywords (`match_id`, `ref_id`, `r#loop`, `r#in`,
//! and `crate_id`, `self_id`, `super_id`, whose relationships `crate`, `self`
//! and `super` have no raw spelling). `build.rs` runs the whole pipeline
//! (markdown store, API, HTTP and MCP servers, TypeScript clients): building
//! the crate is the Rust test, and `just test-ts-clients` type-checks the
//! emitted TypeScript.

pub mod api;
pub mod persistence;
pub mod schema;
pub mod store;

use schema::{AppError, Doc, Event};
use store::Store;
use tokio::sync::broadcast;

pub struct AppState {
    store: Store,
    /// What `api::lookup::event_feed` subscribers receive: an entity named
    /// like the SSE frame type.
    pub event_feed: broadcast::Sender<Event>,
    /// What `api::lookup::watch_doc` subscribers receive.
    pub doc_feed: broadcast::Sender<Doc>,
}

impl AppState {
    pub fn new(vault: markdown_store::VaultHandle) -> Self {
        Self { store: Store::new(vault), event_feed: broadcast::channel(16).0, doc_feed: broadcast::channel(16).0 }
    }

    pub async fn store(&self) -> Result<&Store, AppError> {
        Ok(&self.store)
    }
}
