//! A live feed of store changes, for the app to refresh on.

use tokio::sync::broadcast;

use crate::AppState;
use crate::schema::EntityChange;

/// Every create, update and delete, as it happens.
pub fn entity_changes(state: &AppState) -> broadcast::Receiver<EntityChange> {
    state.changes()
}
