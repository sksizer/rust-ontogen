mod exercise;
mod tag;
mod workout;
mod workout_set;

pub mod dto;

pub use exercise::Exercise;
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
    #[error("Workout not found: {0}")]
    WorkoutNotFound(String),
    #[error("WorkoutSet not found: {0}")]
    WorkoutSetNotFound(String),
    #[error("Tag not found: {0}")]
    TagNotFound(String),
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
