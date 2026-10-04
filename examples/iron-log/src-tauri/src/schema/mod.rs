mod activity;
mod exercise;
mod stats;
mod tag;
mod workout;
mod workout_set;

pub mod dto;

pub use activity::Activity;
pub use exercise::Exercise;
pub use stats::WorkoutStats;
pub use tag::Tag;
pub use workout::Workout;
pub use workout_set::WorkoutSet;

// Re-export DTOs at the schema level (generated code imports from crate::schema::)
pub use dto::exercise::{CreateExerciseInput, UpdateExerciseInput};
pub use dto::tag::{CreateTagInput, UpdateTagInput};
pub use dto::workout::{CreateWorkoutInput, UpdateWorkoutInput};
pub use dto::workout_set::{CreateWorkoutSetInput, UpdateWorkoutSetInput};

// ── Error type ──────────────────────────────────────────────────────────────
// Generated store code imports AppError with entity-specific NotFound variants.

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Exercise not found: {0}")]
    ExerciseNotFound(String),
    #[error("Exercise id required: {0}")]
    ExerciseIdRequired(String),
    #[error("Exercise already exists: {0}")]
    ExerciseAlreadyExists(String),
    #[error("Workout not found: {0}")]
    WorkoutNotFound(String),
    #[error("Workout id required: {0}")]
    WorkoutIdRequired(String),
    #[error("Workout already exists: {0}")]
    WorkoutAlreadyExists(String),
    #[error("WorkoutSet not found: {0}")]
    WorkoutSetNotFound(String),
    #[error("WorkoutSet id required: {0}")]
    WorkoutSetIdRequired(String),
    #[error("WorkoutSet already exists: {0}")]
    WorkoutSetAlreadyExists(String),
    #[error("Tag not found: {0}")]
    TagNotFound(String),
    #[error("Tag id required: {0}")]
    TagIdRequired(String),
    #[error("Tag already exists: {0}")]
    TagAlreadyExists(String),
    /// `activity_for_kind` was asked for a kind that is not an entity.
    #[error("Activity kind not found: {0}")]
    ActivityKindNotFound(String),
    #[error("Database error: {0}")]
    DbError(String),
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

// ── Event types ─────────────────────────────────────────────────────────────
// Generated store code emits change events via self.emit_change().

#[derive(Debug, Clone)]
pub enum ChangeOp {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone)]
pub enum EntityKind {
    Exercise,
    Workout,
    WorkoutSet,
    Tag,
}
