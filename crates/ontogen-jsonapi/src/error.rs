//! The error document (§13).

use std::{borrow::Cow, fmt};

#[cfg(feature = "axum")]
use axum::response::{IntoResponse, Response};
use http::StatusCode;
#[cfg(feature = "axum")]
use http::{HeaderValue, Method, header};
use serde::{Serialize, Serializer, ser::SerializeMap};

use crate::{document::JsonApiObject, path::LookupKey};

/// The errors the generated server raises itself (§13.3). An `AppError`
/// variant's code is its snake_case name instead, built with
/// [`ErrorObject::app`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    /// `400 invalid_query_parameter`
    InvalidQueryParameter,
    /// `400 invalid_sort_field`
    InvalidSortField,
    /// `400 invalid_include_path`
    InvalidIncludePath,
    /// `400 invalid_path_parameter`
    InvalidPathParameter,
    /// `400 invalid_document`
    InvalidDocument,
    /// `400 unknown_attribute`
    UnknownAttribute,
    /// `400 missing_attribute`
    MissingAttribute,
    /// `400 invalid_attribute`
    InvalidAttribute,
    /// `400 unknown_relationship`
    UnknownRelationship,
    /// `400 missing_relationship`
    MissingRelationship,
    /// `403 relationship_required`
    RelationshipRequired,
    /// `403 relationship_update_unsupported`
    RelationshipUpdateUnsupported,
    /// `403 relationship_batch_unsupported`
    RelationshipBatchUnsupported,
    /// `403 relationship_cycle`
    RelationshipCycle,
    /// `404 no_such_related_resource`
    NoSuchRelatedResource,
    /// `404 no_such_relationship`
    NoSuchRelationship,
    /// `405 method_not_allowed`
    MethodNotAllowed,
    /// `406 not_acceptable`
    NotAcceptable,
    /// `409 type_mismatch`
    TypeMismatch,
    /// `409 id_mismatch`
    IdMismatch,
    /// `413 content_too_large`
    ContentTooLarge,
    /// `415 unsupported_media_type`
    UnsupportedMediaType,
    /// `500 internal_error`
    InternalError,
}

impl ErrorCode {
    /// Every code, in the order of §13.3's table. The generator checks
    /// `AppError` variant names against these so a code means one thing
    /// (§13.4). A slice, so adding a code does not change the type.
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::InvalidQueryParameter,
        ErrorCode::InvalidSortField,
        ErrorCode::InvalidIncludePath,
        ErrorCode::InvalidPathParameter,
        ErrorCode::InvalidDocument,
        ErrorCode::UnknownAttribute,
        ErrorCode::MissingAttribute,
        ErrorCode::InvalidAttribute,
        ErrorCode::UnknownRelationship,
        ErrorCode::MissingRelationship,
        ErrorCode::RelationshipRequired,
        ErrorCode::RelationshipUpdateUnsupported,
        ErrorCode::RelationshipBatchUnsupported,
        ErrorCode::RelationshipCycle,
        ErrorCode::NoSuchRelatedResource,
        ErrorCode::NoSuchRelationship,
        ErrorCode::MethodNotAllowed,
        ErrorCode::NotAcceptable,
        ErrorCode::TypeMismatch,
        ErrorCode::IdMismatch,
        ErrorCode::ContentTooLarge,
        ErrorCode::UnsupportedMediaType,
        ErrorCode::InternalError,
    ];

    /// The wire `code`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidQueryParameter => "invalid_query_parameter",
            ErrorCode::InvalidSortField => "invalid_sort_field",
            ErrorCode::InvalidIncludePath => "invalid_include_path",
            ErrorCode::InvalidPathParameter => "invalid_path_parameter",
            ErrorCode::InvalidDocument => "invalid_document",
            ErrorCode::UnknownAttribute => "unknown_attribute",
            ErrorCode::MissingAttribute => "missing_attribute",
            ErrorCode::InvalidAttribute => "invalid_attribute",
            ErrorCode::UnknownRelationship => "unknown_relationship",
            ErrorCode::MissingRelationship => "missing_relationship",
            ErrorCode::RelationshipRequired => "relationship_required",
            ErrorCode::RelationshipUpdateUnsupported => "relationship_update_unsupported",
            ErrorCode::RelationshipBatchUnsupported => "relationship_batch_unsupported",
            ErrorCode::RelationshipCycle => "relationship_cycle",
            ErrorCode::NoSuchRelatedResource => "no_such_related_resource",
            ErrorCode::NoSuchRelationship => "no_such_relationship",
            ErrorCode::MethodNotAllowed => "method_not_allowed",
            ErrorCode::NotAcceptable => "not_acceptable",
            ErrorCode::TypeMismatch => "type_mismatch",
            ErrorCode::IdMismatch => "id_mismatch",
            ErrorCode::ContentTooLarge => "content_too_large",
            ErrorCode::UnsupportedMediaType => "unsupported_media_type",
            ErrorCode::InternalError => "internal_error",
        }
    }

    /// The status §13.3 assigns to the code.
    pub const fn status(self) -> StatusCode {
        match self {
            ErrorCode::InvalidQueryParameter
            | ErrorCode::InvalidSortField
            | ErrorCode::InvalidIncludePath
            | ErrorCode::InvalidPathParameter
            | ErrorCode::InvalidDocument
            | ErrorCode::UnknownAttribute
            | ErrorCode::MissingAttribute
            | ErrorCode::InvalidAttribute
            | ErrorCode::UnknownRelationship
            | ErrorCode::MissingRelationship => StatusCode::BAD_REQUEST,
            ErrorCode::RelationshipRequired
            | ErrorCode::RelationshipUpdateUnsupported
            | ErrorCode::RelationshipBatchUnsupported
            | ErrorCode::RelationshipCycle => StatusCode::FORBIDDEN,
            ErrorCode::NoSuchRelatedResource | ErrorCode::NoSuchRelationship => StatusCode::NOT_FOUND,
            ErrorCode::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            ErrorCode::NotAcceptable => StatusCode::NOT_ACCEPTABLE,
            ErrorCode::TypeMismatch | ErrorCode::IdMismatch => StatusCode::CONFLICT,
            ErrorCode::ContentTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ErrorCode::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where in the request an error lies (§13.1). Exactly one member is
/// written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorSource {
    /// An RFC 6901 pointer into the request body, naming a value that is
    /// present in it.
    Pointer(String),
    /// A query parameter's decoded name, with raw brackets (`page[limit]`).
    Parameter(String),
    /// A request header name.
    Header(&'static str),
}

impl Serialize for ErrorSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            ErrorSource::Pointer(p) => map.serialize_entry("pointer", p)?,
            ErrorSource::Parameter(p) => map.serialize_entry("parameter", p)?,
            ErrorSource::Header(h) => map.serialize_entry("header", h)?,
        }
        map.end()
    }
}

/// One JSON:API error object (§13.1). A response carries exactly one, so
/// this type is also the response: with the `axum` feature its
/// `IntoResponse` writes the whole error document with the object's status.
///
/// Members serialize as `status`, `code`, `title`, `detail`, `source`.
/// `title` is derived from the status. There is deliberately no way to set
/// `id`, `links` or `meta`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorObject {
    status: StatusCode,
    code: Cow<'static, str>,
    detail: String,
    source: Option<ErrorSource>,
}

impl ErrorObject {
    /// An ontogen-authored error, with the status §13.3 gives its code.
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        ErrorObject { status: code.status(), code: Cow::Borrowed(code.as_str()), detail: detail.into(), source: None }
    }

    /// An error mapped from an `AppError` variant (§13.4): `code` is the
    /// variant name in snake_case and `detail` its `Display` text.
    pub fn app(status: StatusCode, code: impl Into<Cow<'static, str>>, detail: impl Into<String>) -> Self {
        ErrorObject { status, code: code.into(), detail: detail.into(), source: None }
    }

    /// `500 internal_error`.
    pub fn internal(detail: impl Into<String>) -> Self {
        ErrorObject::new(ErrorCode::InternalError, detail)
    }

    /// Sets `source.pointer`.
    pub fn with_pointer(self, pointer: impl Into<String>) -> Self {
        self.with_source(ErrorSource::Pointer(pointer.into()))
    }

    /// Sets `source.parameter`.
    pub fn with_parameter(self, parameter: impl Into<String>) -> Self {
        self.with_source(ErrorSource::Parameter(parameter.into()))
    }

    /// Sets `source.header`.
    pub fn with_header(self, header: &'static str) -> Self {
        self.with_source(ErrorSource::Header(header))
    }

    /// Sets `source`, replacing any earlier one.
    pub fn with_source(mut self, source: ErrorSource) -> Self {
        self.source = Some(source);
        self
    }

    /// The HTTP status, which is also the response status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The `code` member.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The `title` member: the status's RFC 9110 reason phrase.
    pub fn title(&self) -> &'static str {
        reason_phrase(self.status)
    }

    /// The `detail` member.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// The `source` member.
    pub fn source(&self) -> Option<&ErrorSource> {
        self.source.as_ref()
    }

    /// The error document carrying this object.
    pub fn document(&self) -> ErrorDocument<'_> {
        ErrorDocument { jsonapi: JsonApiObject, errors: [self] }
    }
}

impl Serialize for ErrorObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("status", self.status.as_str())?;
        map.serialize_entry("code", &self.code)?;
        map.serialize_entry("title", self.title())?;
        map.serialize_entry("detail", &self.detail)?;
        if let Some(source) = &self.source {
            map.serialize_entry("source", source)?;
        }
        map.end()
    }
}

impl fmt::Display for ErrorObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.status.as_str(), self.code, self.detail)
    }
}

impl std::error::Error for ErrorObject {}

#[cfg(feature = "axum")]
impl IntoResponse for ErrorObject {
    fn into_response(self) -> Response {
        crate::response::error_response(&self)
    }
}

/// An error document, `{"jsonapi": …, "errors": [ … ]}` (§13.1).
#[derive(Debug, Clone, Serialize)]
pub struct ErrorDocument<'a> {
    jsonapi: JsonApiObject,
    errors: [&'a ErrorObject; 1],
}

/// The RFC 9110 reason phrase for `status`, the `title` of every error with
/// that status (§13.1).
///
/// RFC 9110 renamed some phrases that `http`'s canonical reasons still give
/// under their RFC 7231 names (`413 Content Too Large`, `422 Unprocessable
/// Content`), so the RFC 9110 table is spelled out here.
pub fn reason_phrase(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        426 => "Upgrade Required",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => status.canonical_reason().unwrap_or("Error"),
    }
}

/// `404 no_such_relationship`: `rel` names no relationship of the resource
/// type `type_name` (§9).
///
/// A relationship route captures `{rel}` rather than listing each name, so
/// that an unknown one is answered here and does not fall through to the
/// consumer's router. A `rel` that does not decode names no relationship and
/// is written as sent. The error has no `source`: the path is not a body
/// member or a query parameter.
pub fn no_such_relationship(type_name: &str, rel: &LookupKey) -> ErrorObject {
    ErrorObject::new(ErrorCode::NoSuchRelationship, format!("`{type_name}` has no relationship `{rel}`"))
}

/// `403 relationship_update_unsupported`: the relationship `rel` of
/// `type_name` does not accept `method` (§9).
///
/// The spec requires `403` for a relationship update the server does not
/// support. The refusal is decided from the route alone (§13.2 step 6),
/// before the body is read, so the error has no `source`.
pub fn relationship_update_unsupported(type_name: &str, rel: &str, method: &str) -> ErrorObject {
    ErrorObject::new(
        ErrorCode::RelationshipUpdateUnsupported,
        format!("the relationship `{rel}` of `{type_name}` cannot be changed with {method}"),
    )
}

/// The `405` response for a route serving `allowed` (§13.5).
///
/// `Allow` lists the methods in a fixed order (`GET, HEAD, PATCH, POST,
/// DELETE`, then any others), with `HEAD` wherever `GET` is because Axum
/// serves it. Install it per route, where the methods are known, with
/// `MethodRouter::fallback`; Axum leaves an `Allow` header the handler set
/// untouched.
#[cfg(feature = "axum")]
pub fn method_not_allowed(method: &Method, allowed: &[Method]) -> Response {
    let allow = allow_header(allowed);
    let mut response =
        ErrorObject::new(ErrorCode::MethodNotAllowed, format!("{method} is not allowed here; allowed: {allow}"))
            .into_response();
    if let Ok(value) = HeaderValue::from_str(&allow) {
        response.headers_mut().insert(header::ALLOW, value);
    }
    response
}

/// A handler for `Router::method_not_allowed_fallback` (§13.5).
///
/// It does not know the route, so it leaves `Allow` to Axum, which fills it
/// from the route's method router (listing `HEAD` with `GET`). Axum writes
/// that list without spaces and in registration order; [`method_not_allowed`]
/// gives the canonical form when the route's methods are known.
#[cfg(feature = "axum")]
pub async fn method_not_allowed_fallback(method: Method) -> Response {
    ErrorObject::new(ErrorCode::MethodNotAllowed, format!("{method} is not allowed here")).into_response()
}

#[cfg(feature = "axum")]
fn allow_header(allowed: &[Method]) -> String {
    const ORDER: [Method; 9] = [
        Method::GET,
        Method::HEAD,
        Method::PATCH,
        Method::POST,
        Method::DELETE,
        Method::PUT,
        Method::OPTIONS,
        Method::TRACE,
        Method::CONNECT,
    ];
    let serves = |m: &Method| allowed.contains(m) || (*m == Method::HEAD && allowed.contains(&Method::GET));
    let mut methods: Vec<&str> = ORDER.iter().filter(|m| serves(m)).map(Method::as_str).collect();
    for extension in allowed.iter().filter(|m| !ORDER.contains(m)) {
        if !methods.contains(&extension.as_str()) {
            methods.push(extension.as_str());
        }
    }
    methods.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document_json(error: &ErrorObject) -> String {
        serde_json::to_string(&error.document()).unwrap()
    }

    #[test]
    fn not_found_document_matches_the_contract_example() {
        // §8.1 / §13.1: an AppError-mapped 404.
        let error = ErrorObject::app(StatusCode::NOT_FOUND, "task_not_found", "Task not found: nope");
        assert_eq!(
            document_json(&error),
            r#"{"jsonapi":{"version":"1.1"},"errors":[{"status":"404","code":"task_not_found","title":"Not Found","detail":"Task not found: nope"}]}"#
        );
    }

    #[test]
    fn page_limit_document_matches_the_contract_example() {
        // §7.2
        let error = ErrorObject::new(
            ErrorCode::InvalidQueryParameter,
            "page[limit] must be an integer between 1 and 4294967295",
        )
        .with_parameter("page[limit]");
        assert_eq!(
            document_json(&error),
            concat!(
                r#"{"jsonapi":{"version":"1.1"},"errors":[{"status":"400","code":"invalid_query_parameter","#,
                r#""title":"Bad Request","detail":"page[limit] must be an integer between 1 and 4294967295","#,
                r#""source":{"parameter":"page[limit]"}}]}"#
            )
        );
    }

    #[test]
    fn header_source_is_written() {
        let error = ErrorObject::new(ErrorCode::NotAcceptable, "x").with_header("Accept");
        assert_eq!(
            serde_json::to_string(&error).unwrap(),
            r#"{"status":"406","code":"not_acceptable","title":"Not Acceptable","detail":"x","source":{"header":"Accept"}}"#
        );
    }

    #[test]
    fn every_code_has_its_table_status() {
        let expected = [
            ("invalid_query_parameter", 400),
            ("invalid_sort_field", 400),
            ("invalid_include_path", 400),
            ("invalid_path_parameter", 400),
            ("invalid_document", 400),
            ("unknown_attribute", 400),
            ("missing_attribute", 400),
            ("invalid_attribute", 400),
            ("unknown_relationship", 400),
            ("missing_relationship", 400),
            ("relationship_required", 403),
            ("relationship_update_unsupported", 403),
            ("relationship_batch_unsupported", 403),
            ("relationship_cycle", 403),
            ("no_such_related_resource", 404),
            ("no_such_relationship", 404),
            ("method_not_allowed", 405),
            ("not_acceptable", 406),
            ("type_mismatch", 409),
            ("id_mismatch", 409),
            ("content_too_large", 413),
            ("unsupported_media_type", 415),
            ("internal_error", 500),
        ];
        let actual: Vec<(&str, u16)> = ErrorCode::ALL.iter().map(|c| (c.as_str(), c.status().as_u16())).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn titles_are_rfc_9110_reason_phrases() {
        assert_eq!(reason_phrase(StatusCode::CONFLICT), "Conflict");
        assert_eq!(reason_phrase(StatusCode::UNSUPPORTED_MEDIA_TYPE), "Unsupported Media Type");
        assert_eq!(reason_phrase(StatusCode::PAYLOAD_TOO_LARGE), "Content Too Large");
        assert_eq!(reason_phrase(StatusCode::UNPROCESSABLE_ENTITY), "Unprocessable Content");
        assert_eq!(reason_phrase(StatusCode::TOO_MANY_REQUESTS), "Too Many Requests");
    }

    #[test]
    fn all_lists_every_variant_once_in_table_order() {
        // One list drives both an exhaustive match and the comparison with
        // `ALL`: a new variant does not compile until it is listed here, and
        // once listed the assertion requires it in `ALL` at the same position
        // (§13.3's table order).
        macro_rules! assert_all_in_order {
            ($($v:ident),* $(,)?) => {{
                #[allow(dead_code)]
                fn listed(c: ErrorCode) {
                    match c {
                        $(ErrorCode::$v)|* => {}
                    }
                }
                assert_eq!(ErrorCode::ALL, [$(ErrorCode::$v),*].as_slice());
            }};
        }
        assert_all_in_order!(
            InvalidQueryParameter,
            InvalidSortField,
            InvalidIncludePath,
            InvalidPathParameter,
            InvalidDocument,
            UnknownAttribute,
            MissingAttribute,
            InvalidAttribute,
            UnknownRelationship,
            MissingRelationship,
            RelationshipRequired,
            RelationshipUpdateUnsupported,
            RelationshipBatchUnsupported,
            RelationshipCycle,
            NoSuchRelatedResource,
            NoSuchRelationship,
            MethodNotAllowed,
            NotAcceptable,
            TypeMismatch,
            IdMismatch,
            ContentTooLarge,
            UnsupportedMediaType,
            InternalError,
        );
    }

    #[test]
    fn relationship_route_errors_have_no_source() {
        let error = no_such_relationship("tasks", &LookupKey::from("owner"));
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "status": "404",
                "code": "no_such_relationship",
                "title": "Not Found",
                "detail": "`tasks` has no relationship `owner`"
            })
        );
        let undecodable = no_such_relationship("tasks", &LookupKey::undecodable("%FF"));
        assert_eq!(undecodable.detail(), "`tasks` has no relationship `%FF`");

        let error = relationship_update_unsupported("tasks", "epic", "POST");
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "status": "403",
                "code": "relationship_update_unsupported",
                "title": "Forbidden",
                "detail": "the relationship `epic` of `tasks` cannot be changed with POST"
            })
        );
    }

    #[cfg(feature = "axum")]
    #[test]
    fn allow_lists_head_with_get_in_canonical_order() {
        assert_eq!(allow_header(&[Method::DELETE, Method::PATCH, Method::GET]), "GET, HEAD, PATCH, DELETE");
        assert_eq!(allow_header(&[Method::POST, Method::GET]), "GET, HEAD, POST");
        assert_eq!(
            allow_header(&[Method::GET, Method::PATCH, Method::POST, Method::DELETE]),
            "GET, HEAD, PATCH, POST, DELETE"
        );
        assert_eq!(allow_header(&[Method::POST]), "POST");
    }
}
