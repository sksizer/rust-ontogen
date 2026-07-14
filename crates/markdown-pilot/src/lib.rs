//! markdown-pilot — the in-workspace pilot consumer for ADR 0001's markdown
//! store backend.
//!
//! Exists so the root workspace's CI **compiles and executes** the generated
//! markdown store on every run (the `examples/` apps live outside the
//! workspace and are invisible to CI). `build.rs` runs the full pipeline —
//! schema → markdown_io → dtos → store → api — and the committed
//! `generated/` trees are exactly what it produces; the smoke tests in
//! `tests/` drive the generated CRUD over a real temp vault.

pub mod api;
pub mod persistence;
pub mod schema;
pub mod store;

pub use store::Store;

/// Minimal app-state shape for the generated API layer's signatures.
pub struct AppState {
    pub store: Store,
}

impl AppState {
    /// Store accessor the generated HTTP handlers call (`state.store().await`).
    pub async fn store(&self) -> Result<&Store, schema::AppError> {
        Ok(&self.store)
    }
}
