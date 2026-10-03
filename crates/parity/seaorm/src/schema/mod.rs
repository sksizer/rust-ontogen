// The schema files are shared with parity-markdown, so both stores are
// generated from the same source.
#[path = "../../../schema/item.rs"]
mod item;
#[path = "../../../schema/section.rs"]
mod section;
#[path = "../../../schema/tag.rs"]
mod tag;

pub mod dto;

pub use item::{Item, Kind};
pub use section::Section;
pub use tag::Tag;

pub use dto::item::{CreateItemInput, UpdateItemInput};
pub use dto::section::{CreateSectionInput, UpdateSectionInput};
pub use dto::tag::{CreateTagInput, UpdateTagInput};

/// The variants the generated store constructs, plus `DbError` for
/// everything else SeaORM reports.
#[derive(Debug)]
pub enum AppError {
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
    Item,
    Section,
    Tag,
}
