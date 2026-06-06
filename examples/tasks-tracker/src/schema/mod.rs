mod epic;
mod tag;
mod task;

pub mod dto;

pub use epic::Epic;
pub use tag::Tag;
pub use task::Task;

pub use dto::epic::{CreateEpicInput, UpdateEpicInput};
pub use dto::tag::{CreateTagInput, UpdateTagInput};
pub use dto::task::{CreateTaskInput, UpdateTaskInput};

// ── Error type (the markdown consumer contract) ─────────────────────────────

#[derive(Debug)]
pub enum AppError {
    TaskNotFound(String),
    EpicNotFound(String),
    TagNotFound(String),
    Md(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::TaskNotFound(id) => write!(f, "Task not found: {id}"),
            AppError::EpicNotFound(id) => write!(f, "Epic not found: {id}"),
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
    Task,
    Epic,
    Tag,
}
