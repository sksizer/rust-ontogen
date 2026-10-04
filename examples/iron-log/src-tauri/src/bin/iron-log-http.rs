//! Serve iron-log's generated HTTP API without the Tauri shell.
//!
//! ```sh
//! cargo run --bin iron-log-http                    # http://127.0.0.1:3004, in-memory SQLite
//! PORT=39104 cargo run --bin iron-log-http         # any other port
//! IRON_LOG_DB=iron-log.sqlite cargo run --bin iron-log-http   # keep the data in a file
//! curl -s localhost:3004/api/workouts | jq
//! ```

use std::sync::Arc;

use iron_log::AppState;
use iron_log::persistence::db::entities::{exercise, tag, workout, workout_set, workout_tags};
use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sea_orm::{ConnectionTrait, DatabaseConnection, Schema, SqlxSqliteConnector};

#[tokio::main]
async fn main() {
    let addr = format!("127.0.0.1:{}", port(3004));
    let db = open_db().await;
    create_tables(&db).await;
    let state = Arc::new(AppState::new(Arc::new(db)));

    let app = iron_log::api::transport::http::generated::entity_routes().with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    let addr = listener.local_addr().expect("local address");
    println!("iron-log serving its HTTP API at http://{addr} (try /api/workouts)");
    axum::serve(listener, app).await.expect("serve");
}

/// SQLite in memory, or the file `IRON_LOG_DB` names (created if missing).
///
/// One connection that is never recycled: an in-memory database lives only
/// as long as its connection, and the pool's default idle and lifetime
/// limits would otherwise drop every record after a few quiet minutes.
/// SQLite enforces the entities' foreign keys (sqlx's default), so deleting
/// a row that another row still points at fails.
async fn open_db() -> DatabaseConnection {
    let options = match std::env::var("IRON_LOG_DB") {
        Ok(path) => SqliteConnectOptions::new().filename(path).create_if_missing(true),
        Err(std::env::VarError::NotPresent) => SqliteConnectOptions::new().in_memory(true),
        Err(e) => panic!("IRON_LOG_DB: {e}"),
    };
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await
        .expect("open SQLite");
    SqlxSqliteConnector::from_sqlx_sqlite_pool(pool)
}

/// Create a table for every generated entity, junction table included.
///
/// The app ships no migrations, so the generated entities are the schema.
/// Parents come before the tables whose foreign keys name them.
async fn create_tables(db: &DatabaseConnection) {
    let backend = db.get_database_backend();
    let schema = Schema::new(backend);
    let tables = [
        schema.create_table_from_entity(exercise::Entity),
        schema.create_table_from_entity(tag::Entity),
        schema.create_table_from_entity(workout::Entity),
        schema.create_table_from_entity(workout_set::Entity),
        schema.create_table_from_entity(workout_tags::Entity),
    ];
    for mut table in tables {
        db.execute(backend.build(table.if_not_exists())).await.expect("create table");
    }
}

/// The port to serve on: `PORT` when set, otherwise `default`, which stays
/// clear of the markdown examples' 3001-3003. A value that is not a port
/// number stops the server instead of quietly binding somewhere else.
fn port(default: u16) -> u16 {
    match std::env::var("PORT") {
        Ok(v) => v.parse().unwrap_or_else(|_| panic!("PORT must be a port number (0-65535), got {v:?}")),
        Err(std::env::VarError::NotPresent) => default,
        Err(e) => panic!("PORT: {e}"),
    }
}
