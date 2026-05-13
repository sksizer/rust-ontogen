use serde::Serialize;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WorkoutStats {
    pub total_count: i64,
    pub total_duration_minutes: i64,
}
