mod note;

pub mod dto;

pub use note::Note;

pub use dto::note::{CreateNoteInput, UpdateNoteInput};

// ── Error type (the markdown consumer contract) ─────────────────────────────

#[derive(Debug)]
pub enum AppError {
    NoteNotFound(String),
    Md(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NoteNotFound(id) => write!(f, "Note not found: {id}"),
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
    Note,
}
