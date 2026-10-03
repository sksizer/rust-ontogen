//! The consumer half of the SeaORM store: the connection and change
//! channel the generated `impl Store` blocks call, plus table creation.
//! `Fixed` has no many_to_many, so no junction helpers.

pub mod generated;
pub mod hooks;

pub use generated::*;

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Schema};

use crate::persistence::db::entities as tables;
use crate::schema::{AppError, ChangeOp, EntityKind};

#[derive(Debug, Clone)]
pub struct EntityChange {
    pub op: ChangeOp,
    pub kind: EntityKind,
    pub id: String,
}

pub struct Store {
    db: DatabaseConnection,
    change_tx: tokio::sync::broadcast::Sender<EntityChange>,
}

impl Store {
    /// A store over a fresh SQLite database in memory, with its table
    /// created from the generated entity.
    pub async fn open_in_memory() -> Result<Self, AppError> {
        // One connection: each SQLite `:memory:` connection is its own
        // database, so a pool of several would see different data.
        let mut options = sea_orm::ConnectOptions::new("sqlite::memory:");
        options.max_connections(1).min_connections(1).sqlx_logging(false);
        let db = Database::connect(options).await.map_err(db_error)?;
        let backend = db.get_database_backend();
        let table = Schema::new(backend).create_table_from_entity(tables::fixed::Entity);
        db.execute(backend.build(&table)).await.map_err(db_error)?;
        let (change_tx, _) = tokio::sync::broadcast::channel(256);
        Ok(Self { db, change_tx })
    }

    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    pub fn emit_change(&self, op: ChangeOp, kind: EntityKind, id: String) {
        let _ = self.change_tx.send(EntityChange { op, kind, id });
    }
}

fn db_error(e: impl std::fmt::Display) -> AppError {
    AppError::DbError(e.to_string())
}
