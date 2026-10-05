//! The consumer half of the SeaORM store: what the generated `impl Store`
//! blocks call but ontogen does not emit — the connection, the change
//! channel, the many_to_many junction helpers — plus table creation.

pub mod generated;
pub mod hooks;

pub use generated::*;

use sea_orm::{ConnectionTrait, Database, DatabaseConnection, EntityTrait, Schema, Statement};

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
    /// A store over a fresh SQLite database in memory, with every table
    /// created from the generated entities.
    pub async fn open_in_memory() -> Result<Self, AppError> {
        // One connection: each SQLite `:memory:` connection is its own
        // database, so a pool of several would see different data.
        // sqlite-only: the parity harness runs the SeaORM store on in-memory SQLite.
        let mut options = sea_orm::ConnectOptions::new("sqlite::memory:");
        options.max_connections(1).min_connections(1).sqlx_logging(false);
        let db = Database::connect(options).await.map_err(db_error)?;
        create_table(&db, tables::fixed::Entity).await?;
        create_table(&db, tables::item::Entity).await?;
        create_table(&db, tables::tag::Entity).await?;
        create_table(&db, tables::item_tags::Entity).await?;
        create_table(&db, tables::section::Entity).await?;
        let (change_tx, _) = tokio::sync::broadcast::channel(256);
        Ok(Self { db, change_tx })
    }

    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    pub fn emit_change(&self, op: ChangeOp, kind: EntityKind, id: String) {
        let _ = self.change_tx.send(EntityChange { op, kind, id });
    }

    /// Replace the junction rows of `source_id`, inserting `target_ids` in
    /// list order so `load_junction_ids` reads them back in that order.
    /// Every statement goes through `conn`, the generated create's or
    /// update's transaction, so a failure undoes the whole write.
    pub async fn sync_junction<C: ConnectionTrait>(
        &self,
        conn: &C,
        table: &str,
        source_col: &str,
        target_col: &str,
        source_id: &str,
        target_ids: &[String],
    ) -> Result<(), AppError> {
        // sqlite-only: list order survives only because SQLite's rowid follows insertion order.
        let delete = format!("DELETE FROM {table} WHERE {source_col} = ?");
        conn.execute(sqlite(&delete, vec![source_id.into()])).await.map_err(db_error)?;
        let insert = format!("INSERT INTO {table} ({source_col}, {target_col}) VALUES (?, ?)");
        for target_id in target_ids {
            conn.execute(sqlite(&insert, vec![source_id.into(), target_id.as_str().into()])).await.map_err(db_error)?;
        }
        Ok(())
    }

    /// The junction targets of `source_id` in the order they were written:
    /// many_to_many linkage keeps its written order on both backends
    /// (ADR 0006 §4), and `rowid` is SQLite's insertion order.
    pub async fn load_junction_ids(
        &self,
        table: &str,
        source_col: &str,
        target_col: &str,
        source_id: &str,
    ) -> Result<Vec<String>, AppError> {
        // sqlite-only: `rowid` is SQLite's implicit insertion-order column.
        let select = format!("SELECT {target_col} FROM {table} WHERE {source_col} = ? ORDER BY rowid");
        let rows = self.db.query_all(sqlite(&select, vec![source_id.into()])).await.map_err(db_error)?;
        rows.iter().map(|row| row.try_get_by_index::<String>(0).map_err(db_error)).collect()
    }
}

async fn create_table<E: EntityTrait>(db: &DatabaseConnection, entity: E) -> Result<(), AppError> {
    let backend = db.get_database_backend();
    let statement = Schema::new(backend).create_table_from_entity(entity);
    db.execute(backend.build(&statement)).await.map_err(db_error)?;
    Ok(())
}

// sqlite-only: raw SQL built for DatabaseBackend::Sqlite, with `?` placeholders.
fn sqlite(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(sea_orm::DatabaseBackend::Sqlite, sql, values)
}

fn db_error(e: impl std::fmt::Display) -> AppError {
    AppError::DbError(e.to_string())
}
