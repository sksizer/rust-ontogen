//! Live activity feeds, one per event op shape ontogen generates.

use tokio::sync::broadcast;

use crate::AppState;
use crate::schema::{Activity, AppError};

/// Every activity entry, to every subscriber. The parameterless shape.
pub fn activity_feed(state: &AppState) -> broadcast::Receiver<Activity> {
    state.activity.subscribe()
}

/// Activity for one entity kind. Takes a parameter, can fail, and resumes
/// from an activity `seq`.
pub async fn activity_for_kind(
    state: &AppState,
    kind: String,
    resume: Option<String>,
) -> Result<broadcast::Receiver<Activity>, AppError> {
    let feed = state.kind_feed(&kind).ok_or(AppError::ActivityKindNotFound(kind))?;
    // This example keeps no history, so there is nothing to replay after `resume`.
    let _ = resume;
    Ok(feed.subscribe())
}
