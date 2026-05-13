use crate::schema::{AppError, WorkoutStats};
use crate::store::Store;

pub async fn workout(_store: &Store) -> Result<WorkoutStats, AppError> {
    Ok(WorkoutStats { total_count: 0, total_duration_minutes: 0 })
}
