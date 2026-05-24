use crate::schema::{AppError, WorkoutStats};
use crate::store::Store;

/// Read-only stats summary. The `get_` prefix is required to opt this
/// zero-user-param function into `OpKind::CustomGet` after ontogen's
/// classifier default flipped to `CustomPost` for non-read-prefix names
/// (RFC 7231 §4.2.1).
pub async fn get_workout(_store: &Store) -> Result<WorkoutStats, AppError> {
    Ok(WorkoutStats { total_count: 0, total_duration_minutes: 0 })
}
