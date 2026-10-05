mod axum;
mod doc;
mod document;
mod endpoint;
mod event;
mod links;
mod map;
mod markdown_store;
mod r#match;
mod method;
mod path;
mod relationship;
mod request;
mod response;
mod serde;
mod state;
mod value;

pub mod dto;

pub use axum::Axum;
pub use doc::Doc;
pub use document::Document;
pub use endpoint::Endpoint;
pub use event::Event;
pub use links::Links;
pub use map::Map;
pub use markdown_store::MarkdownStore;
pub use r#match::Match;
pub use method::Method;
pub use path::Path;
pub use relationship::Relationship;
pub use request::Request;
pub use response::Response;
pub use serde::Serde;
pub use state::State;
pub use value::Value;

/// `api::lookup::by_path` takes a `PathBuf`: the generated servers name
/// every bare type in an API signature through this module.
pub use std::path::PathBuf;

pub use dto::axum::{CreateAxumInput, UpdateAxumInput};
pub use dto::doc::{CreateDocInput, UpdateDocInput};
pub use dto::document::{CreateDocumentInput, UpdateDocumentInput};
pub use dto::endpoint::{CreateEndpointInput, UpdateEndpointInput};
pub use dto::event::{CreateEventInput, UpdateEventInput};
pub use dto::links::{CreateLinksInput, UpdateLinksInput};
pub use dto::map::{CreateMapInput, UpdateMapInput};
pub use dto::markdown_store::{CreateMarkdownStoreInput, UpdateMarkdownStoreInput};
pub use dto::r#match::{CreateMatchInput, UpdateMatchInput};
pub use dto::method::{CreateMethodInput, UpdateMethodInput};
pub use dto::path::{CreatePathInput, UpdatePathInput};
pub use dto::relationship::{CreateRelationshipInput, UpdateRelationshipInput};
pub use dto::request::{CreateRequestInput, UpdateRequestInput};
pub use dto::response::{CreateResponseInput, UpdateResponseInput};
pub use dto::serde::{CreateSerdeInput, UpdateSerdeInput};
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
    PathNotFound(String),
    PathIdRequired(String),
    PathAlreadyExists(String),
    SerdeNotFound(String),
    SerdeIdRequired(String),
    SerdeAlreadyExists(String),
    AxumNotFound(String),
    AxumIdRequired(String),
    AxumAlreadyExists(String),
    MarkdownStoreNotFound(String),
    MarkdownStoreIdRequired(String),
    MarkdownStoreAlreadyExists(String),
    Md(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AppError {}

impl From<::markdown_store::Error> for AppError {
    fn from(e: ::markdown_store::Error) -> Self {
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
    Path,
    Serde,
    Axum,
    MarkdownStore,
}
