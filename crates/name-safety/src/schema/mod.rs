mod doc;
mod document;
mod endpoint;
mod event;
mod links;
mod map;
mod r#match;
mod method;
mod relationship;
mod request;
mod response;
mod state;
mod value;

pub mod dto;

pub use doc::Doc;
pub use document::Document;
pub use endpoint::Endpoint;
pub use event::Event;
pub use links::Links;
pub use map::Map;
pub use r#match::Match;
pub use method::Method;
pub use relationship::Relationship;
pub use request::Request;
pub use response::Response;
pub use state::State;
pub use value::Value;

pub use dto::doc::{CreateDocInput, UpdateDocInput};
pub use dto::document::{CreateDocumentInput, UpdateDocumentInput};
pub use dto::endpoint::{CreateEndpointInput, UpdateEndpointInput};
pub use dto::event::{CreateEventInput, UpdateEventInput};
pub use dto::links::{CreateLinksInput, UpdateLinksInput};
pub use dto::map::{CreateMapInput, UpdateMapInput};
pub use dto::r#match::{CreateMatchInput, UpdateMatchInput};
pub use dto::method::{CreateMethodInput, UpdateMethodInput};
pub use dto::relationship::{CreateRelationshipInput, UpdateRelationshipInput};
pub use dto::request::{CreateRequestInput, UpdateRequestInput};
pub use dto::response::{CreateResponseInput, UpdateResponseInput};
pub use dto::state::{CreateStateInput, UpdateStateInput};
pub use dto::value::{CreateValueInput, UpdateValueInput};

// ── Error type (the markdown consumer contract) ─────────────────────────────

#[derive(Debug)]
pub enum AppError {
    DocNotFound(String),
    DocIdRequired(String),
    DocAlreadyExists(String),
    DocumentNotFound(String),
    DocumentIdRequired(String),
    DocumentAlreadyExists(String),
    EndpointNotFound(String),
    EndpointIdRequired(String),
    EndpointAlreadyExists(String),
    EventNotFound(String),
    EventIdRequired(String),
    EventAlreadyExists(String),
    LinksNotFound(String),
    LinksIdRequired(String),
    LinksAlreadyExists(String),
    MapNotFound(String),
    MatchNotFound(String),
    MatchIdRequired(String),
    MatchAlreadyExists(String),
    MapIdRequired(String),
    MapAlreadyExists(String),
    MethodNotFound(String),
    MethodIdRequired(String),
    MethodAlreadyExists(String),
    RelationshipNotFound(String),
    RelationshipIdRequired(String),
    RelationshipAlreadyExists(String),
    RequestNotFound(String),
    RequestIdRequired(String),
    RequestAlreadyExists(String),
    ResponseNotFound(String),
    ResponseIdRequired(String),
    ResponseAlreadyExists(String),
    StateNotFound(String),
    StateIdRequired(String),
    StateAlreadyExists(String),
    ValueNotFound(String),
    ValueIdRequired(String),
    ValueAlreadyExists(String),
    Md(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
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
    Doc,
    Document,
    Endpoint,
    Event,
    Links,
    Map,
    Match,
    Method,
    Relationship,
    Request,
    Response,
    State,
    Value,
}
