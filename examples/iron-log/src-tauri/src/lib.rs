pub mod api;
pub mod persistence;
pub mod schema;
pub mod store;

use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::broadcast;

use crate::schema::{Activity, AppError, EntityKind};
use crate::store::Store;

/// Each entity kind and its name on the activity feeds. The publisher and
/// `activity_for_kind` both read this one list, and its order is the order
/// of `AppState::activity_by_kind`.
pub(crate) const ACTIVITY_KINDS: [(EntityKind, &str); 4] = [
    (EntityKind::Exercise, "exercise"),
    (EntityKind::Workout, "workout"),
    (EntityKind::WorkoutSet, "workout_set"),
    (EntityKind::Tag, "tag"),
];

/// Application state shared across all handlers.
pub struct AppState {
    store: Store,
    /// Every change, for `activity_feed`.
    activity: broadcast::Sender<Activity>,
    /// One channel per kind, in `ACTIVITY_KINDS` order, for
    /// `activity_for_kind`. A subscriber holds a receiver of its kind's
    /// channel itself, so when it falls behind the stream reports `lag`
    /// rather than a filter in between dropping entries unseen.
    activity_by_kind: [broadcast::Sender<Activity>; ACTIVITY_KINDS.len()],
}

impl AppState {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self {
            store: Store::new(db),
            activity: broadcast::channel(256).0,
            activity_by_kind: std::array::from_fn(|_| broadcast::channel(256).0),
        }
    }

    /// Access the store. Generated IPC handlers call this.
    pub async fn store(&self) -> Result<&Store, AppError> {
        Ok(&self.store)
    }

    /// The activity channel for one kind name; `None` when no entity has it.
    pub(crate) fn kind_feed(&self, name: &str) -> Option<&broadcast::Sender<Activity>> {
        ACTIVITY_KINDS.iter().position(|(_, n)| *n == name).map(|i| &self.activity_by_kind[i])
    }

    /// Publish every store change on the activity feeds, numbered from 1.
    ///
    /// Subscribes to the store before it returns, so a change made as soon
    /// as the server is up is not missed. The host spawns the returned
    /// future once; it ends when the state is dropped.
    pub fn publish_activity(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut changes = self.store.subscribe();
        let all = self.activity.clone();
        let by_kind = self.activity_by_kind.clone();
        async move {
            let mut seq = 0;
            loop {
                match changes.recv().await {
                    Ok(change) => {
                        seq += 1;
                        let i = ACTIVITY_KINDS
                            .iter()
                            .position(|(kind, _)| *kind == change.kind)
                            .expect("ACTIVITY_KINDS lists every EntityKind");
                        let activity = Activity { seq, kind: ACTIVITY_KINDS[i].1.into(), id: change.id };
                        // No subscriber is not an error: the feeds are live-only.
                        let _ = by_kind[i].send(activity.clone());
                        let _ = all.send(activity);
                    }
                    // The changes this task fell behind on are gone; skipping
                    // their numbers leaves subscribers a visible gap in `seq`.
                    Err(broadcast::error::RecvError::Lagged(missed)) => seq += missed,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
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
