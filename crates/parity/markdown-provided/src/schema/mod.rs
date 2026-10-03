// Shared with parity-seaorm-provided, so both stores are generated from the
// same source.
#[path = "../../../schema-provided/fixed.rs"]
mod fixed;

pub mod dto;

pub use dto::fixed::{CreateFixedInput, UpdateFixedInput};
pub use fixed::Fixed;

#[derive(Debug)]
pub enum AppError {
    FixedNotFound(String),
    FixedIdRequired(String),
    FixedAlreadyExists(String),
    Md(String),
}

impl From<markdown_store::Error> for AppError {
    fn from(e: markdown_store::Error) -> Self {
        AppError::Md(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub enum ChangeOp {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone)]
pub enum EntityKind {
    Fixed,
}
