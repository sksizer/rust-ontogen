//! Runtime for the JSON:API 1.1 HTTP server that ontogen generates.
//!
//! Generated Axum handlers call this crate to negotiate the media type, read
//! query parameters and request documents, and write resource, link and error
//! documents. The [wire contract] is normative: where this crate and the
//! contract disagree, the crate is wrong. Section signs (§) in this crate's
//! docs refer to that contract.
//!
//! Everything that needs the schema (attribute and relationship names, sort
//! fields, include paths) stays in generated code; this crate holds only the
//! schema-independent rules.
//!
//! [wire contract]: https://github.com/sksizer/rust-ontogen/blob/main/docs/jsonapi-wire-contract.md

#![forbid(unsafe_code)]

pub mod document;
pub mod error;
pub mod extract;
pub mod links;
pub mod media;
pub mod query;
pub mod request;
pub mod response;

pub use document::{
    Absent, AnyResource, Document, JsonApiObject, Linkage, Links, PageMeta, PaginationLinks, Relationship,
    ResourceIdentifier, ResourceObject, ResultMeta,
};
pub use error::{ErrorCode, ErrorObject, ErrorSource};
pub use links::CanonicalQuery;
pub use query::{QueryParams, QuerySpec};

/// The JSON:API media type, written without parameters on every response (§3.1).
pub const MEDIA_TYPE: &str = "application/vnd.api+json";
