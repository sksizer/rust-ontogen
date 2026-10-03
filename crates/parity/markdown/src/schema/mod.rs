// The schema files are shared with parity-seaorm, so both stores are
// generated from the same source.
#[path = "../../../schema/fixed.rs"]
mod fixed;
#[path = "../../../schema/item.rs"]
mod item;
#[path = "../../../schema/section.rs"]
mod section;
#[path = "../../../schema/tag.rs"]
mod tag;

pub mod dto;

pub use fixed::Fixed;
pub use item::{Item, Kind};
pub use section::Section;
pub use tag::Tag;

pub use dto::fixed::{CreateFixedInput, UpdateFixedInput};
pub use dto::item::{CreateItemInput, UpdateItemInput};
pub use dto::section::{CreateSectionInput, UpdateSectionInput};
pub use dto::tag::{CreateTagInput, UpdateTagInput};

/// The variants the generated store constructs, plus `Md` for everything
/// else the runtime crate reports.
#[derive(Debug)]
pub enum AppError {
    FixedNotFound(String),
    FixedIdRequired(String),
    FixedAlreadyExists(String),
    ItemNotFound(String),
    ItemIdRequired(String),
    ItemAlreadyExists(String),
    SectionNotFound(String),
    SectionIdRequired(String),
    SectionAlreadyExists(String),
    SectionParentRequired(String),
    TagNotFound(String),
    TagIdRequired(String),
    TagAlreadyExists(String),
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
    Item,
    Section,
    Tag,
}
