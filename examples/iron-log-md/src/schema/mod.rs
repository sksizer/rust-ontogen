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
// The markdown consumer contract: per-entity NotFound variants plus one Md
// variant carrying everything from the runtime crate. (Compare iron-log's
// AppError: DbError is gone; Md replaces it.)

#[derive(Debug)]
pub enum AppError {
    ExerciseNotFound(String),
    WorkoutNotFound(String),
    WorkoutSetNotFound(String),
    TagNotFound(String),
    Md(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::ExerciseNotFound(id) => write!(f, "Exercise not found: {id}"),
            AppError::WorkoutNotFound(id) => write!(f, "Workout not found: {id}"),
            AppError::WorkoutSetNotFound(id) => write!(f, "WorkoutSet not found: {id}"),
            AppError::TagNotFound(id) => write!(f, "Tag not found: {id}"),
            AppError::Md(msg) => write!(f, "markdown store error: {msg}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<markdown_store::Error> for AppError {
    fn from(e: markdown_store::Error) -> Self {
        AppError::Md(e.to_string())
    }
}

// ── Event types ─────────────────────────────────────────────────────────────

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
