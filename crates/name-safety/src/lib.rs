//! name-safety: the compile fixture for generated-code name safety.
//!
//! What it guards: its schema takes names that generated code once bound
//! bare or wrote unescaped, so a regression fails the root CI build.
//! - Entities named after items the output imports or defines: `Document`,
//!   `Request`, `Response`, `Relationship`, `Links`, `Method`, `Endpoint`,
//!   `Event`, `State`, `Value`, `Doc`, `Map`, `Path`.
//! - Entities whose module is a crate the output names: `Serde`, `Axum`,
//!   `MarkdownStore`.
//! - An entity named after a Rust keyword: `Match` (module `r#match`).
//! - Relationships named after Rust keywords: `match_id`, `ref_id`,
//!   `r#loop`, `r#in`, plus `crate_id`, `self_id` and `super_id`, whose
//!   relationship names have no raw spelling.
//! - Custom and event op arguments named `r#type`, `r#in`, `class`, `new`,
//!   `default` and `_kind`, and `PathBuf`/`&Path` arguments
//!   (`api::lookup`).
//!
//! How it checks: `build.rs` runs the whole pipeline (markdown store, API,
//! HTTP and MCP servers, `HttpTs`, `HttpTauriIpcSplit` and the admin
//! registry). Compiling the crate is the Rust check, and
//! `just test-ts-clients` (part of `just test`) runs `tsc --strict` over
//! `generated-ts/`. There is no Tauri IPC server here, since Tauri is not a
//! workspace dependency: IPC output is covered by the `servers_name_safety_ipc`
//! snapshot and its syn checks in `src/servers/tests.rs`.
//!
//! How to extend it: add the entity to `src/schema` (its module, re-exports
//! and `AppError`/`EntityKind` variants in `schema/mod.rs`, and its module
//! name in `build.rs`'s paginated list), or the argument to an op in
//! `src/api/lookup.rs`. Then run `cargo build -p name-safety`, commit the
//! regenerated trees under `src/` and `generated-ts/`, and run
//! `just test-ts-clients`. A name the generator refuses (see
//! `ontogen`'s `ident` module) belongs in a refusal test, not here.

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
