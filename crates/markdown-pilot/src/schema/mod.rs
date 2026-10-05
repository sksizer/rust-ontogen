mod bookmark;
mod note;
mod section;
mod tag;
mod task;
mod task_summary;

pub mod dto;

pub use bookmark::{Bookmark, BookmarkQuery, CreateBookmarkInput, UpdateBookmarkInput};
pub use note::Note;
pub use section::Section;
pub use tag::Tag;
pub use task::Task;
pub use task_summary::TaskSummary;
// The generated handlers import every type an api fn names from here, the
// section list's filter included.
pub use crate::api::section::ListSectionsQuery;

// Re-export DTOs at the schema level (generated code imports from crate::schema::)
pub use dto::note::{CreateNoteInput, UpdateNoteInput};
pub use dto::section::{CreateSectionInput, UpdateSectionInput};
pub use dto::tag::{CreateTagInput, UpdateTagInput};
pub use dto::task::{CreateTaskInput, UpdateTaskInput};

// ── Error type ──────────────────────────────────────────────────────────────
// The markdown consumer contract: the typed variants the generated store
// constructs (per entity NotFound, IdRequired and AlreadyExists, plus
// ParentCycle for the child of a self-referential has_many and
// ParentRequired for a child whose has_many foreign key is required), the
// hand-written bookmark API's BookmarkNotFound, and a single Md variant
// carrying everything else from the runtime crate.

#[derive(Debug)]
pub enum AppError {
    NoteNotFound(String),
    NoteIdRequired(String),
    NoteAlreadyExists(String),
    SectionNotFound(String),
    SectionIdRequired(String),
    SectionAlreadyExists(String),
    SectionParentRequired(String),
    SectionParentCycle(String),
    TaskNotFound(String),
    TaskIdRequired(String),
    TaskAlreadyExists(String),
    TaskParentCycle(String),
    TagNotFound(String),
    TagIdRequired(String),
    TagAlreadyExists(String),
    BookmarkNotFound(String),
    Md(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NoteNotFound(id) => write!(f, "Note not found: {id}"),
            AppError::SectionNotFound(id) => write!(f, "Section not found: {id}"),
            AppError::TaskNotFound(id) => write!(f, "Task not found: {id}"),
            AppError::TagNotFound(id) => write!(f, "Tag not found: {id}"),
            AppError::BookmarkNotFound(id) => write!(f, "Bookmark not found: {id}"),
            AppError::NoteIdRequired(reason)
            | AppError::SectionIdRequired(reason)
            | AppError::TaskIdRequired(reason)
            | AppError::TagIdRequired(reason) => write!(f, "id required: {reason}"),
            AppError::NoteAlreadyExists(id)
            | AppError::SectionAlreadyExists(id)
            | AppError::TaskAlreadyExists(id)
            | AppError::TagAlreadyExists(id) => write!(f, "already exists: {id}"),
            AppError::SectionParentRequired(id) => write!(f, "section {id} needs a parent and cannot be dropped"),
            AppError::SectionParentCycle(id) | AppError::TaskParentCycle(id) => {
                write!(f, "{id} cannot be its own child")
            }
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
    Section,
    Task,
    Tag,
}
