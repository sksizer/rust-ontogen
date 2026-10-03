// Shared with parity-markdown-provided, so both stores are generated from the
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
    DbError(String),
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
