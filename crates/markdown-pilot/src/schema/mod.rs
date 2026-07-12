mod note;
mod tag;
mod task;

pub mod dto;

pub use note::Note;
pub use tag::Tag;
pub use task::Task;

// Re-export DTOs at the schema level (generated code imports from crate::schema::)
pub use dto::note::{CreateNoteInput, UpdateNoteInput};
pub use dto::tag::{CreateTagInput, UpdateTagInput};
pub use dto::task::{CreateTaskInput, UpdateTaskInput};

// ── Error type ──────────────────────────────────────────────────────────────
// The markdown consumer contract: per-entity NotFound variants plus a single
// Md variant carrying everything from the runtime crate.

#[derive(Debug)]
pub enum AppError {
    NoteNotFound(String),
    TaskNotFound(String),
    TagNotFound(String),
    Md(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NoteNotFound(id) => write!(f, "Note not found: {id}"),
            AppError::TaskNotFound(id) => write!(f, "Task not found: {id}"),
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
// Generated store code emits change events via self.emit_change().

#[derive(Debug, Clone)]
pub enum ChangeOp {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone)]
pub enum EntityKind {
    Note,
    Task,
    Tag,
}
