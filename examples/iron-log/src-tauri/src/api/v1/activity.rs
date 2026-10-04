//! Live activity feeds, one per event op shape ontogen generates.

use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

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
    if !["exercise", "workout", "workout_set", "tag"].contains(&kind.as_str()) {
        return Err(AppError::ActivityKindNotFound(kind));
    }
    // This example keeps no history, so there is nothing to replay after `resume`.
    let _ = resume;

    // A broadcast receiver cannot filter, so each subscriber gets its own
    // channel, fed by a task that forwards only this kind. A slow subscriber
    // lags on its own channel, which the stream reports as `lag`.
    let mut feed = state.activity.subscribe();
    let (tx, rx) = broadcast::channel(256);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // The subscriber hung up: stop instead of waiting for a
                // matching entry to find out.
                () = tx.closed() => break,
                entry = feed.recv() => match entry {
                    Ok(activity) if activity.kind == kind => {
                        let _ = tx.send(activity);
                    }
                    // Other kinds, and entries this task fell behind on
                    // (it only filters, so it rarely does).
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                },
            }
        }
    });
    Ok(rx)
}
