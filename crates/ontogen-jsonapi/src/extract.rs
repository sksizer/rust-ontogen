//! Axum extractors whose rejections are JSON:API error documents (§13.5).
//!
//! Generated handlers list them in the order of §13.2, because Axum runs
//! extractors in argument order and answers with the first rejection:
//! [`AcceptGuard`], [`ContentTypeGuard`], [`Path`], [`Query`], and last
//! [`Body`], the only one that reads the body. The guards and [`Query`]
//! wrap plain functions in this crate.

mod path_params;

use std::{fmt, marker::PhantomData, ops::Deref};

use axum::{
    body::Bytes,
    extract::{
        FromRequest, FromRequestParts, MatchedPath, OriginalUri, Request,
        rejection::{BytesRejection, FailedToBufferBody},
    },
};
use http::{HeaderMap, header, request::Parts};
use serde::de::DeserializeOwned;

use self::path_params::PathError;
use crate::{
    error::{ErrorCode, ErrorObject},
    media::{check_accept, check_content_type_headers},
    query::{QueryParams, QuerySpec},
};

/// Step 2: rejects with `406 not_acceptable` unless `Accept` admits a
/// JSON:API response.
#[derive(Debug, Clone, Copy)]
pub struct AcceptGuard;

impl<S: Send + Sync> FromRequestParts<S> for AcceptGuard {
    type Rejection = ErrorObject;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        check_accept(&parts.headers).map(|()| AcceptGuard)
    }
}

/// Step 3: rejects with `415 unsupported_media_type` when the request
/// carries a body that is not declared `application/vnd.api+json`.
///
/// The body is not read yet at this step, so its presence is taken from the
/// headers: a non-zero `Content-Length`, or any `Transfer-Encoding`. [`Body`]
/// repeats the check once the body is read, which catches a body sent with
/// neither header (possible on HTTP/2), at the cost of reporting it after
/// path and query errors.
#[derive(Debug, Clone, Copy)]
pub struct ContentTypeGuard;

impl<S: Send + Sync> FromRequestParts<S> for ContentTypeGuard {
    type Rejection = ErrorObject;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        check_content_type_headers(&parts.headers, announces_body(&parts.headers)).map(|()| ContentTypeGuard)
    }
}

fn announces_body(headers: &HeaderMap) -> bool {
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().parse::<u64>().map_or(true, |n| n > 0));
    content_length || headers.contains_key(header::TRANSFER_ENCODING)
}

/// Step 4: the route's path parameters, in place of `axum::extract::Path`:
/// a tuple in path order, a struct by name, or a single value.
///
/// A typed parameter that fails to parse, including one that does not
/// percent-decode to UTF-8, is `400 invalid_path_parameter` with no
/// `source` (§11.1). A [`LookupKey`](crate::LookupKey) never fails: an
/// `{id}` or `{rel}` that cannot name anything is the handler's `404` (§8.1),
/// so read those as `Path<(Uuid, LookupKey)>`, say, not as `String`.
///
/// The segments are read from the request path as sent, because Axum fails
/// every parameter of a path when one does not decode. A route whose
/// parameters do not fit `T` (a wrong count, a name `T` lacks) is a
/// generator bug, and answers `500 internal_error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ErrorObject;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let template = parts.extensions.get::<MatchedPath>().map(MatchedPath::as_str);
        // A nested router strips its prefix from `parts.uri`, but not from
        // the matched template.
        let path = parts.extensions.get::<OriginalUri>().map_or(parts.uri.path(), |uri| uri.path());
        let Some(params) = template.and_then(|template| path_params::params(template, path)) else {
            return Err(ErrorObject::internal("the route's path parameters could not be found in the request path"));
        };
        match T::deserialize(path_params::Params(&params)) {
            Ok(value) => Ok(Path(value)),
            Err(PathError::Invalid(detail)) => Err(ErrorObject::new(ErrorCode::InvalidPathParameter, detail)),
            Err(PathError::Route(detail)) => Err(ErrorObject::internal(detail)),
        }
    }
}

/// The query parameters a route accepts, named by a type so that [`Query`]
/// can carry them.
///
/// ```
/// use ontogen_jsonapi::{QuerySpec, extract::RouteQuery};
///
/// struct ListTasks;
/// impl RouteQuery for ListTasks {
///     const SPEC: QuerySpec = QuerySpec { sort: true, include: true, page: true, ..QuerySpec::NONE };
/// }
/// ```
pub trait RouteQuery {
    /// The accepted set.
    const SPEC: QuerySpec;
}

/// A route that accepts no query parameter.
#[derive(Debug, Clone, Copy)]
pub struct NoParams;

impl RouteQuery for NoParams {
    const SPEC: QuerySpec = QuerySpec::NONE;
}

/// Step 5, first half: the request's query parameters, rejecting the first
/// name, in request order, that `R` does not accept. The handler then calls
/// the [`QueryParams`] accessors in canonical order.
pub struct Query<R> {
    params: QueryParams,
    route: PhantomData<fn() -> R>,
}

impl<R> Query<R> {
    /// The parsed parameters.
    pub fn into_inner(self) -> QueryParams {
        self.params
    }
}

impl<R> Deref for Query<R> {
    type Target = QueryParams;

    fn deref(&self) -> &QueryParams {
        &self.params
    }
}

impl<R> fmt::Debug for Query<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Query").field(&self.params).finish()
    }
}

impl<R: RouteQuery, S: Send + Sync> FromRequestParts<S> for Query<R> {
    type Rejection = ErrorObject;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params = QueryParams::parse(parts.uri.query(), &R::SPEC)?;
        Ok(Query { params, route: PhantomData })
    }
}

/// The request body, in place of `axum::Json`: its bytes, with the media
/// type checked. Read the document from it with [`crate::request`].
///
/// The size is bounded by Axum's `DefaultBodyLimit` (2 MB unless the router
/// sets another), and a body over it is `413 content_too_large` (§13.3).
/// A body that cannot be read otherwise (a broken connection) is `400
/// invalid_document`, since no document can be read from it. JSON nesting is
/// bounded by `serde_json`'s recursion limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body(pub Bytes);

impl<S: Send + Sync> FromRequest<S> for Body {
    type Rejection = ErrorObject;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let announced = announces_body(req.headers());
        check_content_type_headers(req.headers(), announced)?;
        // Headers that announce no body passed without a look at
        // `Content-Type`; it is checked again once the body is read.
        let unchecked = (!announced).then(|| content_type_only(req.headers()));
        let bytes = Bytes::from_request(req, state).await.map_err(unreadable)?;
        if let Some(headers) = unchecked {
            check_content_type_headers(&headers, !bytes.is_empty())?;
        }
        Ok(Body(bytes))
    }
}

fn content_type_only(headers: &HeaderMap) -> HeaderMap {
    let mut only = HeaderMap::new();
    for value in headers.get_all(header::CONTENT_TYPE) {
        only.append(header::CONTENT_TYPE, value.clone());
    }
    only
}

fn unreadable(err: BytesRejection) -> ErrorObject {
    if let BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_)) = err {
        return ErrorObject::new(ErrorCode::ContentTooLarge, "the request body is larger than this server accepts");
    }
    ErrorObject::new(ErrorCode::InvalidDocument, format!("the request body could not be read: {}", err.body_text()))
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body as HttpBody, to_bytes},
        extract::DefaultBodyLimit,
        response::Response,
        routing::{get, post},
    };
    use http::{HeaderValue, Method, Request as HttpRequest, StatusCode};
    use tower::ServiceExt;

    use super::*;
    use crate::{
        LookupKey, MEDIA_TYPE,
        error::{method_not_allowed, method_not_allowed_fallback},
        request::{Endpoint, parse_create},
        response,
    };

    struct ListParams;
    impl RouteQuery for ListParams {
        const SPEC: QuerySpec = QuerySpec { page: true, ..QuerySpec::NONE };
    }

    /// The store lookup a generated handler makes last (§13.2 step 9).
    fn find_task(id: &LookupKey) -> Result<&str, ErrorObject> {
        match id.as_str() {
            Some(id) if id != "nope" => Ok(id),
            _ => Err(ErrorObject::app(StatusCode::NOT_FOUND, "task_not_found", format!("Task not found: {id}"))),
        }
    }

    async fn get_task(
        _: AcceptGuard,
        Path(id): Path<LookupKey>,
        query: Query<ListParams>,
    ) -> Result<Response, ErrorObject> {
        let limit = query.page_limit()?;
        let id = find_task(&id)?;
        Ok(response::ok(&serde_json::json!({ "id": id, "limit": limit })))
    }

    async fn get_scoped_task(
        _: AcceptGuard,
        Path((project_id, id)): Path<(u32, LookupKey)>,
    ) -> Result<Response, ErrorObject> {
        let id = find_task(&id)?;
        Ok(response::ok(&serde_json::json!({ "project_id": project_id, "id": id })))
    }

    async fn create_task(
        _: AcceptGuard,
        _: ContentTypeGuard,
        _: Query<NoParams>,
        body: Body,
    ) -> Result<Response, ErrorObject> {
        let data =
            parse_create(&body.0, Endpoint { type_name: "tasks", path: "/api/tasks" }, |_| Ok::<(), String>(()))?;
        let id = data.id.unwrap_or_else(|| "derived".to_owned());
        Ok(response::created(&format!("/api/tasks/{id}"), &serde_json::json!({ "id": id })))
    }

    async fn delete_task(_: AcceptGuard, Path(_id): Path<u32>) -> Response {
        response::no_content()
    }

    async fn mismatched_path(Path(_id): Path<String>) -> Response {
        response::no_content()
    }

    fn app() -> Router {
        Router::new()
            .route(
                "/api/tasks/{id}",
                get(get_task).delete(delete_task).fallback(|method: Method| async move {
                    method_not_allowed(&method, &[Method::GET, Method::DELETE])
                }),
            )
            .route("/api/tasks", post(create_task))
            .route("/api/projects/{project_id}/tasks/{id}", get(get_scoped_task))
            .route("/api/small/tasks", post(create_task).layer(DefaultBodyLimit::max(16)))
            .route("/api/{prefix}/broken/{id}", get(mismatched_path))
            .method_not_allowed_fallback(method_not_allowed_fallback)
    }

    async fn send(request: HttpRequest<HttpBody>) -> (StatusCode, HeaderMap, serde_json::Value) {
        send_to(app(), request).await
    }

    async fn send_to(app: Router, request: HttpRequest<HttpBody>) -> (StatusCode, HeaderMap, serde_json::Value) {
        let response = app.oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let json = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
        (parts.status, parts.headers, json)
    }

    fn request(method: &str, uri: &str) -> axum::http::request::Builder {
        HttpRequest::builder().method(method).uri(uri)
    }

    fn assert_error(headers: &HeaderMap, body: &serde_json::Value, code: &str) {
        assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), MEDIA_TYPE);
        assert_eq!(headers.get(header::VARY).unwrap(), "Accept");
        assert_eq!(body["jsonapi"], serde_json::json!({ "version": "1.1" }));
        assert_eq!(body["errors"].as_array().unwrap().len(), 1);
        assert_eq!(body["errors"][0]["code"], code);
    }

    #[tokio::test]
    async fn success_carries_the_media_type_and_vary() {
        let (status, headers, body) =
            send(request("GET", "/api/tasks/a%20b?page%5Blimit%5D=5").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), MEDIA_TYPE);
        assert_eq!(headers.get(header::VARY).unwrap(), "Accept");
        assert_eq!(body, serde_json::json!({ "id": "a b", "limit": 5 }));
    }

    #[tokio::test]
    async fn not_acceptable() {
        let req =
            request("GET", "/api/tasks/x").header(header::ACCEPT, "application/json").body(HttpBody::empty()).unwrap();
        let (status, headers, body) = send(req).await;
        assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
        assert_error(&headers, &body, "not_acceptable");
        assert_eq!(body["errors"][0]["source"], serde_json::json!({ "header": "Accept" }));
    }

    #[tokio::test]
    async fn accept_is_checked_before_the_query() {
        let req =
            request("GET", "/api/tasks/x?bogus=1").header(header::ACCEPT, "text/html").body(HttpBody::empty()).unwrap();
        assert_eq!(send(req).await.0, StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn invalid_query_parameters() {
        let (status, headers, body) =
            send(request("GET", "/api/tasks/x?sort=title").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error(&headers, &body, "invalid_query_parameter");
        assert_eq!(body["errors"][0]["source"], serde_json::json!({ "parameter": "sort" }));

        let (status, _, body) =
            send(request("GET", "/api/tasks/x?page[limit]=0").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["errors"][0]["source"], serde_json::json!({ "parameter": "page[limit]" }));
    }

    #[tokio::test]
    async fn invalid_path_parameter_has_no_source() {
        let (status, headers, body) =
            send(request("DELETE", "/api/tasks/not-a-number").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error(&headers, &body, "invalid_path_parameter");
        assert!(body["errors"][0].get("source").is_none());
    }

    #[tokio::test]
    async fn an_undecodable_typed_parameter_is_an_invalid_path_parameter() {
        let (status, headers, body) = send(request("DELETE", "/api/tasks/%FF").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error(&headers, &body, "invalid_path_parameter");
        assert!(body["errors"][0].get("source").is_none());
    }

    #[tokio::test]
    async fn an_undecodable_id_is_the_stores_not_found() {
        // §8.1: the id is never validated, so it reaches the lookup.
        let (status, headers, body) = send(request("GET", "/api/tasks/%FF").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_error(&headers, &body, "task_not_found");
        assert_eq!(body["errors"][0]["detail"], "Task not found: %FF");
        // After the query checks, as any missing id is (§13.2).
        let (status, _, body) =
            send(request("GET", "/api/tasks/%FF?page[limit]=0").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["errors"][0]["code"], "invalid_query_parameter");
    }

    #[tokio::test]
    async fn prefix_parameters_parse_beside_an_undecodable_id() {
        let get = |uri: &'static str| request("GET", uri).body(HttpBody::empty()).unwrap();
        let (status, _, body) = send(get("/api/projects/7/tasks/a%20b")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({ "project_id": 7, "id": "a b" }));
        let (status, _, body) = send(get("/api/projects/7/tasks/%FF")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["errors"][0]["code"], "task_not_found");
        // A bad prefix is still a 400, whatever the id (§11.1).
        for uri in ["/api/projects/x/tasks/%FF", "/api/projects/%FF/tasks/a"] {
            let (status, _, body) = send(get(uri)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert_eq!(body["errors"][0]["code"], "invalid_path_parameter", "{uri}");
        }
    }

    #[tokio::test]
    async fn path_parameters_are_found_under_a_nested_router() {
        let nested = || Router::new().nest("/v1", app());
        let get = |uri: &'static str| request("GET", uri).body(HttpBody::empty()).unwrap();
        let (status, _, body) = send_to(nested(), get("/v1/api/projects/7/tasks/a%20b")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({ "project_id": 7, "id": "a b" }));
        let (status, _, body) = send_to(nested(), get("/v1/api/tasks/%FF")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["errors"][0]["code"], "task_not_found");
    }

    #[tokio::test]
    async fn a_path_that_does_not_fit_the_route_is_an_internal_error() {
        let (status, headers, body) = send(request("GET", "/api/x/broken/7").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_error(&headers, &body, "internal_error");
    }

    #[tokio::test]
    async fn no_content_has_vary_and_no_content_type() {
        let (status, headers, body) = send(request("DELETE", "/api/tasks/7").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(headers.get(header::VARY).unwrap(), "Accept");
        assert!(headers.get(header::CONTENT_TYPE).is_none());
        assert_eq!(body, serde_json::Value::Null);
    }

    fn post_tasks(content_type: Option<&str>, body: &'static str) -> HttpRequest<HttpBody> {
        let mut builder = request("POST", "/api/tasks").header(header::CONTENT_LENGTH, body.len());
        if let Some(ct) = content_type {
            builder = builder.header(header::CONTENT_TYPE, ct);
        }
        builder.body(HttpBody::from(body)).unwrap()
    }

    #[tokio::test]
    async fn create_through_the_extractors() {
        let (status, headers, body) =
            send(post_tasks(Some(MEDIA_TYPE), r#"{"data":{"type":"tasks","id":"review-q3"}}"#)).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(headers.get(header::LOCATION).unwrap(), "/api/tasks/review-q3");
        assert_eq!(body, serde_json::json!({ "id": "review-q3" }));
    }

    #[tokio::test]
    async fn unsupported_media_type() {
        for ct in [Some("application/json"), None, Some("application/vnd.api+json; ext=\"x\"")] {
            let (status, headers, body) = send(post_tasks(ct, r#"{"data":{"type":"tasks"}}"#)).await;
            assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{ct:?}");
            assert_error(&headers, &body, "unsupported_media_type");
            assert_eq!(body["errors"][0]["source"], serde_json::json!({ "header": "Content-Type" }));
        }
    }

    #[tokio::test]
    async fn content_type_is_checked_before_the_query() {
        let req = request("POST", "/api/tasks?x=1")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, 2)
            .body(HttpBody::from("{}"))
            .unwrap();
        assert_eq!(send(req).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn a_body_without_length_headers_is_still_checked() {
        // No Content-Length: the guard sees no body, Body sees one.
        let req = request("POST", "/api/tasks").body(HttpBody::from(r#"{"data":{"type":"tasks"}}"#)).unwrap();
        assert_eq!(send(req).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn an_empty_body_is_an_invalid_document_not_a_415() {
        let (status, headers, body) = send(post_tasks(None, "")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error(&headers, &body, "invalid_document");
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_content_too_large() {
        let body = r#"{"data":{"type":"tasks","id":"review-q3"}}"#;
        let req = request("POST", "/api/small/tasks")
            .header(header::CONTENT_TYPE, MEDIA_TYPE)
            .header(header::CONTENT_LENGTH, body.len())
            .body(HttpBody::from(body))
            .unwrap();
        let (status, headers, json) = send(req).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_error(&headers, &json, "content_too_large");
        assert_eq!(json["errors"][0]["title"], "Content Too Large");
        // Without a Content-Length the limit is met while reading.
        let req = request("POST", "/api/small/tasks")
            .header(header::CONTENT_TYPE, MEDIA_TYPE)
            .body(HttpBody::from(body))
            .unwrap();
        assert_eq!(send(req).await.0, StatusCode::PAYLOAD_TOO_LARGE);
        // Within the limit the route works.
        let req = request("POST", "/api/small/tasks")
            .header(header::CONTENT_TYPE, MEDIA_TYPE)
            .body(HttpBody::from(r#"{"data":1}"#))
            .unwrap();
        assert_eq!(send(req).await.0, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn type_mismatch_through_the_extractors() {
        let (status, _, body) = send(post_tasks(Some(MEDIA_TYPE), r#"{"data":{"type":"epics"}}"#)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["errors"][0]["code"], "type_mismatch");
        assert_eq!(body["errors"][0]["source"], serde_json::json!({ "pointer": "/data/type" }));
    }

    #[tokio::test]
    async fn per_route_405_writes_the_canonical_allow() {
        let (status, headers, body) = send(request("PUT", "/api/tasks/x").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers.get(header::ALLOW).unwrap(), "GET, HEAD, DELETE");
        assert_error(&headers, &body, "method_not_allowed");
        assert_eq!(body["errors"][0]["title"], "Method Not Allowed");
    }

    #[tokio::test]
    async fn router_level_405_keeps_axums_allow() {
        let (status, headers, body) = send(request("PUT", "/api/tasks").body(HttpBody::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(headers.get(header::ALLOW), Some(&HeaderValue::from_static("POST")));
        assert_error(&headers, &body, "method_not_allowed");
    }

    #[test]
    fn body_presence_from_headers() {
        let mut headers = HeaderMap::new();
        assert!(!announces_body(&headers));
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
        assert!(!announces_body(&headers));
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("12"));
        assert!(announces_body(&headers));
        let mut chunked = HeaderMap::new();
        chunked.insert(header::TRANSFER_ENCODING, HeaderValue::from_static("chunked"));
        assert!(announces_body(&chunked));
    }
}
