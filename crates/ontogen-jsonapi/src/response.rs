//! Writing responses (§3.1, §3.3).
//!
//! Every response here carries `Vary: Accept`, because any of them could
//! have been a `406` (§3.3). Bodies are `application/vnd.api+json` with no
//! parameters; a `204` has neither body nor `Content-Type`.

use axum::{body::Body, response::Response};
use http::{HeaderValue, StatusCode, header};
use serde::Serialize;

use crate::{MEDIA_TYPE, error::ErrorObject};

/// `200 OK` with `document`.
pub fn ok<T: Serialize>(document: &T) -> Response {
    document_response(StatusCode::OK, document, None)
}

/// `201 Created` with `document` and a `Location` equal to the resource's
/// `links.self` (§8.2).
pub fn created<T: Serialize>(location: &str, document: &T) -> Response {
    document_response(StatusCode::CREATED, document, Some(location))
}

/// `204 No Content`: no body and no `Content-Type` (§3.1).
pub fn no_content() -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    response.headers_mut().insert(header::VARY, HeaderValue::from_static("Accept"));
    response
}

/// `status` with `document`, and `Location` when given.
///
/// A document that fails to serialize (only a consumer type's own
/// `Serialize` can fail) becomes `500 internal_error`, as does a `Location`
/// that is not a valid header value; links built with [`crate::links`] are
/// always ASCII.
pub fn document_response<T: Serialize>(status: StatusCode, document: &T, location: Option<&str>) -> Response {
    let body = match serde_json::to_vec(document) {
        Ok(body) => body,
        Err(err) => return error_response(&ErrorObject::internal(format!("failed to serialize the response: {err}"))),
    };
    let location = match location.map(HeaderValue::from_str).transpose() {
        Ok(location) => location,
        Err(_) => return error_response(&ErrorObject::internal("the Location header is not a valid header value")),
    };
    let mut response = with_body(status, body);
    if let Some(location) = location {
        response.headers_mut().insert(header::LOCATION, location);
    }
    response
}

pub(crate) fn error_response(error: &ErrorObject) -> Response {
    // An error document holds only strings, so serializing it cannot fail.
    let body = serde_json::to_vec(&error.document()).unwrap_or_default();
    with_body(error.status(), body)
}

fn with_body(status: StatusCode, body: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(MEDIA_TYPE));
    headers.insert(header::VARY, HeaderValue::from_static("Accept"));
    response
}
