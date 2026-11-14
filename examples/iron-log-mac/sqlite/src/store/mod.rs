//! The hand-written half of the SQLite store: the connection, the change
//! channel, and the junction helpers the generated store calls.

pub mod generated;
pub mod hooks;

pub use generated::*;

use std::sync::Arc;

use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

use crate::schema::{AppError, ChangeOp, EntityKind};

#[derive(Debug, Clone)]
pub struct EntityChange {
    pub op: ChangeOp,
    pub kind: EntityKind,
    pub id: String,
}

pub struct Store {
    pub db: Arc<DatabaseConnection>,
    change_tx: tokio::sync::broadcast::Sender<EntityChange>,
}

impl Store {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        let (change_tx, _) = tokio::sync::broadcast::channel(256);
        Self { db, change_tx }
    }

    /// Called by generated CRUD methods.
    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    pub fn emit_change(&self, op: ChangeOp, kind: EntityKind, id: String) {
        let _ = self.change_tx.send(EntityChange { op, kind, id });
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<EntityChange> {
        self.change_tx.subscribe()
    }

    /// Replace a many-to-many junction's rows for one source entity.
    pub async fn sync_junction(
        &self,
        table: &str,
        source_col: &str,
        target_col: &str,
        source_id: &str,
        target_ids: &[String],
    ) -> Result<(), AppError> {
        let run = |sql: String, values: Vec<sea_orm::Value>| async move {
            self.db()
                .execute(Statement::from_sql_and_values(sea_orm::DatabaseBackend::Sqlite, sql, values))
                .await
                .map_err(|e| AppError::DbError(e.to_string()))
        };
        run(format!("DELETE FROM {table} WHERE {source_col} = ?"), vec![source_id.into()]).await?;
        for target_id in target_ids {
            run(
                format!("INSERT INTO {table} ({source_col}, {target_col}) VALUES (?, ?)"),
                vec![source_id.into(), target_id.into()],
            )
            .await?;
        }
        Ok(())
    }

    /// Read a many-to-many junction's target ids for one source entity.
    pub async fn load_junction_ids(
        &self,
        table: &str,
        source_col: &str,
        target_col: &str,
        source_id: &str,
    ) -> Result<Vec<String>, AppError> {
        let rows = self
            .db()
            .query_all(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                format!("SELECT {target_col} FROM {table} WHERE {source_col} = ?"),
                vec![source_id.into()],
            ))
            .await
            .map_err(|e| AppError::DbError(e.to_string()))?;
        Ok(rows.iter().filter_map(|row| row.try_get_by_index::<String>(0).ok()).collect())
    }
}
