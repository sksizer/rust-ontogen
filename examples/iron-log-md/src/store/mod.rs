//! The hand-written half of the store. Diff against iron-log's
//! `src/store/mod.rs` to see the entire consumer delta of the markdown
//! backend: `VaultHandle` where iron-log holds `db: Arc<DatabaseConnection>`,
//! a `vault()` accessor where iron-log has `db()`, and **no**
//! `sync_junction`/`load_junction_ids` — many-to-many lives in frontmatter.

pub mod generated;
pub mod hooks;

pub use generated::*;

use crate::schema::{ChangeOp, EntityKind};

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

    /// Called by generated CRUD methods — the markdown analog of `db()`.
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
