//! Runtime for the JSON:API 1.1 HTTP server that ontogen generates.
//!
//! Generated Axum handlers call this crate to negotiate the media type, read
//! query parameters (a list's filter into its typed `*Query` struct and bare
//! parameters) and request documents (a resource's, or a custom op's
//! arguments), and write resource, link and error documents and the payloads
//! of event frames. The [wire contract] is normative: where this crate and the
//! contract disagree, the crate is wrong. Section signs (§) in this crate's
//! docs refer to that contract.
//!
//! Everything that needs the schema (attribute and relationship names, sort
//! fields, which relationships can be included) stays in generated code;
//! this crate holds only the schema-independent rules.
//!
//! The Axum extractors ([`extract`]) and responses ([`response`]) sit behind
//! the default `axum` feature. Without it the crate is the core meant for
//! the build-time generator: error codes, documents, media-type
//! checks, query parsing, links and request documents, over the `http`
//! types.
//!
//! [wire contract]: https://github.com/sksizer/rust-ontogen/blob/main/docs/jsonapi-wire-contract.md
#![cfg_attr(
    feature = "axum",
    doc = r#"
# A create handler

A generated handler lists its extractors in the order of §13.2, since Axum
runs them in argument order and answers with the first rejection. `Body`
must come last, yet its media type (step 3) precedes the path and the query
(steps 4 and 5), so the handler takes those two as `Result`s and answers
them once `Body` has run. It then reads and writes documents with the plain
functions:

```no_run
use axum::{Router, response::Response, routing::post};
use ontogen_jsonapi::{
    CanonicalQuery, Document, ErrorObject, ResourceObject,
    extract::{AcceptGuard, Body, NoParams, Path, Query},
    links::encode_path_segment,
    request::{Endpoint, parse_create},
    response,
};
use serde::Serialize;

#[derive(Serialize)]
struct TaskAttributes {
    title: String,
}

// POST /api/projects/{project_id}/tasks
async fn create_task(
    _: AcceptGuard,                             // step 2: 406 not_acceptable
    path: Result<Path<u32>, ErrorObject>,       // step 4
    query: Result<Query<NoParams>, ErrorObject>, // step 5
    body: Body,                                 // step 3: 415 unsupported_media_type
) -> Result<Response, ErrorObject> {
    let Path(project_id) = path?; // 400 invalid_path_parameter
    query?; // 400 invalid_query_parameter
    let body = body.into_bytes()?; // step 7: 413 content_too_large
    let collection = format!("/api/projects/{project_id}/tasks");
    let endpoint = Endpoint { type_name: "tasks", path: &collection };
    // The shared id-validity rule (§8.2) goes where this closure is.
    let data = parse_create(&body, endpoint, |_id| Ok::<(), String>(()))?;
    // Generated code checks `data.attributes` and `data.relationships()`
    // against the schema, then calls the store (steps 7 to 9).
    let title = data
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.get("title")?.as_str())
        .unwrap_or_default()
        .to_owned();
    let id = data.id.unwrap_or_else(|| "derived-by-the-store".to_owned());
    let self_link = format!("{collection}/{}", encode_path_segment(&id));
    let resource = ResourceObject::new("tasks", id, TaskAttributes { title }, self_link.clone());
    let document = Document::resource(resource, &CanonicalQuery::new());
    Ok(response::created(Some(&self_link), &document))
}

let app: Router = Router::new().route("/api/projects/{project_id}/tasks", post(create_task));
```
"#
)]
#![forbid(unsafe_code)]

pub mod document;
pub mod error;
#[cfg(feature = "axum")]
pub mod extract;
pub mod include;
pub mod links;
pub mod media;
pub mod path;
pub mod query;
pub mod request;
#[cfg(feature = "axum")]
pub mod response;

pub use document::{
    Absent, AnyResource, Document, JsonApiObject, Linkage, Links, PageMeta, PaginationLinks, Relationship,
    ResourceIdentifier, ResourceObject, ResultFrame, ResultMeta, UnlinkedResource,
};
pub use error::{ErrorCode, ErrorObject, ErrorSource};
pub use include::Included;
pub use links::CanonicalQuery;
pub use path::LookupKey;
pub use query::{QueryParams, QuerySpec, filter_fields};

/// The JSON:API media type, written without parameters on every response (§3.1).
pub const MEDIA_TYPE: &str = "application/vnd.api+json";
