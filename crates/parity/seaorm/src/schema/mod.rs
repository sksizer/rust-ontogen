// The schema files are shared with parity-markdown, so both stores are
// generated from the same source.
#[path = "../../../schema/doc.rs"]
mod doc;
#[path = "../../../schema/fixed.rs"]
mod fixed;
#[path = "../../../schema/item.rs"]
mod item;
#[path = "../../../schema/match.rs"]
mod r#match;
#[path = "../../../schema/order.rs"]
mod order;
#[path = "../../../schema/section.rs"]
mod section;
#[path = "../../../schema/tag.rs"]
mod tag;

pub mod dto;

pub use doc::Doc;
pub use fixed::Fixed;
pub use item::{Item, Kind};
pub use r#match::Match;
pub use order::Order;
pub use section::Section;
pub use tag::Tag;

pub use dto::doc::{CreateDocInput, UpdateDocInput};
pub use dto::fixed::{CreateFixedInput, UpdateFixedInput};
pub use dto::item::{CreateItemInput, UpdateItemInput};
pub use dto::r#match::{CreateMatchInput, UpdateMatchInput};
pub use dto::order::{CreateOrderInput, UpdateOrderInput};
pub use dto::section::{CreateSectionInput, UpdateSectionInput};
pub use dto::tag::{CreateTagInput, UpdateTagInput};

/// The variants the generated store constructs, plus `DbError` for
/// everything else SeaORM reports.
#[derive(Debug)]
pub enum AppError {
    DocNotFound(String),
    DocIdRequired(String),
    DocAlreadyExists(String),
    FixedNotFound(String),
    FixedIdRequired(String),
    FixedAlreadyExists(String),
    ItemNotFound(String),
    ItemIdRequired(String),
    ItemAlreadyExists(String),
    MatchNotFound(String),
    MatchIdRequired(String),
    MatchAlreadyExists(String),
    OrderNotFound(String),
    OrderIdRequired(String),
    OrderAlreadyExists(String),
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
    Doc,
    Fixed,
    Item,
    Match,
    Order,
    Section,
    Tag,
}
