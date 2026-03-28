pub mod generated;
pub mod hooks;

pub use generated::*;

use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::broadcast;

use crate::schema::{ChangeOp, EntityKind};

/// Central store providing CRUD access to all entities.
///
/// Generated code calls `self.db` for database access and
/// `self.emit_change()` for event notification.
pub struct Store {
    pub db: Arc<DatabaseConnection>,
    change_tx: broadcast::Sender<EntityChange>,
}

#[derive(Debug, Clone)]
pub struct EntityChange {
    pub op: ChangeOp,
    pub kind: EntityKind,
    pub id: String,
}

impl Store {
    /// Access the database connection. Called by generated CRUD methods.
    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        let (change_tx, _) = broadcast::channel(256);
        Self { db, change_tx }
    }

    /// Emit a change event. Called by generated CRUD methods.
    pub fn emit_change(&self, op: ChangeOp, kind: EntityKind, id: String) {
        let _ = self.change_tx.send(EntityChange { op, kind, id });
    }

    /// Subscribe to change events.
    pub fn subscribe(&self) -> broadcast::Receiver<EntityChange> {
        self.change_tx.subscribe()
    }

    /// Sync a many-to-many junction table: delete all existing rows for the
    /// source entity, then insert the new set.
    pub async fn sync_junction(
        &self,
        table: &str,
        source_col: &str,
        target_col: &str,
        source_id: &str,
        target_ids: &[String],
    ) -> Result<(), crate::schema::AppError> {
        use sea_orm::{ConnectionTrait, Statement};

        let db = self.db();

        // Delete existing junction rows
        let delete_sql = format!("DELETE FROM {table} WHERE {source_col} = ?");
        db.execute(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            &delete_sql,
            vec![source_id.into()],
        ))
        .await
        .map_err(|e| crate::schema::AppError::DbError(e.to_string()))?;

        // Insert new junction rows
        for target_id in target_ids {
            let insert_sql =
                format!("INSERT INTO {table} ({source_col}, {target_col}) VALUES (?, ?)");
            db.execute(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                &insert_sql,
                vec![source_id.into(), target_id.into()],
            ))
            .await
            .map_err(|e| crate::schema::AppError::DbError(e.to_string()))?;
        }

        Ok(())
    }

    /// Load target IDs from a junction table for a given source entity.
    pub async fn load_junction_ids(
        &self,
        table: &str,
        source_col: &str,
        target_col: &str,
        source_id: &str,
    ) -> Result<Vec<String>, crate::schema::AppError> {
        use sea_orm::{ConnectionTrait, Statement};

        let sql = format!("SELECT {target_col} FROM {table} WHERE {source_col} = ?");
        let rows = self
            .db()
            .query_all(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                &sql,
                vec![source_id.into()],
            ))
            .await
            .map_err(|e| crate::schema::AppError::DbError(e.to_string()))?;

        let ids: Vec<String> = rows
            .iter()
            .filter_map(|row| {
                row.try_get_by_index::<String>(0).ok()
            })
            .collect();

        Ok(ids)
    }
}
