pub mod api;
pub mod persistence;
pub mod schema;
pub mod store;

use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::broadcast;

use crate::schema::{Activity, AppError, EntityKind};
use crate::store::Store;

/// Application state shared across all handlers.
pub struct AppState {
    store: Store,
    activity: broadcast::Sender<Activity>,
}

impl AppState {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self {
            store: Store::new(db),
            activity: broadcast::channel(256).0,
        }
    }

    /// Access the store. Generated IPC handlers call this.
    pub async fn store(&self) -> Result<&Store, AppError> {
        Ok(&self.store)
    }

    /// Publish every store change on the activity feed, numbered from 1.
    /// Runs for as long as the state lives, so the host spawns it once.
    pub async fn publish_activity(&self) {
        let mut changes = self.store.subscribe();
        let mut seq = 0;
        loop {
            match changes.recv().await {
                Ok(change) => {
                    seq += 1;
                    let kind = match change.kind {
                        EntityKind::Exercise => "exercise",
                        EntityKind::Workout => "workout",
                        EntityKind::WorkoutSet => "workout_set",
                        EntityKind::Tag => "tag",
                    };
                    // No subscriber is not an error: the feed is live-only.
                    let _ = self.activity.send(Activity { seq, kind: kind.into(), id: change.id });
                }
                // Changes this task fell behind on are gone; keep publishing.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // TODO: Initialize SQLite database and AppState
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
