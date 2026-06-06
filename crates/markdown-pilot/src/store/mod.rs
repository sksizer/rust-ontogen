//! The hand-written half of the store: the consumer contract the generated
//! `impl Store` blocks extend. For the markdown backend that is a
//! `VaultHandle` (where a SeaORM consumer holds `db`) plus the change
//! channel — no `sync_junction`/`load_junction_ids`: m2m lives in
//! frontmatter.

pub mod generated;
pub mod hooks;

pub use generated::*;

use crate::schema::{ChangeOp, EntityKind};

/// A change event broadcast after every successful mutation.
#[derive(Debug, Clone)]
pub struct EntityChange {
    pub op: ChangeOp,
    pub kind: EntityKind,
    pub id: String,
}

pub struct Store {
    vault: markdown_store::VaultHandle,
    change_tx: tokio::sync::broadcast::Sender<EntityChange>,
}

impl Store {
    pub fn new(vault: markdown_store::VaultHandle) -> Self {
        let (change_tx, _) = tokio::sync::broadcast::channel(256);
        Self { vault, change_tx }
    }

    /// The vault accessor generated CRUD code calls — the markdown analog of
    /// the SeaORM consumer's `db()`.
    pub fn vault(&self) -> &markdown_store::VaultHandle {
        &self.vault
    }

    pub fn emit_change(&self, op: ChangeOp, kind: EntityKind, id: String) {
        let _ = self.change_tx.send(EntityChange { op, kind, id });
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<EntityChange> {
        self.change_tx.subscribe()
    }
}
