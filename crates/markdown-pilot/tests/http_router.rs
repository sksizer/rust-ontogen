//! Proof that the generated axum HTTP handlers compile against the axum
//! version in Cargo.toml, that the router builds, and that it speaks the
//! JSON:API wire contract (`docs/jsonapi-wire-contract.md`) over a real
//! temp vault.
//!
//! The construction half matters as much as the compile half: axum 0.8
//! rejects 0.7-style `/:param` paths with a panic at `Router::route` time,
//! not at compile time — a build-only CI check stays green while every
//! consumer panics at startup.
//!
//! The pilot paginates every module with `default_limit: 2, max_limit: 3`.
//! `scoped_routes` is the same API generated under the route prefix
//! `projects/:project_id`, whose one project is `PROJECT`.

use std::sync::Arc;

use std::time::Duration;

use axum::Router;
use axum::body::{Body, BodyDataStream, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use futures::StreamExt;
use markdown_pilot::api::transport::http::generated::entity_routes;
use markdown_pilot::api::transport::http_scoped::generated::entity_routes as scoped_routes;
use markdown_pilot::persistence::markdown::generated::open_vault;
use markdown_pilot::schema::{Note, Section, Tag, Task};
use markdown_pilot::{AppState, PROJECT, Store};
use serde_json::{Value, json};
use tower::util::ServiceExt;

const MEDIA_TYPE: &str = "application/vnd.api+json";

#[test]
fn entity_routes_constructs_router() {
    // A panic here means the emitted route syntax is invalid for the axum
    // version this crate compiles against.
    let _router = entity_routes();
    let _scoped = scoped_routes();
}

/// A response: status, headers, the body parsed, and the body as sent.
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
    raw: String,
}

/// A vault in a tempdir behind the generated router.
struct Server {
    _dir: tempfile::TempDir,
    state: Arc<AppState>,
}

impl Server {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(open_vault(dir.path()));
        Server { _dir: dir, state: Arc::new(AppState::new(store)) }
    }

    fn store(&self) -> &Store {
        &self.state.store
    }

    async fn send(&self, request: Request<Body>) -> Reply {
        self.send_to(entity_routes(), request).await
    }

    async fn send_to(&self, router: Router<Arc<AppState>>, request: Request<Body>) -> Reply {
        let response = router.with_state(Arc::clone(&self.state)).oneshot(request).await.expect("infallible");
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        let raw = String::from_utf8(bytes.to_vec()).expect("UTF-8 body");
        let body = if raw.is_empty() { Value::Null } else { serde_json::from_str(&raw).expect("JSON body") };
        Reply { status: parts.status, headers: parts.headers, body, raw }
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Request::get(uri).header(header::ACCEPT, MEDIA_TYPE).body(Body::empty()).unwrap()).await
    }

    /// A `GET` to the router generated under `projects/:project_id`.
    async fn get_scoped(&self, uri: &str) -> Reply {
        let request = Request::get(uri).header(header::ACCEPT, MEDIA_TYPE).body(Body::empty()).unwrap();
        self.send_to(scoped_routes(), request).await
    }

    async fn write(&self, method: &str, uri: &str, body: Value) -> Reply {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::ACCEPT, MEDIA_TYPE)
            .header(header::CONTENT_TYPE, MEDIA_TYPE)
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send(request).await
    }

    /// A request with exactly the given headers and body.
    async fn raw(&self, method: &str, uri: &str, headers: &[(header::HeaderName, &str)], body: &str) -> Reply {
        let mut request = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        self.send(request.body(Body::from(body.to_string())).unwrap()).await
    }

    /// A custom-op request whose body is `body` as sent, with JSON:API
    /// media types.
    async fn op(&self, method: &str, uri: &str, body: &str) -> Reply {
        self.raw(method, uri, &[(header::ACCEPT, MEDIA_TYPE), (header::CONTENT_TYPE, MEDIA_TYPE)], body).await
    }

    /// Opens an event stream. The handler has subscribed once this returns,
    /// so an event sent afterwards reaches the returned body.
    async fn subscribe(&self, uri: &str) -> (StatusCode, HeaderMap, BodyDataStream) {
        let request = Request::get(uri).header(header::ACCEPT, "text/event-stream").body(Body::empty()).unwrap();
        let response = entity_routes().with_state(Arc::clone(&self.state)).oneshot(request).await.expect("infallible");
        let (parts, body) = response.into_parts();
        (parts.status, parts.headers, body.into_data_stream())
    }

    async fn task(&self, title: &str, status: &str) -> Task {
        let task = Task {
            id: String::new(),
            title: title.into(),
            status: status.into(),
            parent_id: None,
            subtasks: vec![],
            tags: vec![],
            body: String::new(),
        };
        self.store().create_task(task).await.expect("create task")
    }

    async fn tag(&self, id: &str, title: &str) {
        self.store().create_tag(Tag { id: id.into(), title: title.into() }).await.expect("create tag");
    }

    async fn section(&self, id: &str, title: &str, parent_id: &str) {
        let section = Section { id: id.into(), title: title.into(), parent_id: parent_id.into(), children: vec![] };
        self.store().create_section(section).await.expect("create section");
    }

    async fn bookmark(&self, url: &str, title: &str) {
        let input = json!({ "url": url, "title": title });
        let reply = self.op("POST", "/api/bookmarks", &args(json!({ "input": input }))).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    }

    async fn note(&self, title: &str) -> Note {
        let note = Note { id: String::new(), title: title.into(), body: format!("About {title}.\n") };
        self.store().create_note(note).await.expect("create note")
    }
}

impl Reply {
    /// The single error object of an error document, after checking the
    /// document's shape, status and media type.
    fn error(&self, expected: StatusCode, code: &str) -> Value {
        assert_eq!(self.status, expected, "{}", self.raw);
        assert_eq!(self.headers[header::CONTENT_TYPE], MEDIA_TYPE);
        assert_eq!(self.headers[header::VARY], "Accept");
        assert_eq!(self.body["jsonapi"], json!({ "version": "1.1" }));
        let errors = self.body["errors"].as_array().expect("an errors array");
        assert_eq!(errors.len(), 1, "one error object per response: {}", self.raw);
        assert_eq!(errors[0]["status"], expected.as_str());
        assert_eq!(errors[0]["code"], code, "{}", self.raw);
        errors[0].clone()
    }

    fn pointer(&self, expected: StatusCode, code: &str) -> Value {
        self.error(expected, code)["source"]["pointer"].clone()
    }
}

#[tokio::test]
async fn generated_routes_serve_requests() {
    let server = Server::new();
    let created = server.note("Hello Vault").await;

    // Collection route.
    assert_eq!(server.get("/api/notes").await.status, StatusCode::OK);

    // Path-param route: proves the `{id}` segment actually captures.
    assert_eq!(server.get(&format!("/api/notes/{}", created.id)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn a_list_is_a_paginated_collection_document() {
    let server = Server::new();
    for title in ["Alpha", "Beta", "Gamma"] {
        server.note(title).await;
    }

    let reply = server.get("/api/notes").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[header::CONTENT_TYPE], MEDIA_TYPE);
    assert_eq!(reply.headers[header::VARY], "Accept");
    // Byte for byte, so member order is pinned too (§4.1, §5.2).
    assert_eq!(
        reply.raw,
        concat!(
            r#"{"jsonapi":{"version":"1.1"},"links":{"#,
            r#""self":"/api/notes?page%5Boffset%5D=0&page%5Blimit%5D=2","#,
            r#""first":"/api/notes?page%5Boffset%5D=0&page%5Blimit%5D=2","prev":null,"#,
            r#""next":"/api/notes?page%5Boffset%5D=2&page%5Blimit%5D=2","#,
            r#""last":"/api/notes?page%5Boffset%5D=2&page%5Blimit%5D=2"},"#,
            r#""meta":{"total":3,"limit":2,"offset":0},"data":["#,
            r#"{"type":"notes","id":"alpha","attributes":{"title":"Alpha","body":"About Alpha.\n"},"#,
            r#""relationships":{"tags":{"links":{"self":"/api/notes/alpha/relationships/tags","#,
            r#""related":"/api/notes/alpha/tags"}}},"links":{"self":"/api/notes/alpha"}},"#,
            r#"{"type":"notes","id":"beta","attributes":{"title":"Beta","body":"About Beta.\n"},"#,
            r#""relationships":{"tags":{"links":{"self":"/api/notes/beta/relationships/tags","#,
            r#""related":"/api/notes/beta/tags"}}},"links":{"self":"/api/notes/beta"}}]}"#,
        )
    );

    // The last page; brackets may come raw.
    let body = server.get("/api/notes?page[offset]=2&page[limit]=2").await.body;
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["links"]["next"], Value::Null);
    assert_eq!(body["links"]["prev"], "/api/notes?page%5Boffset%5D=0&page%5Blimit%5D=2");

    // A limit above `max_limit` is clamped silently.
    let reply = server.get("/api/notes?page%5Blimit%5D=50").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["meta"], json!({ "total": 3, "limit": 3, "offset": 0 }));
    assert_eq!(reply.body["links"]["next"], Value::Null);

    // An empty collection is still a `200`, with `data: []`.
    let reply = server.get("/api/tags").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["data"], json!([]));
    assert_eq!(reply.body["meta"], json!({ "total": 0, "limit": 2, "offset": 0 }));
}

#[tokio::test]
async fn list_query_parameters_are_checked() {
    let server = Server::new();

    let error = server.get("/api/notes?page[limit]=0").await.error(StatusCode::BAD_REQUEST, "invalid_query_parameter");
    assert_eq!(error["source"], json!({ "parameter": "page[limit]" }));

    let error = server.get("/api/notes?foo=1").await.error(StatusCode::BAD_REQUEST, "invalid_query_parameter");
    assert_eq!(error["source"], json!({ "parameter": "foo" }));

    // The generated list takes no filter.
    assert_eq!(server.get("/api/notes?filter[title]=x").await.parameter("invalid_query_parameter"), "filter[title]");

    // `sort` and `include` are accepted names that no route honours, and
    // `sort` is checked first (§13.2 step 5).
    let error =
        server.get("/api/notes?include=x&sort=title").await.error(StatusCode::BAD_REQUEST, "invalid_sort_field");
    assert_eq!(error["source"], json!({ "parameter": "sort" }));
    let error = server.get("/api/notes/x?include=x").await.error(StatusCode::BAD_REQUEST, "invalid_include_path");
    assert_eq!(error["source"], json!({ "parameter": "include" }));
}

#[tokio::test]
async fn get_and_its_missing_case() {
    let server = Server::new();
    server.note("Hello Vault").await;

    let reply = server.get("/api/notes/hello-vault").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.raw,
        concat!(
            r#"{"jsonapi":{"version":"1.1"},"links":{"self":"/api/notes/hello-vault"},"#,
            r#""data":{"type":"notes","id":"hello-vault","#,
            r#""attributes":{"title":"Hello Vault","body":"About Hello Vault.\n"},"#,
            r#""relationships":{"tags":{"links":{"self":"/api/notes/hello-vault/relationships/tags","#,
            r#""related":"/api/notes/hello-vault/tags"}}},"links":{"self":"/api/notes/hello-vault"}}}"#,
        )
    );

    let reply = server.get("/api/notes/nope").await;
    assert_eq!(
        reply.raw,
        concat!(
            r#"{"jsonapi":{"version":"1.1"},"errors":[{"status":"404","code":"note_not_found","#,
            r#""title":"Not Found","detail":"Note not found: nope"}]}"#,
        )
    );
    reply.error(StatusCode::NOT_FOUND, "note_not_found");
}

#[tokio::test]
async fn relationships_carry_linkage_in_declared_order() {
    let server = Server::new();
    server.store().create_tag(Tag { id: "codegen".into(), title: "Codegen".into() }).await.expect("tag");

    let reply = server
        .write(
            "POST",
            "/api/tasks",
            json!({ "data": { "type": "tasks", "attributes": {
                "title": "Ship it", "status": "open", "body": "Go.\n"
            }, "relationships": {
                "tags": { "data": [
                    { "type": "tags", "id": "codegen" }, { "type": "tags", "id": "codegen" }
                ] }
            } } }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.raw);
    assert_eq!(reply.headers[header::LOCATION], "/api/tasks/ship-it");
    // Duplicates collapse to their first occurrence (§5.4). Each
    // relationship links its relationship and related routes, and the
    // junction relationship `labels` has the links alone.
    let links = |rel: &str| {
        format!(r#"{{"self":"/api/tasks/ship-it/relationships/{rel}","related":"/api/tasks/ship-it/{rel}"}}"#)
    };
    let relationships = format!(
        r#""relationships":{{"parent":{{"links":{},"data":null}},"subtasks":{{"links":{},"data":[]}},"tags":{{"links":{},"data":[{{"type":"tags","id":"codegen"}}]}},"labels":{{"links":{}}}}},"links":{{"self":"/api/tasks/ship-it"}}}}"#,
        links("parent"),
        links("subtasks"),
        links("tags"),
        links("labels"),
    );
    assert!(reply.raw.contains(&relationships), "{}", reply.raw);
}

#[tokio::test]
async fn create_answers_201_with_location() {
    let server = Server::new();
    let post = |body: Value| server.write("POST", "/api/notes", body);

    // Without an id: the id strategy slugs the title.
    let reply =
        post(json!({ "data": { "type": "notes", "attributes": { "title": "Review the stack", "body": "x\n" } } }))
            .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.raw);
    assert_eq!(reply.headers[header::LOCATION], "/api/notes/review-the-stack");
    assert_eq!(reply.body["data"]["links"]["self"], "/api/notes/review-the-stack");
    assert_eq!(reply.body["links"]["self"], "/api/notes/review-the-stack");

    // With a client id.
    let q3 = json!({ "data": { "type": "notes", "id": "review-q3", "attributes": { "title": "Q3", "body": "" } } });
    let reply = post(q3.clone()).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.raw);
    assert_eq!(reply.headers[header::LOCATION], "/api/notes/review-q3");

    // The same client id again is the store's 409, pointing at it.
    assert_eq!(post(q3).await.pointer(StatusCode::CONFLICT, "note_already_exists"), "/data/id");

    // A reserved id is not a valid one.
    let index = json!({ "data": { "type": "notes", "id": "Index", "attributes": { "title": "I", "body": "" } } });
    assert_eq!(post(index).await.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/data/id");
}

#[tokio::test]
async fn create_documents_are_checked_member_by_member() {
    let server = Server::new();
    let post = |body: Value| server.write("POST", "/api/tasks", body);
    let task = |attributes: Value, relationships: Value| json!({ "data": { "type": "tasks", "attributes": attributes, "relationships": relationships } });

    // The flat shape is refused, and the detail names the relationship.
    let reply = post(task(json!({ "title": "T", "status": "s", "body": "", "parent_id": "x" }), json!({}))).await;
    let error = reply.error(StatusCode::BAD_REQUEST, "unknown_attribute");
    assert_eq!(error["source"]["pointer"], "/data/attributes/parent_id");
    assert_eq!(error["detail"], "`parent_id` is not an attribute of `tasks`; it is the `parent` relationship");

    let reply = post(task(json!({ "title": "T", "body": "" }), json!({}))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "missing_attribute"), "/data/attributes");

    let reply = post(task(json!({ "title": 7, "status": "s", "body": "" }), json!({}))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_attribute"), "/data/attributes/title");

    let attributes = json!({ "title": "T", "status": "s", "body": "" });
    let reply = post(task(attributes.clone(), json!({ "owner": { "data": null } }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "unknown_relationship"), "/data/relationships/owner");

    let reply = post(task(attributes.clone(), json!({ "tags": { "data": [{ "type": "notes", "id": "x" }] } }))).await;
    assert_eq!(reply.pointer(StatusCode::CONFLICT, "type_mismatch"), "/data/relationships/tags/data/0");

    // Step 8: a linked resource must exist.
    let reply =
        post(task(attributes.clone(), json!({ "parent": { "data": { "type": "tasks", "id": "nope" } } }))).await;
    assert_eq!(reply.pointer(StatusCode::NOT_FOUND, "related_resource_not_found"), "/data/relationships/parent/data");

    // A section's parent is a required to-one.
    let section = json!({ "data": { "type": "sections", "attributes": { "title": "S" } } });
    let reply = server.write("POST", "/api/sections", section).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "missing_relationship"), "/data");

    let reply = post(json!({ "data": { "type": "notes", "attributes": {} } })).await;
    assert_eq!(reply.pointer(StatusCode::CONFLICT, "type_mismatch"), "/data/type");
}

#[tokio::test]
async fn patch_updates_and_put_is_not_allowed() {
    let server = Server::new();
    server.note("Hello Vault").await;

    let reply = server
        .write(
            "PATCH",
            "/api/notes/hello-vault",
            json!({ "data": { "type": "notes", "id": "hello-vault", "attributes": { "title": "Renamed" } } }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    assert_eq!(reply.body["data"]["attributes"], json!({ "title": "Renamed", "body": "About Hello Vault.\n" }));
    assert_eq!(reply.body["links"]["self"], "/api/notes/hello-vault");

    let reply =
        server.write("PATCH", "/api/notes/hello-vault", json!({ "data": { "type": "notes", "id": "other" } })).await;
    assert_eq!(reply.pointer(StatusCode::CONFLICT, "id_mismatch"), "/data/id");

    // A missing resource is the store's 404, after the body checks.
    let reply = server.write("PATCH", "/api/notes/nope", json!({ "data": { "type": "notes", "id": "nope" } })).await;
    reply.error(StatusCode::NOT_FOUND, "note_not_found");

    let reply = server
        .write("PUT", "/api/notes/hello-vault", json!({ "data": { "type": "notes", "id": "hello-vault" } }))
        .await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD, PATCH, DELETE");

    let reply = server.write("PUT", "/api/notes", json!({})).await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD, POST");
}

#[tokio::test]
async fn media_types_are_negotiated() {
    let server = Server::new();

    let request = Request::post("/api/notes")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"data":{"type":"notes","attributes":{"title":"T","body":""}}}"#))
        .unwrap();
    let error = server.send(request).await.error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
    assert_eq!(error["source"], json!({ "header": "Content-Type" }));

    let request = Request::get("/api/notes").header(header::ACCEPT, "application/json").body(Body::empty()).unwrap();
    let error = server.send(request).await.error(StatusCode::NOT_ACCEPTABLE, "not_acceptable");
    assert_eq!(error["source"], json!({ "header": "Accept" }));

    // No `Accept` at all, or `*/*`, is acceptable.
    let request = Request::get("/api/notes").body(Body::empty()).unwrap();
    assert_eq!(server.send(request).await.status, StatusCode::OK);
    let request = Request::get("/api/notes").header(header::ACCEPT, "*/*").body(Body::empty()).unwrap();
    assert_eq!(server.send(request).await.status, StatusCode::OK);
}

#[tokio::test]
async fn the_media_type_is_checked_before_the_query() {
    let server = Server::new();
    server.note("Hello Vault").await;

    for (method, uri) in [("POST", "/api/notes?x=1"), ("PATCH", "/api/notes/hello-vault?x=1")] {
        // With and without a Content-Length announcing the body.
        for announce in [false, true] {
            let body = r#"{"data":{"type":"notes","attributes":{"title":"T"}}}"#;
            let mut request = Request::builder().method(method).uri(uri).header(header::CONTENT_TYPE, "text/plain");
            if announce {
                request = request.header(header::CONTENT_LENGTH, body.len());
            }
            let reply = server.send(request.body(Body::from(body)).unwrap()).await;
            let error = reply.error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
            assert_eq!(error["source"], json!({ "header": "Content-Type" }), "{method} {uri}");
        }
    }
}

#[tokio::test]
async fn delete_answers_204() {
    let server = Server::new();
    server.note("Hello Vault").await;

    let reply = server.send(Request::delete("/api/notes/hello-vault").body(Body::empty()).unwrap()).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert!(reply.raw.is_empty());
    assert!(reply.headers.get(header::CONTENT_TYPE).is_none());
    assert_eq!(reply.headers[header::VARY], "Accept");

    server.get("/api/notes/hello-vault").await.error(StatusCode::NOT_FOUND, "note_not_found");

    let reply = server.send(Request::delete("/api/notes/hello-vault?x=1").body(Body::empty()).unwrap()).await;
    reply.error(StatusCode::BAD_REQUEST, "invalid_query_parameter");
}

/// The generated router a request goes to: the unscoped one, or the one
/// generated under `projects/:project_id`, at [`PROJECT`]. A scoped route
/// answers as its unscoped twin with the prefix in its links (§11.1), so
/// the relationship tests run against both.
#[derive(Debug, Clone, Copy)]
enum Scope {
    Unscoped,
    Scoped,
}

impl Scope {
    /// The `/api/…` path `uri` as this router serves it, which is also how
    /// it writes the link.
    fn path(self, uri: &str) -> String {
        match self {
            Scope::Unscoped => uri.to_string(),
            Scope::Scoped => uri.replacen("/api/", &format!("/api/{}/", project_path()), 1),
        }
    }

    /// A document the unscoped router sends, with every link this router's.
    fn links(self, raw: &str) -> String {
        match self {
            Scope::Unscoped => raw.to_string(),
            Scope::Scoped => raw.replace("\"/api/", &format!("\"/api/{}/", project_path())),
        }
    }

    fn router(self) -> Router<Arc<AppState>> {
        match self {
            Scope::Unscoped => entity_routes(),
            Scope::Scoped => scoped_routes(),
        }
    }
}

/// Runs `async fn $name(scope: Scope)` as two tests, `$name::unscoped` and
/// `$name::scoped`.
macro_rules! in_both_scopes {
    ($name:ident) => {
        mod $name {
            #[tokio::test]
            async fn unscoped() {
                super::$name(super::Scope::Unscoped).await
            }

            #[tokio::test]
            async fn scoped() {
                super::$name(super::Scope::Scoped).await
            }
        }
    };
}

// ── Custom ops (§10) ──

/// `{"jsonapi":…,"meta":{"result":<result>}}` as sent.
fn meta_result(result: &str) -> String {
    format!(r#"{{"jsonapi":{{"version":"1.1"}},"meta":{{"result":{result}}}}}"#)
}

/// A `meta.args` document, as sent.
fn args(args: Value) -> String {
    json!({ "meta": { "args": args } }).to_string()
}

fn capture_input(title: &str) -> Value {
    json!({ "title": title, "status": "open", "body": "" })
}

impl Reply {
    /// A `200` meta-only document, checked byte for byte.
    fn ok_result(&self, result: &str) {
        assert_eq!(self.status, StatusCode::OK, "{}", self.raw);
        assert_eq!(self.headers[header::CONTENT_TYPE], MEDIA_TYPE);
        assert_eq!(self.raw, meta_result(result));
    }

    fn no_content(&self) {
        assert_eq!(self.status, StatusCode::NO_CONTENT, "{}", self.raw);
        assert!(self.raw.is_empty());
        assert!(self.headers.get(header::CONTENT_TYPE).is_none());
    }

    fn parameter(&self, code: &str) -> Value {
        self.error(StatusCode::BAD_REQUEST, code)["source"]["parameter"].clone()
    }
}

#[tokio::test]
async fn a_custom_get_takes_its_path_and_op_args() {
    let server = Server::new();
    server.task("Alpha", "open").await;
    server.task("Beta", "open").await;
    server.task("Gamma", "done").await;

    server.get("/api/tasks/summary/open").await.ok_result(r#"{"status":"open","count":2}"#);
    server
        .get("/api/tasks/summary/open?opArg[verbose]=true")
        .await
        .ok_result(r#"{"status":"open","count":2,"titles":["Alpha","Beta"]}"#);
    // Encoded brackets read the same; `opArg[limit]` is a second argument.
    server
        .get("/api/tasks/summary/open?opArg%5Bverbose%5D=true&opArg%5Blimit%5D=1")
        .await
        .ok_result(r#"{"status":"open","count":2,"titles":["Alpha"]}"#);
    server.get("/api/tasks/summary/archived").await.ok_result(r#"{"status":"archived","count":0}"#);
}

#[tokio::test]
async fn custom_get_query_parameters_are_checked() {
    let server = Server::new();

    // An unknown name, an `opArg` the fn does not declare, and the bare
    // all-lowercase name a pre-JSON:API client would send.
    assert_eq!(server.get("/api/tasks/summary/open?x=1").await.parameter("invalid_query_parameter"), "x");
    assert_eq!(
        server.get("/api/tasks/summary/open?opArg[nope]=1").await.parameter("invalid_query_parameter"),
        "opArg[nope]"
    );
    assert_eq!(
        server.get("/api/tasks/summary/open?verbose=true").await.parameter("invalid_query_parameter"),
        "verbose"
    );

    // A malformed value names its parameter.
    assert_eq!(
        server.get("/api/tasks/summary/open?opArg[verbose]=maybe").await.parameter("invalid_query_parameter"),
        "opArg[verbose]"
    );
    // An unknown name is checked before any value, and values in byte
    // order of name: `limit` before `verbose`.
    assert_eq!(
        server.get("/api/tasks/summary/open?opArg[verbose]=maybe&x=1").await.parameter("invalid_query_parameter"),
        "x"
    );
    assert_eq!(
        server
            .get("/api/tasks/summary/open?opArg[verbose]=maybe&opArg[limit]=lots")
            .await
            .parameter("invalid_query_parameter"),
        "opArg[limit]"
    );
}

#[tokio::test]
async fn a_custom_post_reads_its_arguments_from_meta_args() {
    let server = Server::new();

    // An `*Input` argument beside an `Option` one.
    let reply = server
        .op("POST", "/api/tasks/capture", &args(json!({ "input": capture_input("Alpha"), "status": "inbox" })))
        .await;
    // `200`, never `201`, and the result is the fn's plain serde value.
    reply.ok_result(
        r#"{"id":"alpha","title":"Alpha","status":"inbox","parent_id":null,"subtasks":[],"tags":[],"body":""}"#,
    );
    assert!(reply.headers.get(header::LOCATION).is_none());

    // An absent or `null` `Option` is `None`; other top-level members are
    // ignored.
    let reply = server.op("POST", "/api/tasks/capture", &args(json!({ "input": capture_input("Beta") }))).await;
    assert_eq!(reply.body["meta"]["result"]["status"], "open");
    let body = json!({ "data": null, "meta": { "args": { "input": capture_input("Gamma"), "status": null } } });
    let reply = server.op("POST", "/api/tasks/capture", &body.to_string()).await;
    assert_eq!(reply.body["meta"]["result"]["status"], "open");
    assert_eq!(server.store().get_task("gamma").await.expect("captured").status, "open");
}

/// Arguments named `state` and `store` reach the fn as its own, beside the
/// handler's state and store.
#[tokio::test]
async fn an_argument_may_share_a_name_with_a_handler_binding() {
    let server = Server::new();
    server.task("Alpha", "open").await;
    let reply = server
        .op("POST", "/api/tasks/set-state", &args(json!({ "id": "alpha", "state": "blocked", "store": "Renamed" })))
        .await;
    assert_eq!(reply.body["meta"]["result"]["status"], "blocked");
    assert_eq!(reply.body["meta"]["result"]["title"], "Renamed");
    let reply = server.op("POST", "/api/tasks/set-state", &args(json!({ "id": "alpha", "state": "open" }))).await;
    assert_eq!(reply.body["meta"]["result"]["status"], "open");
    assert_eq!(reply.body["meta"]["result"]["title"], "Renamed");
}

#[tokio::test]
async fn a_custom_post_returning_unit_answers_204() {
    let server = Server::new();
    server.task("Alpha", "open").await;

    server.op("POST", "/api/tasks/complete", &args(json!({ "id": "alpha" }))).await.no_content();
    assert_eq!(server.store().get_task("alpha").await.expect("task").status, "done");

    // The op's `AppError` maps through its variant: step 9.
    let reply = server.op("POST", "/api/tasks/complete", &args(json!({ "id": "nope" }))).await;
    assert_eq!(
        reply.raw,
        concat!(
            r#"{"jsonapi":{"version":"1.1"},"errors":[{"status":"404","code":"task_not_found","#,
            r#""title":"Not Found","detail":"Task not found: nope"}]}"#,
        )
    );
    reply.error(StatusCode::NOT_FOUND, "task_not_found");
}

#[tokio::test]
async fn a_post_op_without_arguments_needs_no_body() {
    let server = Server::new();
    server.task("Alpha", "done").await;
    server.task("Beta", "open").await;

    // No body, so no `Content-Type` either.
    server.raw("POST", "/api/tasks/purge-done", &[], "").await.ok_result("1");
    // With no required argument a missing `meta` or `args` reads as `{}`.
    server.op("POST", "/api/tasks/purge-done", "{}").await.ok_result("0");
    server.op("POST", "/api/tasks/purge-done", r#"{"meta":{}}"#).await.ok_result("0");
    server.op("POST", "/api/tasks/purge-done", &args(json!({}))).await.ok_result("0");
    // But it still declares no arguments.
    let reply = server.op("POST", "/api/tasks/purge-done", &args(json!({ "x": 1 }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args/x");
}

#[tokio::test]
async fn custom_post_documents_are_checked_member_by_member() {
    let server = &Server::new();
    let capture = |body: String| async move { server.op("POST", "/api/tasks/capture", &body).await };
    let input = capture_input("Alpha");

    // Not a JSON object, or not JSON: no `source`.
    for body in ["[]", "7", "nope"] {
        let error = capture(body.to_string()).await.error(StatusCode::BAD_REQUEST, "invalid_document");
        assert!(error.get("source").is_none(), "{body}: {error}");
    }

    // The nearest member that exists.
    assert_eq!(capture("{}".into()).await.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "");
    assert_eq!(capture(r#"{"meta":{}}"#.into()).await.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta");
    assert_eq!(capture(r#"{"meta":[]}"#.into()).await.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta");
    assert_eq!(
        capture(r#"{"meta":{"args":[]}}"#.into()).await.pointer(StatusCode::BAD_REQUEST, "invalid_document"),
        "/meta/args"
    );
    // A resource document is not a custom op's document.
    let resource = json!({ "data": { "type": "tasks", "attributes": input } });
    assert_eq!(capture(resource.to_string()).await.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "");

    // Unknown names first, in byte order, then declared arguments in
    // declaration order.
    let reply = capture(args(json!({ "x": 1, "input": input }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args/x");
    let reply = capture(args(json!({ "b": 1, "a/b": 1 }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args/a~1b");
    let reply = capture(args(json!({ "status": "inbox" }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args");
    let reply = capture(args(json!({ "input": 7, "status": 5 }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args/input");
    let reply = capture(args(json!({ "input": input, "status": 5 }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args/status");

    // Nothing was created along the way.
    assert!(server.store().list_tasks(None, None).await.expect("list").is_empty());
}

#[tokio::test]
async fn a_post_op_accepts_no_query_parameters() {
    let server = Server::new();
    let body = args(json!({ "input": capture_input("Alpha") }));

    // Not even its own `Option` argument: a POST reads every argument from
    // its body.
    let reply = server.op("POST", "/api/tasks/capture?opArg[status]=inbox", &body).await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "opArg[status]");
    let reply = server.raw("POST", "/api/tasks/purge-done?x=1", &[], "").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "x");
}

#[tokio::test]
async fn custom_op_checks_run_in_order() {
    let server = Server::new();
    let json = "application/json";
    let bad_body = "[]";

    // 1. Routing: a method the route does not serve, whatever else is wrong.
    let reply = server.raw("PUT", "/api/tasks/capture?x=1", &[(header::ACCEPT, json)], bad_body).await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "POST");
    let reply = server.get("/api/tasks/capture").await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "POST");
    let reply = server.op("POST", "/api/tasks/summary/open", "").await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD");

    // 2. `Accept`, ahead of the body's media type.
    let headers = [(header::ACCEPT, json), (header::CONTENT_TYPE, json)];
    let reply = server.raw("POST", "/api/tasks/capture?x=1", &headers, bad_body).await;
    let error = reply.error(StatusCode::NOT_ACCEPTABLE, "not_acceptable");
    assert_eq!(error["source"], json!({ "header": "Accept" }));
    let reply = server.raw("GET", "/api/tasks/summary/open", &[(header::ACCEPT, json)], "").await;
    reply.error(StatusCode::NOT_ACCEPTABLE, "not_acceptable");

    // 3. The body's media type, ahead of the query.
    let reply = server.raw("POST", "/api/tasks/capture?x=1", &[(header::CONTENT_TYPE, json)], bad_body).await;
    let error = reply.error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
    assert_eq!(error["source"], json!({ "header": "Content-Type" }));

    // 5. The query, ahead of the body.
    let reply = server.op("POST", "/api/tasks/capture?x=1", bad_body).await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "x");

    // 7. The body.
    server.op("POST", "/api/tasks/capture", bad_body).await.error(StatusCode::BAD_REQUEST, "invalid_document");
}

/// `task::list_by_status(status)` has no `add_`/`remove_` partner, so it is
/// a custom `GET`, not a relationship: its plain `Vec` in `meta.result`,
/// unpaged although the module paginates.
async fn a_lone_list_is_a_custom_get_with_no_page(scope: Scope) {
    let server = Server::new();
    for (title, status) in [("Alpha", "open"), ("Beta", "done"), ("Gamma", "open"), ("Delta", "open")] {
        server.task(title, status).await;
    }
    let task = |id: &str, title: &str| {
        format!(
            r#"{{"id":"{id}","title":"{title}","status":"open","parent_id":null,"subtasks":[],"tags":[],"body":""}}"#
        )
    };

    // Three tasks, above `default_limit: 2`.
    let reply = server.get_in(scope, "/api/tasks/list-by-status/open").await;
    reply.ok_result(&format!("[{},{},{}]", task("alpha", "Alpha"), task("delta", "Delta"), task("gamma", "Gamma")));
    server.get_in(scope, "/api/tasks/list-by-status/archived").await.ok_result("[]");

    // It takes no page and no other parameter.
    for (query, parameter) in [("opArg[limit]=1", "opArg[limit]"), ("page[limit]=1", "page[limit]"), ("x=1", "x")] {
        let reply = server.get_in(scope, &format!("/api/tasks/list-by-status/open?{query}")).await;
        assert_eq!(reply.parameter("invalid_query_parameter"), parameter);
    }
    let reply = server.op_in(scope, "POST", "/api/tasks/list-by-status/open", "").await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD");

    // And `by_status` is no relationship of a task.
    for uri in ["/api/tasks/alpha/relationships/by_status", "/api/tasks/alpha/by_status"] {
        server.get_in(scope, uri).await.error(StatusCode::NOT_FOUND, "relationship_not_found");
    }
}
in_both_scopes!(a_lone_list_is_a_custom_get_with_no_page);

// ── Relationship routes (§9) ──

/// A relationship document: `{"data": data}`.
fn linkage(data: Value) -> Value {
    json!({ "data": data })
}

/// One resource identifier.
fn identifier(type_name: &str, id: &str) -> Value {
    json!({ "type": type_name, "id": id })
}

impl Server {
    /// A write to the router generated under `projects/:project_id`.
    async fn write_scoped(&self, method: &str, uri: &str, body: Value) -> Reply {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::ACCEPT, MEDIA_TYPE)
            .header(header::CONTENT_TYPE, MEDIA_TYPE)
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send_to(scoped_routes(), request).await
    }

    async fn task_parent(&self, id: &str) -> Option<String> {
        self.store().get_task(id).await.expect("task").parent_id
    }

    async fn task_tags(&self, id: &str) -> Vec<String> {
        self.store().get_task(id).await.expect("task").tags
    }

    /// The tag ids `api::note::list_tags` answers for the note `id`.
    fn note_tags(&self, id: &str) -> Vec<String> {
        self.state.note_tags.lock().expect("note tags lock").get(id).cloned().unwrap_or_default()
    }

    /// A request to `scope`'s router at the `/api/…` path `uri`, with exactly
    /// the given headers and body.
    async fn raw_in(
        &self,
        scope: Scope,
        method: &str,
        uri: &str,
        headers: &[(header::HeaderName, &str)],
        body: &str,
    ) -> Reply {
        let mut request = Request::builder().method(method).uri(scope.path(uri));
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        self.send_to(scope.router(), request.body(Body::from(body.to_string())).unwrap()).await
    }

    async fn get_in(&self, scope: Scope, uri: &str) -> Reply {
        self.raw_in(scope, "GET", uri, &[(header::ACCEPT, MEDIA_TYPE)], "").await
    }

    /// A request whose body is `body` as sent, with JSON:API media types.
    async fn op_in(&self, scope: Scope, method: &str, uri: &str, body: &str) -> Reply {
        self.raw_in(scope, method, uri, &[(header::ACCEPT, MEDIA_TYPE), (header::CONTENT_TYPE, MEDIA_TYPE)], body).await
    }

    async fn write_in(&self, scope: Scope, method: &str, uri: &str, body: Value) -> Reply {
        self.op_in(scope, method, uri, &body.to_string()).await
    }
}

async fn a_to_one_relationship_is_read_and_set_through_its_parent(scope: Scope) {
    let server = Server::new();
    server.task("Alpha", "open").await;
    server.task("Beta", "open").await;
    let parent = "/api/tasks/beta/relationships/parent";

    let reply = server.get_in(scope, parent).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    assert_eq!(reply.headers[header::CONTENT_TYPE], MEDIA_TYPE);
    assert_eq!(
        reply.raw,
        scope.links(concat!(
            r#"{"jsonapi":{"version":"1.1"},"links":{"self":"/api/tasks/beta/relationships/parent","#,
            r#""related":"/api/tasks/beta/parent"},"data":null}"#,
        ))
    );

    server.write_in(scope, "PATCH", parent, linkage(identifier("tasks", "alpha"))).await.no_content();
    assert_eq!(server.store().get_task("beta").await.expect("beta").parent_id.as_deref(), Some("alpha"));
    assert_eq!(server.get_in(scope, parent).await.body["data"], identifier("tasks", "alpha"));
    // The other end of the relation follows: `subtasks` is derived.
    assert_eq!(
        server.get_in(scope, "/api/tasks/alpha/relationships/subtasks").await.body["data"],
        json!([identifier("tasks", "beta")])
    );

    server.write_in(scope, "PATCH", parent, linkage(Value::Null)).await.no_content();
    assert_eq!(server.store().get_task("beta").await.expect("beta").parent_id, None);

    // The linked task must exist and be of the relationship's type.
    let reply = server.write_in(scope, "PATCH", parent, linkage(identifier("tasks", "nope"))).await;
    assert_eq!(reply.pointer(StatusCode::NOT_FOUND, "related_resource_not_found"), "/data");
    let reply = server.write_in(scope, "PATCH", parent, linkage(identifier("tags", "alpha"))).await;
    assert_eq!(reply.pointer(StatusCode::CONFLICT, "type_mismatch"), "/data");
    let reply = server.write_in(scope, "PATCH", parent, linkage(json!([identifier("tasks", "alpha")]))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/data");
    let reply = server.write_in(scope, "PATCH", parent, json!({})).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "");
    // A to-one has no members to add or remove.
    for method in ["POST", "DELETE"] {
        let reply = server.write_in(scope, method, parent, linkage(json!([identifier("tasks", "alpha")]))).await;
        let error = reply.error(StatusCode::FORBIDDEN, "relationship_update_unsupported");
        assert!(error.get("source").is_none(), "{}", reply.raw);
    }
    // The parent is read before the linked task.
    let reply = server
        .write_in(scope, "PATCH", "/api/tasks/nope/relationships/parent", linkage(identifier("tasks", "nope")))
        .await;
    reply.error(StatusCode::NOT_FOUND, "task_not_found");

    // A required to-one cannot be emptied.
    seed_sections(&server).await;
    let reply = server.write_in(scope, "PATCH", "/api/sections/intro/relationships/parent", linkage(Value::Null)).await;
    assert_eq!(reply.pointer(StatusCode::FORBIDDEN, "relationship_required"), "/data");
    let reply = server
        .write_in(scope, "PATCH", "/api/sections/intro/relationships/parent", linkage(identifier("sections", "usage")))
        .await;
    reply.no_content();
    assert_eq!(server.store().get_section("intro").await.expect("intro").parent_id, "usage");
}
in_both_scopes!(a_to_one_relationship_is_read_and_set_through_its_parent);

async fn a_many_to_many_relationship_adds_removes_and_replaces_members(scope: Scope) {
    let server = Server::new();
    server.task("Alpha", "open").await;
    for (id, title) in [("a", "A"), ("b", "B"), ("c", "C")] {
        server.tag(id, title).await;
    }
    let tags = "/api/tasks/alpha/relationships/tags";
    let one = |id: &str| linkage(json!([identifier("tags", id)]));

    // Adding a member twice changes nothing; `[]` is a no-op.
    for id in ["a", "b", "a"] {
        server.write_in(scope, "POST", tags, one(id)).await.no_content();
    }
    server.write_in(scope, "POST", tags, linkage(json!([]))).await.no_content();
    assert_eq!(server.task_tags("alpha").await, ["a", "b"]);
    let reply =
        server.write_in(scope, "POST", tags, linkage(json!([identifier("tags", "c"), identifier("tags", "a")]))).await;
    assert_eq!(reply.pointer(StatusCode::FORBIDDEN, "relationship_batch_unsupported"), "/data");
    let reply = server.write_in(scope, "POST", tags, one("nope")).await;
    assert_eq!(reply.pointer(StatusCode::NOT_FOUND, "related_resource_not_found"), "/data/0");

    // Removing an absent member, or one that names nothing, is a no-op.
    for id in ["a", "a", "nope"] {
        server.write_in(scope, "DELETE", tags, one(id)).await.no_content();
    }
    assert_eq!(server.task_tags("alpha").await, ["b"]);

    // A replacement collapses duplicates to their first occurrence.
    let replace = linkage(json!([identifier("tags", "c"), identifier("tags", "b"), identifier("tags", "c")]));
    server.write_in(scope, "PATCH", tags, replace).await.no_content();
    assert_eq!(server.task_tags("alpha").await, ["c", "b"]);
    let reply = server.get_in(scope, tags).await;
    assert_eq!(reply.body["data"], json!([identifier("tags", "c"), identifier("tags", "b")]));
    assert_eq!(
        reply.body["links"],
        json!({ "self": scope.path(tags), "related": scope.path("/api/tasks/alpha/tags") })
    );
    assert!(reply.body.get("meta").is_none(), "a field relationship is never paginated: {}", reply.raw);
    server.write_in(scope, "PATCH", tags, linkage(json!([]))).await.no_content();
    assert!(server.task_tags("alpha").await.is_empty());
}
in_both_scopes!(a_many_to_many_relationship_adds_removes_and_replaces_members);

async fn a_has_many_relationship_writes_the_childrens_foreign_keys(scope: Scope) {
    let server = Server::new();
    for title in ["Alpha", "Beta", "Gamma"] {
        server.task(title, "open").await;
    }
    let subtasks = "/api/tasks/alpha/relationships/subtasks";

    server.write_in(scope, "POST", subtasks, linkage(json!([identifier("tasks", "beta")]))).await.no_content();
    assert_eq!(server.task_parent("beta").await.as_deref(), Some("alpha"));
    server.write_in(scope, "PATCH", subtasks, linkage(json!([identifier("tasks", "gamma")]))).await.no_content();
    assert_eq!((server.task_parent("beta").await, server.task_parent("gamma").await.as_deref()), (None, Some("alpha")));
    server.write_in(scope, "DELETE", subtasks, linkage(json!([identifier("tasks", "gamma")]))).await.no_content();
    assert_eq!(server.task_parent("gamma").await, None);

    // A child whose foreign key is not an `Option` cannot be dropped.
    seed_sections(&server).await;
    let children = "/api/sections/usage/relationships/children";
    let reply = server.write_in(scope, "PATCH", children, linkage(json!([identifier("sections", "install")]))).await;
    let error = reply.error(StatusCode::FORBIDDEN, "section_parent_required");
    assert!(error.get("source").is_none(), "{}", reply.raw);
    let reply = server.write_in(scope, "DELETE", children, linkage(json!([identifier("sections", "cli")]))).await;
    reply.error(StatusCode::FORBIDDEN, "section_parent_required");
    // Adding one moves it from its previous parent.
    server.write_in(scope, "POST", children, linkage(json!([identifier("sections", "intro")]))).await.no_content();
    assert_eq!(server.store().get_section("intro").await.expect("intro").parent_id, "usage");
}
in_both_scopes!(a_has_many_relationship_writes_the_childrens_foreign_keys);

async fn a_junction_relationship_calls_its_ops(scope: Scope) {
    let server = Server::new();
    server.task("Alpha", "open").await;
    for (id, title) in [("a", "A"), ("b", "B"), ("c", "C")] {
        server.tag(id, title).await;
    }
    let labels = "/api/tasks/alpha/relationships/labels";
    let one = |id: &str| linkage(json!([identifier("tags", id)]));

    // A member added twice is added once: the membership read answers it.
    for id in ["a", "b", "c", "a"] {
        server.write_in(scope, "POST", labels, one(id)).await.no_content();
    }
    assert_eq!(server.task_tags("alpha").await, ["a", "b", "c"]);

    // Paginated in memory: `default_limit: 2`, `max_limit: 3`.
    let reply = server.get_in(scope, labels).await;
    assert_eq!(
        reply.raw,
        scope.links(concat!(
            r#"{"jsonapi":{"version":"1.1"},"links":{"#,
            r#""self":"/api/tasks/alpha/relationships/labels?page%5Boffset%5D=0&page%5Blimit%5D=2","#,
            r#""related":"/api/tasks/alpha/labels","#,
            r#""first":"/api/tasks/alpha/relationships/labels?page%5Boffset%5D=0&page%5Blimit%5D=2","#,
            r#""prev":null,"#,
            r#""next":"/api/tasks/alpha/relationships/labels?page%5Boffset%5D=2&page%5Blimit%5D=2","#,
            r#""last":"/api/tasks/alpha/relationships/labels?page%5Boffset%5D=2&page%5Blimit%5D=2"},"#,
            r#""meta":{"total":3,"limit":2,"offset":0},"#,
            r#""data":[{"type":"tags","id":"a"},{"type":"tags","id":"b"}]}"#,
        ))
    );
    let reply = server.get_in(scope, &format!("{labels}?page[offset]=2&page[limit]=50")).await;
    assert_eq!(reply.body["data"], json!([identifier("tags", "c")]));
    assert_eq!(reply.body["meta"], json!({ "total": 3, "limit": 3, "offset": 2 }));
    assert_eq!(
        server.get_in(scope, &format!("{labels}?page[limit]=0")).await.parameter("invalid_query_parameter"),
        "page[limit]"
    );
    assert_eq!(
        server.get_in(scope, &format!("{labels}?opArg[limit]=1")).await.parameter("invalid_query_parameter"),
        "opArg[limit]"
    );

    // A member removed twice is removed once; a target is never checked.
    for id in ["b", "b", "nope"] {
        server.write_in(scope, "DELETE", labels, one(id)).await.no_content();
    }
    assert_eq!(server.task_tags("alpha").await, ["a", "c"]);

    // No junction op replaces a set.
    let reply = server.write_in(scope, "PATCH", labels, one("a")).await;
    let error = reply.error(StatusCode::FORBIDDEN, "relationship_update_unsupported");
    assert!(error.get("source").is_none(), "{}", reply.raw);
    let reply = server.write_in(scope, "POST", labels, one("nope")).await;
    assert_eq!(reply.pointer(StatusCode::NOT_FOUND, "related_resource_not_found"), "/data/0");
    let reply = server.write_in(scope, "POST", labels, json!({})).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "");
    server.get_in(scope, "/api/tasks/nope/relationships/labels").await.error(StatusCode::NOT_FOUND, "task_not_found");
}
in_both_scopes!(a_junction_relationship_calls_its_ops);

async fn related_links_answer_the_related_resources(scope: Scope) {
    let server = Server::new();
    server.task("Alpha", "open").await;
    server.task("Beta", "open").await;
    for (id, title) in [("a", "A"), ("b", "B"), ("c", "C")] {
        server.tag(id, title).await;
    }
    server
        .write_in(scope, "PATCH", "/api/tasks/beta/relationships/parent", linkage(identifier("tasks", "alpha")))
        .await
        .no_content();
    let all = linkage(json!([identifier("tags", "a"), identifier("tags", "b"), identifier("tags", "c")]));
    server.write_in(scope, "PATCH", "/api/tasks/alpha/relationships/tags", all).await.no_content();

    // To-one: the resource, or `null`.
    let reply = server.get_in(scope, "/api/tasks/beta/parent").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    assert_eq!(reply.body["links"], json!({ "self": scope.path("/api/tasks/beta/parent") }));
    assert_eq!(reply.body["data"], server.get_in(scope, "/api/tasks/alpha").await.body["data"]);
    assert_eq!(
        server.get_in(scope, "/api/tasks/alpha/parent").await.raw,
        scope.links(r#"{"jsonapi":{"version":"1.1"},"links":{"self":"/api/tasks/alpha/parent"},"data":null}"#)
    );

    // To-many, in linkage order; a dangling id is skipped.
    server.store().delete_tag("b").await.expect("delete b");
    let reply = server.get_in(scope, "/api/tasks/alpha/tags").await;
    assert_eq!(ids(&reply.body), ["a", "c"]);
    assert_eq!(reply.body["data"][0]["links"]["self"], scope.path("/api/tags/a"));
    assert!(reply.body.get("meta").is_none(), "{}", reply.raw);
    assert_eq!(ids(&server.get_in(scope, "/api/tasks/alpha/subtasks").await.body), ["beta"]);

    // A junction relationship: `list_labels`'s entities, paged.
    server
        .write_in(scope, "DELETE", "/api/tasks/alpha/relationships/tags", linkage(json!([identifier("tags", "b")])))
        .await
        .no_content();
    let reply = server.get_in(scope, "/api/tasks/alpha/labels?page[limit]=1&page[offset]=1").await;
    assert_eq!(ids(&reply.body), ["c"]);
    assert_eq!(reply.body["data"][0]["attributes"], json!({ "title": "C" }));
    assert_eq!(reply.body["meta"], json!({ "total": 2, "limit": 1, "offset": 1 }));
    assert_eq!(reply.body["links"]["self"], scope.path("/api/tasks/alpha/labels?page%5Boffset%5D=1&page%5Blimit%5D=1"));
    assert!(reply.body["links"].get("related").is_none(), "{}", reply.raw);

    server.get_in(scope, "/api/tasks/nope/tags").await.error(StatusCode::NOT_FOUND, "task_not_found");
    // Only a junction relationship pages.
    assert_eq!(
        server.get_in(scope, "/api/tasks/alpha/tags?page[limit]=1").await.parameter("invalid_query_parameter"),
        "page[limit]"
    );
}
in_both_scopes!(related_links_answer_the_related_resources);

async fn relationship_routes_check_in_the_contract_order(scope: Scope) {
    let server = Server::new();
    server.task("Alpha", "open").await;

    // Step 4: an unknown `{rel}` is a 404 on both routes and every method,
    // ahead of the query, the body and the parent.
    for uri in
        ["/api/tasks/alpha/relationships/nope", "/api/tasks/alpha/nope", "/api/tasks/nope/relationships/nope?x=1"]
    {
        let error = server.get_in(scope, uri).await.error(StatusCode::NOT_FOUND, "relationship_not_found");
        assert!(error.get("source").is_none());
    }
    for method in ["PATCH", "POST", "DELETE"] {
        let reply = server.op_in(scope, method, "/api/tasks/nope/relationships/nope?x=1", "not json").await;
        reply.error(StatusCode::NOT_FOUND, "relationship_not_found");
    }
    // Undecodable, it names no relationship either.
    server
        .get_in(scope, "/api/tasks/alpha/relationships/%FF")
        .await
        .error(StatusCode::NOT_FOUND, "relationship_not_found");

    // Step 3 precedes it: the body's media type.
    let reply = server
        .raw_in(
            scope,
            "PATCH",
            "/api/tasks/alpha/relationships/nope",
            &[(header::CONTENT_TYPE, "application/json")],
            "{}",
        )
        .await;
    reply.error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");

    // Step 5, then 6, then 7, then 8.
    let reply = server.op_in(scope, "PATCH", "/api/tasks/alpha/relationships/labels?x=1", "not json").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "x");
    server
        .op_in(scope, "PATCH", "/api/tasks/alpha/relationships/labels", "not json")
        .await
        .error(StatusCode::FORBIDDEN, "relationship_update_unsupported");
    server
        .op_in(scope, "POST", "/api/tasks/nope/relationships/tags", "not json")
        .await
        .error(StatusCode::BAD_REQUEST, "invalid_document");
    let reply = server
        .op_in(scope, "POST", "/api/tasks/nope/relationships/tags", r#"{"data":[{"type":"tags","id":"nope"}]}"#)
        .await;
    reply.error(StatusCode::NOT_FOUND, "task_not_found");
    // A relationship route takes no `include` or `sort`.
    assert_eq!(
        server
            .get_in(scope, "/api/tasks/alpha/relationships/parent?include=parent")
            .await
            .parameter("invalid_query_parameter"),
        "include"
    );

    // Step 1: routing happens before `{rel}` is read.
    for uri in ["/api/tasks/alpha/relationships/parent", "/api/tasks/alpha/relationships/nope"] {
        let reply = server.op_in(scope, "PUT", uri, "{}").await;
        reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
        assert_eq!(reply.headers[header::ALLOW], "GET, HEAD, PATCH, POST, DELETE");
    }
    for method in ["PUT", "POST", "PATCH", "DELETE"] {
        let reply = server.op_in(scope, method, "/api/tasks/alpha/parent", "{}").await;
        reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
        assert_eq!(reply.headers[header::ALLOW], "GET, HEAD");
    }
}
in_both_scopes!(relationship_routes_check_in_the_contract_order);

async fn a_junction_relationship_cannot_be_written_in_a_resource_document(scope: Scope) {
    let server = Server::new();
    server.task("Alpha", "open").await;
    server.tag("a", "A").await;
    let labelled = json!({ "labels": linkage(json!([identifier("tags", "a")])) });

    let reply = server
        .write_in(scope,
            "POST",
            "/api/tasks",
            json!({ "data": { "type": "tasks", "attributes": { "title": "Beta", "status": "open", "body": "" }, "relationships": labelled } }),
        )
        .await;
    assert_eq!(reply.pointer(StatusCode::FORBIDDEN, "relationship_update_unsupported"), "/data/relationships/labels");
    let reply = server
        .write_in(
            scope,
            "PATCH",
            "/api/tasks/alpha",
            json!({ "data": { "type": "tasks", "id": "alpha", "relationships": labelled } }),
        )
        .await;
    assert_eq!(reply.pointer(StatusCode::FORBIDDEN, "relationship_update_unsupported"), "/data/relationships/labels");
    // It is a relationship the type has: an unknown name is reported first.
    let reply = server
        .write_in(scope,
            "PATCH",
            "/api/tasks/alpha",
            json!({ "data": { "type": "tasks", "id": "alpha", "relationships": { "labels": labelled["labels"], "zzz": {} } } }),
        )
        .await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "unknown_relationship"), "/data/relationships/zzz");
    assert!(server.task_tags("alpha").await.is_empty());

    // So is a note's, whose list answers ids.
    server.note("Alpha").await;
    let tagged = json!({ "data": { "type": "notes", "id": "alpha", "relationships": { "tags": labelled["labels"] } } });
    let reply = server.write_in(scope, "PATCH", "/api/notes/alpha", tagged).await;
    assert_eq!(reply.pointer(StatusCode::FORBIDDEN, "relationship_update_unsupported"), "/data/relationships/tags");
    assert!(server.note_tags("alpha").is_empty());
}
in_both_scopes!(a_junction_relationship_cannot_be_written_in_a_resource_document);

// `note::list_tags` answers tag ids from `AppState`, so `notes` has one
// relationship, `tags`, with no field behind it.

async fn a_junction_relationship_of_ids_pages_linkage_read_from_its_strings(scope: Scope) {
    let server = Server::new();
    server.note("Alpha").await;
    for (id, title) in [("a", "A"), ("b", "B"), ("c", "C")] {
        server.tag(id, title).await;
    }
    let tags = "/api/notes/alpha/relationships/tags";

    // `add_tag` appends whatever the note has, so `a` is listed once only
    // because the endpoint reads the membership first.
    for id in ["a", "b", "c", "a"] {
        server.write_in(scope, "POST", tags, linkage(json!([identifier("tags", id)]))).await.no_content();
    }
    assert_eq!(server.note_tags("alpha"), ["a", "b", "c"]);

    // Paginated in memory, with `related` beside the page links.
    let reply = server.get_in(scope, tags).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    assert_eq!(reply.headers[header::CONTENT_TYPE], MEDIA_TYPE);
    assert_eq!(
        reply.raw,
        scope.links(concat!(
            r#"{"jsonapi":{"version":"1.1"},"links":{"#,
            r#""self":"/api/notes/alpha/relationships/tags?page%5Boffset%5D=0&page%5Blimit%5D=2","#,
            r#""related":"/api/notes/alpha/tags","#,
            r#""first":"/api/notes/alpha/relationships/tags?page%5Boffset%5D=0&page%5Blimit%5D=2","#,
            r#""prev":null,"#,
            r#""next":"/api/notes/alpha/relationships/tags?page%5Boffset%5D=2&page%5Blimit%5D=2","#,
            r#""last":"/api/notes/alpha/relationships/tags?page%5Boffset%5D=2&page%5Blimit%5D=2"},"#,
            r#""meta":{"total":3,"limit":2,"offset":0},"#,
            r#""data":[{"type":"tags","id":"a"},{"type":"tags","id":"b"}]}"#,
        ))
    );
    let reply = server.get_in(scope, &format!("{tags}?page[offset]=2&page[limit]=50")).await;
    assert_eq!(reply.body["data"], json!([identifier("tags", "c")]));
    assert_eq!(reply.body["meta"], json!({ "total": 3, "limit": 3, "offset": 2 }));
    assert_eq!(reply.body["links"]["next"], Value::Null);

    // The linkage is the strings themselves: a tag deleted since is still a
    // member.
    server.store().delete_tag("c").await.expect("delete c");
    let reply = server.get_in(scope, &format!("{tags}?page[limit]=3")).await;
    assert_eq!(reply.body["data"], json!([identifier("tags", "a"), identifier("tags", "b"), identifier("tags", "c")]));

    // `page[limit]=0` is no page (§7.2), and the route takes no `opArg`.
    let reply = server.get_in(scope, &format!("{tags}?page[limit]=0")).await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "page[limit]");
    let reply = server.get_in(scope, &format!("{tags}?opArg[limit]=1")).await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "opArg[limit]");

    // A note with no tags has empty linkage, still a page.
    server.note("Beta").await;
    let reply = server.get_in(scope, "/api/notes/beta/relationships/tags").await;
    assert_eq!(reply.body["data"], json!([]));
    assert_eq!(reply.body["meta"], json!({ "total": 0, "limit": 2, "offset": 0 }));
    server.get_in(scope, "/api/notes/nope/relationships/tags").await.error(StatusCode::NOT_FOUND, "note_not_found");
}
in_both_scopes!(a_junction_relationship_of_ids_pages_linkage_read_from_its_strings);

async fn a_junction_relationship_of_ids_adds_and_removes_by_membership(scope: Scope) {
    let server = Server::new();
    server.note("Alpha").await;
    server.tag("a", "A").await;
    server.tag("b", "B").await;
    let tags = "/api/notes/alpha/relationships/tags";
    let one = |id: &str| linkage(json!([identifier("tags", id)]));

    // A target must exist to be added; `[]` adds nothing; two identifiers
    // are refused.
    let reply = server.write_in(scope, "POST", tags, one("nope")).await;
    assert_eq!(reply.pointer(StatusCode::NOT_FOUND, "related_resource_not_found"), "/data/0");
    server.write_in(scope, "POST", tags, linkage(json!([]))).await.no_content();
    let reply =
        server.write_in(scope, "POST", tags, linkage(json!([identifier("tags", "a"), identifier("tags", "b")]))).await;
    assert_eq!(reply.pointer(StatusCode::FORBIDDEN, "relationship_batch_unsupported"), "/data");
    assert!(server.note_tags("alpha").is_empty());

    for id in ["a", "b"] {
        server.write_in(scope, "POST", tags, one(id)).await.no_content();
    }
    // Removing a non-member is a no-op whether or not its target exists.
    for id in ["nope", "zzz"] {
        server.write_in(scope, "DELETE", tags, one(id)).await.no_content();
    }
    server.tag("zzz", "Z").await;
    server.write_in(scope, "DELETE", tags, one("zzz")).await.no_content();
    server.write_in(scope, "DELETE", tags, linkage(json!([]))).await.no_content();
    assert_eq!(server.note_tags("alpha"), ["a", "b"]);

    // A DELETE never reads the target, so a member whose tag was deleted is
    // still removed.
    server.store().delete_tag("a").await.expect("delete a");
    server.write_in(scope, "DELETE", tags, one("a")).await.no_content();
    assert_eq!(server.note_tags("alpha"), ["b"]);
    server.write_in(scope, "DELETE", tags, one("a")).await.no_content();
    assert_eq!(server.note_tags("alpha"), ["b"]);

    // The parent is read first (§13.2 step 8): a missing note with a
    // missing tag is the note's 404.
    for method in ["POST", "DELETE"] {
        let reply = server.write_in(scope, method, "/api/notes/nope/relationships/tags", one("b")).await;
        reply.error(StatusCode::NOT_FOUND, "note_not_found");
    }
    let reply = server.write_in(scope, "POST", "/api/notes/nope/relationships/tags", one("nope")).await;
    reply.error(StatusCode::NOT_FOUND, "note_not_found");
    assert!(server.note_tags("nope").is_empty());
}
in_both_scopes!(a_junction_relationship_of_ids_adds_and_removes_by_membership);

async fn the_related_link_of_a_junction_of_ids_reads_each_target(scope: Scope) {
    let server = Server::new();
    server.note("Alpha").await;
    for (id, title) in [("a", "A"), ("b", "B"), ("c", "C")] {
        server.tag(id, title).await;
        server
            .write_in(scope, "POST", "/api/notes/alpha/relationships/tags", linkage(json!([identifier("tags", id)])))
            .await
            .no_content();
    }
    server.store().delete_tag("b").await.expect("delete b");
    let page = |offset: u32, limit: u32| {
        scope.path(&format!("/api/notes/alpha/tags?page%5Boffset%5D={offset}&page%5Blimit%5D={limit}"))
    };

    // Each id is read through the tag module's `get_by_id`, and is the
    // target's own resource object; the dangling `b` is skipped, though the
    // page counts it.
    let reply = server.get_in(scope, "/api/notes/alpha/tags?page[limit]=3").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    assert_eq!(ids(&reply.body), ["a", "c"]);
    assert_eq!(
        reply.body["data"][1],
        json!({ "type": "tags", "id": "c", "attributes": { "title": "C" }, "links": { "self": scope.path("/api/tags/c") } })
    );
    assert_eq!(reply.body["meta"], json!({ "total": 3, "limit": 3, "offset": 0 }));
    assert_eq!(
        reply.body["links"],
        json!({ "self": page(0, 3), "first": page(0, 3), "prev": null, "next": null, "last": page(0, 3) })
    );

    // The default page is the first two ids, one of them dangling.
    let reply = server.get_in(scope, "/api/notes/alpha/tags").await;
    assert_eq!(ids(&reply.body), ["a"]);
    assert_eq!(reply.body["meta"], json!({ "total": 3, "limit": 2, "offset": 0 }));
    assert_eq!(reply.body["links"]["next"], page(2, 2));

    let reply = server.get_in(scope, "/api/notes/alpha/tags?page[limit]=0").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "page[limit]");
    let reply = server.get_in(scope, "/api/notes/alpha/tags?include=tags").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "include");
    server.get_in(scope, "/api/notes/nope/tags").await.error(StatusCode::NOT_FOUND, "note_not_found");
}
in_both_scopes!(the_related_link_of_a_junction_of_ids_reads_each_target);

/// Every relationship of a type that serves relationship routes links
/// both routes; a junction relationship has the links alone.
async fn resource_objects_link_every_relationship(scope: Scope) {
    let server = Server::new();
    server.note("Alpha").await;
    server.task("Alpha", "open").await;
    server.tag("a", "A").await;
    seed_sections(&server).await;
    let links = |base: &str, rel: &str| json!({ "self": scope.path(&format!("{base}/relationships/{rel}")), "related": scope.path(&format!("{base}/{rel}")) });

    let note = server.get_in(scope, "/api/notes/alpha").await.body;
    assert_eq!(note["data"]["relationships"], json!({ "tags": { "links": links("/api/notes/alpha", "tags") } }));
    let notes = server.get_in(scope, "/api/notes").await.body;
    assert_eq!(notes["data"][0]["relationships"], note["data"]["relationships"]);

    let section = server.get_in(scope, "/api/sections/usage").await.body;
    assert_eq!(
        section["data"]["relationships"],
        json!({
            "parent": { "links": links("/api/sections/usage", "parent"), "data": identifier("sections", "root") },
            "children": { "links": links("/api/sections/usage", "children"),
                          "data": [identifier("sections", "cli"), identifier("sections", "install")] },
        })
    );

    let task = server.get_in(scope, "/api/tasks/alpha").await;
    // Relation fields in declaration order, then the junction relationship.
    let at =
        |rel: &str| task.raw.find(&format!(r#""{rel}":{{"links""#)).unwrap_or_else(|| panic!("{rel}: {}", task.raw));
    assert!(at("parent") < at("subtasks") && at("subtasks") < at("tags") && at("tags") < at("labels"), "{}", task.raw);
    let relationships = &task.body["data"]["relationships"];
    assert_eq!(relationships["parent"], json!({ "links": links("/api/tasks/alpha", "parent"), "data": null }));
    assert_eq!(relationships["labels"], json!({ "links": links("/api/tasks/alpha", "labels") }));

    // A type with no relationship has no `relationships` member.
    let tag = server.get_in(scope, "/api/tags/a").await.body;
    assert!(tag["data"].get("relationships").is_none(), "{tag}");
}
in_both_scopes!(resource_objects_link_every_relationship);

/// Where an error document's `source` points, if anywhere.
#[derive(Debug, Clone, Copy)]
enum Source {
    Nothing,
    Pointer(&'static str),
    Parameter(&'static str),
    Header(&'static str),
}

impl Source {
    fn json(self) -> Value {
        match self {
            Source::Nothing => Value::Null,
            Source::Pointer(p) => json!({ "pointer": p }),
            Source::Parameter(p) => json!({ "parameter": p }),
            Source::Header(h) => json!({ "header": h }),
        }
    }
}

/// Requests sent with the same headers, each answering the same status and
/// code: `(method, uri, body, source)`.
type ErrorGroup<'a> =
    (&'a [(header::HeaderName, &'a str)], StatusCode, &'a str, Vec<(&'a str, &'a str, &'a str, Source)>);

/// One request per row of the §9.2 error table, on relation fields and
/// junction relationships, each answering its status, code and source;
/// none of them writes anything.
async fn every_relationship_error_row_answers_as_the_contract_says(scope: Scope) {
    use Source::{Header, Nothing, Parameter, Pointer};

    let server = Server::new();
    server.note("Alpha").await;
    server.task("Alpha", "open").await;
    server.task("Beta", "open").await;
    server.tag("a", "A").await;
    seed_sections(&server).await;
    let json = "application/json";
    let jsonapi = [(header::ACCEPT, MEDIA_TYPE), (header::CONTENT_TYPE, MEDIA_TYPE)];
    let accept_json = [(header::ACCEPT, json), (header::CONTENT_TYPE, MEDIA_TYPE)];
    let content_json = [(header::ACCEPT, MEDIA_TYPE), (header::CONTENT_TYPE, json)];
    let doc = |data: Value| linkage(data).to_string();
    let tag_a = &doc(json!([identifier("tags", "a")]));
    let tag_nope = &doc(json!([identifier("tags", "nope")]));
    let tags_a_b = &doc(json!([identifier("tags", "a"), identifier("tags", "b")]));
    let tags_a_nope = &doc(json!([identifier("tags", "a"), identifier("tags", "nope")]));
    let tag_7 = &doc(json!([identifier("tags", "a"), { "type": "tags", "id": 7 }]));
    let tag_untyped = &doc(json!([{ "id": "a" }]));
    let tag_one = &doc(identifier("tags", "a"));
    let note_a = &doc(json!([identifier("notes", "a")]));
    let task_alpha = &doc(identifier("tasks", "alpha"));
    let task_untyped = &doc(json!({ "type": "tasks" }));
    let tasks_alpha = &doc(json!([identifier("tasks", "alpha")]));
    let tasks_alpha_beta = &doc(json!([identifier("tasks", "alpha"), identifier("tasks", "beta")]));
    let sections_usage = &doc(json!([identifier("sections", "usage")]));
    let section_nope = &doc(identifier("sections", "nope"));
    let section_cli = &doc(json!([identifier("sections", "cli")]));
    let section_install = &doc(json!([identifier("sections", "install")]));
    let null = &doc(Value::Null);
    let empty = &doc(json!([]));
    let no_data = r#"{"meta":{}}"#;

    let groups: Vec<ErrorGroup> = vec![
        // Step 1: routing.
        (
            &jsonapi,
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            vec![
                ("PUT", "/api/notes/alpha/relationships/tags", tag_a, Nothing),
                ("PUT", "/api/notes/alpha/relationships/nope", tag_a, Nothing),
                ("PATCH", "/api/notes/alpha/tags", tag_a, Nothing),
                ("POST", "/api/sections/usage/children", sections_usage, Nothing),
            ],
        ),
        // Step 2: `Accept`.
        (
            &accept_json,
            StatusCode::NOT_ACCEPTABLE,
            "not_acceptable",
            vec![
                ("GET", "/api/notes/alpha/relationships/tags", "", Header("Accept")),
                ("GET", "/api/tasks/alpha/parent", "", Header("Accept")),
                ("POST", "/api/notes/alpha/relationships/tags", tag_a, Header("Accept")),
            ],
        ),
        // Step 3: the body's media type, on every write method.
        (
            &content_json,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            vec![
                ("PATCH", "/api/tasks/beta/relationships/parent", task_alpha, Header("Content-Type")),
                ("POST", "/api/notes/alpha/relationships/tags", tag_a, Header("Content-Type")),
                ("DELETE", "/api/notes/alpha/relationships/tags", tag_a, Header("Content-Type")),
                ("DELETE", "/api/tasks/alpha/relationships/tags", tag_a, Header("Content-Type")),
            ],
        ),
        // Step 4: `{rel}`: unknown, or not UTF-8 once decoded.
        (
            &jsonapi,
            StatusCode::NOT_FOUND,
            "relationship_not_found",
            vec![
                ("GET", "/api/notes/alpha/relationships/nope", "", Nothing),
                ("GET", "/api/notes/alpha/nope", "", Nothing),
                ("GET", "/api/notes/alpha/relationships/%E2%9C%93", "", Nothing),
                ("GET", "/api/notes/alpha/%C3%28", "", Nothing),
                ("PATCH", "/api/notes/alpha/relationships/%FF%FE", tag_a, Nothing),
                ("POST", "/api/sections/usage/relationships/kids", sections_usage, Nothing),
                ("DELETE", "/api/notes/alpha/relationships/labels", tag_a, Nothing),
            ],
        ),
        // Step 5: the query.
        (
            &jsonapi,
            StatusCode::BAD_REQUEST,
            "invalid_query_parameter",
            vec![
                ("GET", "/api/notes/alpha/relationships/tags?x=1", "", Parameter("x")),
                ("GET", "/api/tasks/alpha/subtasks?fields[tasks]=title", "", Parameter("fields[tasks]")),
                ("GET", "/api/sections/usage/relationships/children?sort=id", "", Parameter("sort")),
                ("PATCH", "/api/tasks/beta/relationships/parent?x=1", task_alpha, Parameter("x")),
                ("POST", "/api/notes/alpha/relationships/tags?page[limit]=1", tag_a, Parameter("page[limit]")),
                ("DELETE", "/api/notes/alpha/relationships/tags?include=tags", tag_a, Parameter("include")),
            ],
        ),
        // Step 6: a write the relationship does not support.
        (
            &jsonapi,
            StatusCode::FORBIDDEN,
            "relationship_update_unsupported",
            vec![
                ("PATCH", "/api/notes/alpha/relationships/tags", tag_a, Nothing),
                ("POST", "/api/sections/intro/relationships/parent", sections_usage, Nothing),
                ("DELETE", "/api/tasks/beta/relationships/parent", tasks_alpha, Nothing),
            ],
        ),
        // Step 7: the body.
        (
            &jsonapi,
            StatusCode::BAD_REQUEST,
            "invalid_document",
            vec![
                ("POST", "/api/notes/alpha/relationships/tags", "not json", Nothing),
                ("PATCH", "/api/tasks/beta/relationships/parent", "[]", Nothing),
                ("DELETE", "/api/notes/alpha/relationships/tags", "{}", Pointer("")),
                ("PATCH", "/api/tasks/alpha/relationships/tags", no_data, Pointer("")),
                ("POST", "/api/notes/alpha/relationships/tags", tag_one, Pointer("/data")),
                ("PATCH", "/api/sections/intro/relationships/parent", empty, Pointer("/data")),
                ("PATCH", "/api/tasks/alpha/relationships/subtasks", null, Pointer("/data")),
                ("PATCH", "/api/tasks/beta/relationships/parent", task_untyped, Pointer("/data")),
                ("POST", "/api/notes/alpha/relationships/tags", tag_untyped, Pointer("/data/0")),
                ("PATCH", "/api/tasks/alpha/relationships/tags", tag_7, Pointer("/data/1")),
            ],
        ),
        (
            &jsonapi,
            StatusCode::FORBIDDEN,
            "relationship_batch_unsupported",
            vec![
                ("POST", "/api/notes/alpha/relationships/tags", tags_a_b, Pointer("/data")),
                ("DELETE", "/api/tasks/alpha/relationships/subtasks", tasks_alpha_beta, Pointer("/data")),
            ],
        ),
        (
            &jsonapi,
            StatusCode::FORBIDDEN,
            "relationship_required",
            vec![("PATCH", "/api/sections/intro/relationships/parent", null, Pointer("/data"))],
        ),
        (
            &jsonapi,
            StatusCode::CONFLICT,
            "type_mismatch",
            vec![
                ("POST", "/api/notes/alpha/relationships/tags", note_a, Pointer("/data/0")),
                ("PATCH", "/api/tasks/beta/relationships/parent", tag_one, Pointer("/data")),
            ],
        ),
        // Step 8: the parent, then each linked resource.
        (
            &jsonapi,
            StatusCode::NOT_FOUND,
            "section_not_found",
            vec![("GET", "/api/sections/nope/relationships/children", "", Nothing)],
        ),
        (&jsonapi, StatusCode::NOT_FOUND, "note_not_found", vec![("GET", "/api/notes/nope/tags", "", Nothing)]),
        (
            &jsonapi,
            StatusCode::NOT_FOUND,
            "task_not_found",
            vec![("DELETE", "/api/tasks/nope/relationships/tags", tag_a, Nothing)],
        ),
        (
            &jsonapi,
            StatusCode::NOT_FOUND,
            "related_resource_not_found",
            vec![
                ("POST", "/api/notes/alpha/relationships/tags", tag_nope, Pointer("/data/0")),
                ("PATCH", "/api/tasks/beta/relationships/tags", tags_a_nope, Pointer("/data/1")),
                ("PATCH", "/api/sections/intro/relationships/parent", section_nope, Pointer("/data")),
            ],
        ),
        // Step 9: the store refuses to orphan a section.
        (
            &jsonapi,
            StatusCode::FORBIDDEN,
            "section_parent_required",
            vec![
                ("PATCH", "/api/sections/usage/relationships/children", section_cli, Nothing),
                ("DELETE", "/api/sections/usage/relationships/children", section_install, Nothing),
            ],
        ),
    ];
    for (headers, status, code, requests) in groups {
        for (method, uri, body, source) in requests {
            let reply = server.raw_in(scope, method, uri, headers, body).await;
            assert_eq!(reply.status, status, "{method} {uri}: {}", reply.raw);
            let error = reply.error(status, code);
            assert_eq!(error.get("source").cloned().unwrap_or(Value::Null), source.json(), "{method} {uri}");
        }
    }

    // Nothing was written.
    assert!(server.note_tags("alpha").is_empty());
    assert!(server.task_tags("alpha").await.is_empty() && server.task_tags("beta").await.is_empty());
    assert_eq!(server.task_parent("beta").await, None);
    assert_eq!(server.store().get_section("intro").await.expect("intro").parent_id, "root");
    for child in ["cli", "install"] {
        assert_eq!(server.store().get_section(child).await.expect("child").parent_id, "usage");
    }
}
in_both_scopes!(every_relationship_error_row_answers_as_the_contract_says);

/// Routing answers `405` before `{rel}` is read, so both routes name their
/// methods whatever `{rel}` is.
async fn relationship_routes_allow_their_methods(scope: Scope) {
    let server = Server::new();
    server.note("Alpha").await;
    for uri in [
        "/api/notes/alpha/relationships/tags",
        "/api/notes/alpha/relationships/nope",
        "/api/notes/nope/relationships/tags",
        "/api/sections/usage/relationships/children",
    ] {
        let reply = server.op_in(scope, "PUT", uri, "{}").await;
        reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
        assert_eq!(reply.headers[header::ALLOW], "GET, HEAD, PATCH, POST, DELETE", "{uri}");
    }
    for method in ["PUT", "PATCH", "POST", "DELETE"] {
        for uri in ["/api/notes/alpha/tags", "/api/notes/alpha/nope"] {
            let reply = server.op_in(scope, method, uri, "{}").await;
            reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
            assert_eq!(reply.headers[header::ALLOW], "GET, HEAD", "{method} {uri}");
        }
    }
    // `HEAD` is served wherever `GET` is.
    let reply = server.raw_in(scope, "HEAD", "/api/notes/alpha/relationships/tags", &[], "").await;
    assert_eq!(reply.status, StatusCode::OK);
    // A percent-encoded name decodes to the relationship it spells.
    let reply = server.get_in(scope, "/api/notes/alpha/relationships/t%61gs").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    assert_eq!(reply.body["data"], json!([]));
}
in_both_scopes!(relationship_routes_allow_their_methods);

/// §13.2 on a junction relationship of ids: each step's failure beats
/// every later one.
async fn junction_relationship_checks_run_in_the_contract_order(scope: Scope) {
    let server = Server::new();
    server.note("Alpha").await;
    let json = "application/json";
    let bad_accept = [(header::ACCEPT, json), (header::CONTENT_TYPE, json)];
    let bad_content = [(header::ACCEPT, MEDIA_TYPE), (header::CONTENT_TYPE, json)];

    // 1 beats 2: a method the route does not serve, whatever else is wrong.
    let reply = server.raw_in(scope, "PUT", "/api/notes/nope/relationships/nope?x=1", &bad_accept, "[").await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    // 2 beats 3.
    let reply = server.raw_in(scope, "DELETE", "/api/notes/nope/relationships/nope?x=1", &bad_accept, "[").await;
    reply.error(StatusCode::NOT_ACCEPTABLE, "not_acceptable");
    // 3 beats 4: the body's media type before an unknown `{rel}`, on each
    // write method.
    for method in ["PATCH", "POST", "DELETE"] {
        let reply = server.raw_in(scope, method, "/api/notes/alpha/relationships/nope?x=1", &bad_content, "[").await;
        reply.error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
    }
    // 4 beats 5 and 8: an unknown `{rel}` before the query and the parent.
    server
        .get_in(scope, "/api/notes/nope/relationships/nope?x=1")
        .await
        .error(StatusCode::NOT_FOUND, "relationship_not_found");
    server.get_in(scope, "/api/notes/nope/nope?x=1").await.error(StatusCode::NOT_FOUND, "relationship_not_found");
    for method in ["PATCH", "POST", "DELETE"] {
        let reply = server.op_in(scope, method, "/api/notes/nope/relationships/nope?x=1", "[").await;
        reply.error(StatusCode::NOT_FOUND, "relationship_not_found");
    }
    // 5 beats 6 and 8: the query before the refusal and the parent.
    let reply = server.op_in(scope, "PATCH", "/api/notes/nope/relationships/tags?x=1", "[").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "x");
    let reply = server.get_in(scope, "/api/notes/nope/relationships/tags?page[limit]=0").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "page[limit]");
    let reply = server.get_in(scope, "/api/notes/nope/tags?page[offset]=-1").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "page[offset]");
    // 6 beats 7 and 8: the refusal is decided before the body is read or
    // the parent looked up.
    let reply = server.op_in(scope, "PATCH", "/api/notes/nope/relationships/tags", "[").await;
    reply.error(StatusCode::FORBIDDEN, "relationship_update_unsupported");
    // 7 beats 8: the body before the parent.
    let missing = "/api/notes/nope/relationships/tags";
    server.op_in(scope, "POST", missing, "[").await.error(StatusCode::BAD_REQUEST, "invalid_document");
    assert_eq!(
        server.op_in(scope, "DELETE", missing, "{}").await.pointer(StatusCode::BAD_REQUEST, "invalid_document"),
        ""
    );
    let reply = server.op_in(scope, "POST", missing, r#"{"data":[{"type":"notes","id":"x"}]}"#).await;
    assert_eq!(reply.pointer(StatusCode::CONFLICT, "type_mismatch"), "/data/0");
    // 8: the parent before the target.
    let reply = server.op_in(scope, "POST", missing, r#"{"data":[{"type":"tags","id":"nope"}]}"#).await;
    reply.error(StatusCode::NOT_FOUND, "note_not_found");
    assert!(server.note_tags("alpha").is_empty() && server.note_tags("nope").is_empty());
}
in_both_scopes!(junction_relationship_checks_run_in_the_contract_order);

#[tokio::test]
async fn scoped_relationship_routes_answer_as_the_unscoped_ones() {
    let server = Server::new();
    server.task("Alpha", "open").await;
    server.task("Beta", "open").await;
    server.tag("a", "A").await;
    let scoped = |uri: &str| uri.replacen("/api/", &format!("/api/{}/", project_path()), 1);

    server
        .write_scoped(
            "POST",
            &scoped("/api/tasks/alpha/relationships/labels"),
            linkage(json!([identifier("tags", "a")])),
        )
        .await
        .no_content();
    server
        .write_scoped("PATCH", &scoped("/api/tasks/beta/relationships/parent"), linkage(identifier("tasks", "alpha")))
        .await
        .no_content();
    for uri in [
        "/api/tasks/alpha",
        "/api/tasks/alpha/relationships/labels",
        "/api/tasks/alpha/labels",
        "/api/tasks/alpha/relationships/tags",
        "/api/tasks/alpha/tags",
        "/api/tasks/beta/parent",
        "/api/tasks/alpha/subtasks?page[limit]=1",
        "/api/tasks/alpha/relationships/nope",
    ] {
        let unscoped = server.get(uri).await;
        let reply = server.get_scoped(&scoped(uri)).await;
        assert_eq!(reply.status, unscoped.status, "{uri}: {}", reply.raw);
        assert_eq!(reply.raw, unscoped.raw.replace("\"/api/", &format!("\"/api/{}/", project_path())), "{uri}");
    }
    let reply = server.get_scoped(&scoped("/api/tasks/alpha")).await;
    assert_eq!(
        reply.body["data"]["relationships"]["labels"]["links"]["related"],
        "/api/projects/pilot/tasks/alpha/labels"
    );
}

// ── A module with no entity (§10.4) ──

#[tokio::test]
async fn crud_in_a_module_without_an_entity_is_served_as_custom_ops() {
    let server = Server::new();
    let bookmark = |n: u32| json!({ "url": format!("https://example.com/{n}"), "title": format!("Page {n}") });

    // `create` answers `200` with its result, not `201` with a resource.
    let reply = server.op("POST", "/api/bookmarks", &args(json!({ "input": bookmark(1) }))).await;
    reply.ok_result(r#"{"id":"bm-1","url":"https://example.com/1","title":"Page 1"}"#);
    assert!(reply.headers.get(header::LOCATION).is_none());
    for n in [2, 3] {
        server.op("POST", "/api/bookmarks", &args(json!({ "input": bookmark(n) }))).await;
    }

    // The page goes to the fn and the total comes from its `count`.
    server.get("/api/bookmarks").await.ok_result(concat!(
        r#"{"items":[{"id":"bm-1","url":"https://example.com/1","title":"Page 1"},"#,
        r#"{"id":"bm-2","url":"https://example.com/2","title":"Page 2"}],"total":3,"limit":2,"offset":0}"#,
    ));
    server.get("/api/bookmarks?opArg[offset]=2").await.ok_result(concat!(
        r#"{"items":[{"id":"bm-3","url":"https://example.com/3","title":"Page 3"}],"#,
        r#""total":3,"limit":2,"offset":2}"#,
    ));
    let reply = server.get("/api/bookmarks?page[offset]=2").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "page[offset]");

    server
        .get("/api/bookmarks/bm-2")
        .await
        .ok_result(r#"{"id":"bm-2","url":"https://example.com/2","title":"Page 2"}"#);
    let reply = server.get("/api/bookmarks/bm-2?include=x").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "include");

    // `update` is a PATCH reading `meta.args.input`.
    server
        .op("PATCH", "/api/bookmarks/bm-2", &args(json!({ "input": { "title": "Renamed" } })))
        .await
        .ok_result(r#"{"id":"bm-2","url":"https://example.com/2","title":"Renamed"}"#);
    let reply = server.op("PATCH", "/api/bookmarks/bm-2", &args(json!({ "input": { "title": 7 } }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args/input");
    let reply = server.op("PATCH", "/api/bookmarks/nope", &args(json!({ "input": {} }))).await;
    reply.error(StatusCode::NOT_FOUND, "bookmark_not_found");

    server.send(Request::delete("/api/bookmarks/bm-2").body(Body::empty()).unwrap()).await.no_content();
    let reply = server.get("/api/bookmarks/bm-2").await;
    assert_eq!(
        reply.raw,
        concat!(
            r#"{"jsonapi":{"version":"1.1"},"errors":[{"status":"404","code":"bookmark_not_found","#,
            r#""title":"Not Found","detail":"Bookmark not found: bm-2"}]}"#,
        )
    );

    // No generated route uses PUT.
    let reply = server.op("PUT", "/api/bookmarks/bm-1", &args(json!({ "input": {} }))).await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD, PATCH, DELETE");
    let reply = server.op("PUT", "/api/bookmarks", &args(json!({}))).await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD, POST");

    // Negotiation and the body's media type apply as on every op.
    let reply = server.raw("GET", "/api/bookmarks", &[(header::ACCEPT, "application/json")], "").await;
    reply.error(StatusCode::NOT_ACCEPTABLE, "not_acceptable");
    let reply =
        server.raw("POST", "/api/bookmarks", &[(header::CONTENT_TYPE, "application/json")], &args(json!({}))).await;
    reply.error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
}

/// The ids of a `meta.result` page's items, in order.
fn item_ids(result: &Value) -> Vec<&str> {
    result["items"].as_array().expect("items").iter().map(|i| i["id"].as_str().expect("id")).collect()
}

/// `board` has no entity, so its junction ops are custom ops at
/// `/api/boards/{parent_id}/tasks` (and `…/{child_id}` to remove). They take
/// the store, so the scoped router serves the same routes under the prefix.
async fn junction_ops_outside_a_resource_module_are_custom_ops(scope: Scope) {
    let server = Server::new();
    for title in ["Alpha", "Beta", "Gamma"] {
        server.task(title, "open").await;
    }
    server.tag("a", "A").await;
    let board = "/api/boards/a/tasks";

    // An add carries the child id in `meta.args` and answers `204`.
    for id in ["alpha", "beta", "gamma", "alpha"] {
        server.op_in(scope, "POST", board, &args(json!({ "task_id": id }))).await.no_content();
    }
    assert_eq!(server.task_tags("alpha").await, ["a"]);

    // The list is a `meta.result` page of `opArg[limit]` and `opArg[offset]`,
    // with no links.
    let reply = server.get_in(scope, board).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    let result = &reply.body["meta"]["result"];
    assert_eq!(item_ids(result), ["alpha", "beta"]);
    assert_eq!((&result["total"], &result["limit"], &result["offset"]), (&json!(3), &json!(2), &json!(0)));
    assert!(reply.body.get("links").is_none() && reply.body.get("data").is_none(), "{}", reply.raw);
    let reply = server.get_in(scope, &format!("{board}?opArg[offset]=2&opArg[limit]=9")).await;
    assert_eq!(item_ids(&reply.body["meta"]["result"]), ["gamma"]);
    assert_eq!(reply.body["meta"]["result"]["limit"], 3);
    // `opArg[limit]=0` is an empty page, not an error (§10.4).
    server
        .get_in(scope, &format!("{board}?opArg[limit]=0"))
        .await
        .ok_result(r#"{"items":[],"total":3,"limit":0,"offset":0}"#);
    for (query, parameter) in [("page[limit]=1", "page[limit]"), ("opArg[limit]=-1", "opArg[limit]")] {
        let reply = server.get_in(scope, &format!("{board}?{query}")).await;
        assert_eq!(reply.parameter("invalid_query_parameter"), parameter);
    }

    // A remove names the child in the path; a body, and its media type, are
    // ignored.
    let reply =
        server.raw_in(scope, "DELETE", "/api/boards/a/tasks/beta", &[(header::CONTENT_TYPE, "text/plain")], "x").await;
    reply.no_content();
    assert!(server.task_tags("beta").await.is_empty());
    server.raw_in(scope, "DELETE", "/api/boards/a/tasks/beta", &[], "").await.no_content();
    let reply = server.get_in(scope, board).await;
    assert_eq!(reply.body["meta"]["result"]["total"], 2);

    // The arguments are checked as any custom op's, and the ops' own errors
    // map through `AppError`.
    let reply = server.op_in(scope, "POST", board, &args(json!({}))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args");
    let reply = server.op_in(scope, "POST", board, &args(json!({ "task_id": "alpha", "x": 1 }))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args/x");
    let reply = server.op_in(scope, "POST", &format!("{board}?opArg[task_id]=alpha"), &args(json!({}))).await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "opArg[task_id]");
    server
        .op_in(scope, "POST", board, &args(json!({ "task_id": "nope" })))
        .await
        .error(StatusCode::NOT_FOUND, "task_not_found");
    server.get_in(scope, "/api/boards/nope/tasks").await.error(StatusCode::NOT_FOUND, "tag_not_found");
    let reply = server
        .raw_in(
            scope,
            "POST",
            board,
            &[(header::CONTENT_TYPE, "application/json")],
            &args(json!({ "task_id": "beta" })),
        )
        .await;
    reply.error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");

    // Each route names its methods.
    let reply = server.op_in(scope, "PUT", board, "{}").await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD, POST");
    let reply = server.get_in(scope, "/api/boards/a/tasks/alpha").await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "DELETE");
}
in_both_scopes!(junction_ops_outside_a_resource_module_are_custom_ops);

// ── Filtered lists (§7.3, §10.4) ──
//
// `section::list` takes a `ListSectionsQuery` and a required bare
// `parent_id`, `tag::list` two optional bare filters, `bookmark::list` (no
// entity) a `BookmarkQuery`, and `outline::list` (no entity, takes the
// store) an optional bare filter. Each is hand-written beside the generated
// module and replaces its `list` and `count`.

/// An outline: `root` is its own parent, `usage` has two children.
async fn seed_sections(server: &Server) {
    server.section("root", "Root", "root").await;
    server.section("intro", "Introduction", "root").await;
    server.section("usage", "Usage", "root").await;
    server.section("install", "Install it", "usage").await;
    server.section("cli", "Usage on the CLI", "usage").await;
}

/// The ids of a collection document's resources, in order.
fn ids(body: &Value) -> Vec<&str> {
    body["data"].as_array().expect("data").iter().map(|r| r["id"].as_str().expect("id")).collect()
}

#[tokio::test]
async fn a_list_filtered_by_a_query_struct_and_a_bare_filter() {
    let server = Server::new();
    seed_sections(&server).await;

    // The required bare filter alone; `meta.total` counts the filtered set.
    let reply = server.get("/api/sections?filter[parent_id]=root").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    assert_eq!(reply.headers[header::CONTENT_TYPE], MEDIA_TYPE);
    assert_eq!(ids(&reply.body), ["intro", "root"]);
    assert_eq!(reply.body["meta"], json!({ "total": 3, "limit": 2, "offset": 0 }));
    let page =
        |offset: u32| format!("/api/sections?filter%5Bparent_id%5D=root&page%5Boffset%5D={offset}&page%5Blimit%5D=2");
    assert_eq!(
        reply.body["links"],
        json!({ "self": page(0), "first": page(0), "prev": null, "next": page(2), "last": page(2) })
    );
    let reply = server.get("/api/sections?filter[parent_id]=root&page[offset]=2").await;
    assert_eq!(ids(&reply.body), ["usage"]);
    assert_eq!(reply.body["links"]["prev"], page(0));

    // A struct member with the bare filter: links write the members in byte
    // order of name whatever order they came in, and encoded brackets read
    // the same as raw ones.
    let reply = server.get("/api/sections?filter[parent_id]=root&filter[min_children]=1").await;
    assert_eq!(ids(&reply.body), ["root", "usage"]);
    assert_eq!(reply.body["meta"], json!({ "total": 2, "limit": 2, "offset": 0 }));
    assert_eq!(
        reply.body["links"]["self"],
        "/api/sections?filter%5Bmin_children%5D=1&filter%5Bparent_id%5D=root&page%5Boffset%5D=0&page%5Blimit%5D=2"
    );
    assert_eq!(reply.body["links"]["next"], Value::Null);
    let encoded = server.get("/api/sections?filter%5Bparent_id%5D=root&filter%5Bmin_children%5D=1").await;
    assert_eq!(encoded.raw, reply.raw);

    // Every struct member at once; a value is percent-encoded in links.
    let reply = server
        .get("/api/sections?filter[title_contains]=on%20the&filter[max_children]=0&filter[parent_id]=usage&filter[min_children]=0")
        .await;
    assert_eq!(ids(&reply.body), ["cli"]);
    assert_eq!(reply.body["meta"]["total"], 1);
    assert_eq!(
        reply.body["links"]["self"],
        concat!(
            "/api/sections?filter%5Bmax_children%5D=0&filter%5Bmin_children%5D=0&filter%5Bparent_id%5D=usage",
            "&filter%5Btitle_contains%5D=on%20the&page%5Boffset%5D=0&page%5Blimit%5D=2",
        )
    );
}

#[tokio::test]
async fn a_list_filtered_by_optional_bare_filters() {
    let server = Server::new();
    for (id, title) in [("al", "Alpha"), ("alp", "Alpine Lake"), ("be", "Beta"), ("ga", "Gamma ray")] {
        server.tag(id, title).await;
    }

    // No filter: every tag, as the generated list served them.
    let reply = server.get("/api/tags").await;
    assert_eq!(ids(&reply.body), ["al", "alp"]);
    assert_eq!(reply.body["meta"], json!({ "total": 4, "limit": 2, "offset": 0 }));
    assert_eq!(reply.body["links"]["self"], "/api/tags?page%5Boffset%5D=0&page%5Blimit%5D=2");

    let reply = server.get("/api/tags?filter[title_prefix]=Al").await;
    assert_eq!(ids(&reply.body), ["al", "alp"]);
    assert_eq!(reply.body["meta"]["total"], 2);
    let reply = server.get("/api/tags?filter[min_title_len]=6").await;
    assert_eq!(ids(&reply.body), ["alp", "ga"]);
    assert_eq!(reply.body["meta"]["total"], 2);

    let reply = server.get("/api/tags?filter[title_prefix]=Al&filter[min_title_len]=6").await;
    assert_eq!(ids(&reply.body), ["alp"]);
    assert_eq!(reply.body["meta"], json!({ "total": 1, "limit": 2, "offset": 0 }));
    let page = "/api/tags?filter%5Bmin_title_len%5D=6&filter%5Btitle_prefix%5D=Al&page%5Boffset%5D=0&page%5Blimit%5D=2";
    assert_eq!(reply.body["links"], json!({ "self": page, "first": page, "prev": null, "next": null, "last": page }));

    // An empty value is `Some("")`, which every title starts with, and the
    // link keeps the member.
    let reply = server.get("/api/tags?filter[title_prefix]=").await;
    assert_eq!(reply.body["meta"]["total"], 4);
    assert_eq!(reply.body["links"]["self"], "/api/tags?filter%5Btitle_prefix%5D=&page%5Boffset%5D=0&page%5Blimit%5D=2");
}

#[tokio::test]
async fn a_list_without_an_entity_takes_its_filter_and_op_arg_page() {
    let server = Server::new();
    server.bookmark("https://example.com/1", "One").await;
    server.bookmark("https://rust-lang.org/2", "Two").await;
    server.bookmark("https://example.com/3", "Three").await;

    // The page and the filter both reach the fn; `total` is its filtered
    // `count`, and a `meta.result` page has no links.
    server.get("/api/bookmarks?filter[url_contains]=example&opArg[limit]=1&opArg[offset]=1").await.ok_result(
        r#"{"items":[{"id":"bm-3","url":"https://example.com/3","title":"Three"}],"total":2,"limit":1,"offset":1}"#,
    );
    server.get("/api/bookmarks?opArg%5Blimit%5D=5&filter%5Burl_contains%5D=rust").await.ok_result(
        r#"{"items":[{"id":"bm-2","url":"https://rust-lang.org/2","title":"Two"}],"total":1,"limit":3,"offset":0}"#,
    );
    server
        .get("/api/bookmarks?filter[url_contains]=example&opArg[limit]=0")
        .await
        .ok_result(r#"{"items":[],"total":2,"limit":0,"offset":0}"#);

    // An optional bare filter on a list without an entity.
    seed_sections(&server).await;
    let reply = server.get("/api/outlines?filter[title_contains]=Us").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.raw);
    let items = reply.body["meta"]["result"]["items"].as_array().expect("items");
    assert_eq!(items.iter().map(|s| s["id"].as_str().unwrap()).collect::<Vec<_>>(), ["cli", "usage"]);
    assert_eq!(reply.body["meta"]["result"]["total"], 2);
    assert!(reply.body.get("links").is_none());
}

#[tokio::test]
async fn filter_parameters_are_checked() {
    let server = Server::new();
    seed_sections(&server).await;
    let parameter = async |uri: &str| server.get(uri).await.parameter("invalid_query_parameter");

    // A member that is not a field of the struct or a bare filter.
    assert_eq!(parameter("/api/sections?filter[parent_id]=root&filter[colour]=red").await, "filter[colour]");
    assert_eq!(parameter("/api/tags?filter[title]=A").await, "filter[title]");
    assert_eq!(parameter("/api/bookmarks?filter[url]=x").await, "filter[url]");
    assert_eq!(parameter("/api/outlines?filter[title]=x").await, "filter[title]");
    // A value serde rejects, in the struct and as a bare filter.
    assert_eq!(
        parameter("/api/sections?filter[parent_id]=root&filter[min_children]=many").await,
        "filter[min_children]"
    );
    assert_eq!(parameter("/api/tags?filter[min_title_len]=-1").await, "filter[min_title_len]");
    // A missing required bare filter.
    assert_eq!(parameter("/api/sections").await, "filter[parent_id]");
    assert_eq!(parameter("/api/sections?filter[min_children]=1").await, "filter[parent_id]");
    // Nested and array forms, and a dotted member.
    assert_eq!(parameter("/api/sections?filter[parent_id][]=root").await, "filter[parent_id][]");
    assert_eq!(parameter("/api/sections?filter[parent_id][x]=root").await, "filter[parent_id][x]");
    assert_eq!(parameter("/api/sections?filter[parent.id]=root").await, "filter[parent.id]");
    assert_eq!(parameter("/api/tags?filter%5Btitle_prefix%5D%5B%5D=A").await, "filter[title_prefix][]");
    // A repeated member, struct or bare, and in a list without an entity.
    assert_eq!(
        parameter("/api/sections?filter[parent_id]=root&filter[min_children]=1&filter[min_children]=1").await,
        "filter[min_children]"
    );
    assert_eq!(parameter("/api/sections?filter[parent_id]=root&filter[parent_id]=usage").await, "filter[parent_id]");
    assert_eq!(parameter("/api/bookmarks?filter[url_contains]=a&filter[url_contains]=b").await, "filter[url_contains]");
    // Any `filter[…]` on a list that takes none: a resource list, and a
    // junction relationship's related link.
    assert_eq!(parameter("/api/notes?filter[title]=x").await, "filter[title]");
    server.task("Alpha", "open").await;
    assert_eq!(parameter("/api/tasks/alpha/labels?filter[id]=x").await, "filter[id]");
}

#[tokio::test]
async fn filter_checks_run_in_the_contract_order() {
    let server = Server::new();
    let parameter = async |uri: &str| server.get(uri).await.parameter("invalid_query_parameter");

    // Step 5, first half: a name the route does not accept beats a bad
    // filter value, wherever each stands in the request.
    assert_eq!(parameter("/api/sections?filter[min_children]=x&foo=1").await, "foo");
    assert_eq!(parameter("/api/sections?filter[min_children]=x&filter[colour]=1").await, "filter[colour]");
    assert_eq!(parameter("/api/bookmarks?filter[url_contains]=a&filter[url_contains]=b&opArg[x]=1").await, "opArg[x]");

    // The filter comes before `sort`, `include` and the page.
    assert_eq!(
        parameter("/api/sections?page[limit]=0&filter[min_children]=x&filter[parent_id]=root").await,
        "filter[min_children]"
    );
    assert_eq!(parameter("/api/sections?page[limit]=0").await, "filter[parent_id]");
    assert_eq!(parameter("/api/tags?page[offset]=-1&filter[min_title_len]=x").await, "filter[min_title_len]");
    let error = server.get("/api/sections?sort=title&filter[parent_id]=a&filter[parent_id]=b").await;
    assert_eq!(error.parameter("invalid_query_parameter"), "filter[parent_id]");
    let error = server.get("/api/sections?sort=title&filter[parent_id]=root").await;
    assert_eq!(error.parameter("invalid_sort_field"), "sort");
    assert_eq!(
        parameter("/api/bookmarks?opArg[limit]=-1&filter[url_contains]=a&filter[url_contains]=b").await,
        "filter[url_contains]"
    );
    assert_eq!(
        parameter("/api/outlines?opArg[offset]=x&filter[title_contains]=a&filter[title_contains]=b").await,
        "filter[title_contains]"
    );

    // The struct's members before the bare filters, though `parent_id`
    // sorts before `title_contains`.
    assert_eq!(
        parameter("/api/sections?filter[title_contains]=a&filter[title_contains]=b").await,
        "filter[title_contains]"
    );
    assert_eq!(
        parameter("/api/sections?filter[parent_id]=a&filter[parent_id]=b&filter[min_children]=x").await,
        "filter[min_children]"
    );

    // Among the struct's members, byte order of name: not request order,
    // and not the declaration order (`min_children` before `max_children`).
    assert_eq!(
        parameter("/api/sections?filter[min_children]=x&filter[max_children]=y&filter[parent_id]=root").await,
        "filter[max_children]"
    );
    // Among bare filters too (`title_prefix` is declared first).
    assert_eq!(
        parameter("/api/tags?filter[title_prefix]=a&filter[title_prefix]=b&filter[min_title_len]=x").await,
        "filter[min_title_len]"
    );
}

#[tokio::test]
async fn a_scoped_filtered_list_answers_as_the_unscoped_one() {
    let server = Server::new();
    seed_sections(&server).await;
    for (id, title) in [("al", "Alpha"), ("alp", "Alpine Lake"), ("be", "Beta")] {
        server.tag(id, title).await;
    }

    // Successes and failures alike, resource lists and a list without an
    // entity: the scoped reply is the unscoped one with the prefix in its
    // links.
    for uri in [
        "/api/sections?filter[parent_id]=root",
        "/api/sections?filter[parent_id]=root&filter[min_children]=1&page[offset]=1&page[limit]=1",
        "/api/sections?filter%5Bparent_id%5D=usage&filter%5Btitle_contains%5D=on%20the",
        "/api/sections",
        "/api/sections?filter[colour]=red",
        "/api/sections?filter[min_children]=x&filter[max_children]=y&filter[parent_id]=root",
        "/api/sections?filter[parent_id][]=root",
        "/api/tags?filter[title_prefix]=Al&filter[min_title_len]=6",
        "/api/tags?filter[min_title_len]=-1",
        "/api/outlines?filter[title_contains]=Us&opArg[limit]=1",
        "/api/outlines?filter[title]=x",
        "/api/notes?filter[title]=x",
    ] {
        let unscoped = server.get(uri).await;
        let scoped = server.get_scoped(&uri.replacen("/api/", &format!("/api/{}/", project_path()), 1)).await;
        assert_eq!(scoped.status, unscoped.status, "{uri}: {}", scoped.raw);
        assert_eq!(scoped.raw, unscoped.raw.replace("\"/api/", &format!("\"/api/{}/", project_path())), "{uri}");
    }
    let reply = server.get_scoped("/api/projects/pilot/sections?filter[parent_id]=root").await;
    assert_eq!(
        reply.body["links"]["self"],
        "/api/projects/pilot/sections?filter%5Bparent_id%5D=root&page%5Boffset%5D=0&page%5Blimit%5D=2"
    );
    assert_eq!(reply.body["data"][0]["links"]["self"], "/api/projects/pilot/sections/intro");

    // The scope accessor refuses an unknown project.
    let reply = server.get_scoped("/api/projects/nope/sections?filter[parent_id]=root").await;
    reply.error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
}

fn project_path() -> String {
    format!("projects/{PROJECT}")
}

// ── Event streams (§12) ──

/// The next frame of an event stream, as sent: its lines through the blank
/// line that ends it.
async fn next_frame(stream: &mut BodyDataStream) -> String {
    let mut frame = String::new();
    while !frame.ends_with("\n\n") {
        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("a frame within 5s")
            .expect("the stream stays open")
            .expect("a readable chunk");
        frame.push_str(std::str::from_utf8(&chunk).expect("UTF-8 frame"));
    }
    frame
}

#[tokio::test]
async fn an_entity_event_frame_is_its_resource_object_without_links() {
    let server = Server::new();
    let (status, headers, mut stream) = server.subscribe("/api/events/task-feed").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/event-stream");

    let task = Task {
        id: "ship-it".into(),
        title: "Ship it".into(),
        status: "open".into(),
        parent_id: Some("epic".into()),
        subtasks: vec![],
        tags: vec!["codegen".into()],
        body: "Go.\n".into(),
    };
    server.state.task_feed.send(task).expect("a subscriber");

    assert_eq!(
        next_frame(&mut stream).await,
        concat!(
            "event: task-feed\n",
            r#"data: {"type":"tasks","id":"ship-it","attributes":{"title":"Ship it","status":"open","body":"Go.\n"},"#,
            r#""relationships":{"parent":{"data":{"type":"tasks","id":"epic"}},"subtasks":{"data":[]},"#,
            r#""tags":{"data":[{"type":"tags","id":"codegen"}]}}}"#,
            "\n\n",
        )
    );
}

#[tokio::test]
async fn a_non_entity_event_frame_is_a_meta_result() {
    let server = Server::new();
    let (status, _, mut stream) = server.subscribe("/api/events/bookmark-feed").await;
    assert_eq!(status, StatusCode::OK);

    // The op publishes what it creates.
    let input = json!({ "url": "https://example.com", "title": "Example" });
    server.op("POST", "/api/bookmarks", &args(json!({ "input": input }))).await;

    assert_eq!(
        next_frame(&mut stream).await,
        concat!(
            "event: bookmark-feed\n",
            r#"data: {"meta":{"result":{"id":"bm-1","url":"https://example.com","title":"Example"}}}"#,
            "\n\n",
        )
    );
}

#[tokio::test]
async fn a_failed_subscribe_maps_its_app_error() {
    let server = Server::new();
    server.task("Alpha", "open").await;

    // The subscribe fails before the stream opens: an error document.
    let request = Request::get("/api/events/watch-task/nope").body(Body::empty()).unwrap();
    let reply = server.send(request).await;
    assert_eq!(
        reply.raw,
        concat!(
            r#"{"jsonapi":{"version":"1.1"},"errors":[{"status":"404","code":"task_not_found","#,
            r#""title":"Not Found","detail":"Task not found: nope"}]}"#,
        )
    );
    reply.error(StatusCode::NOT_FOUND, "task_not_found");

    let (status, _, mut stream) = server.subscribe("/api/events/watch-task/alpha").await;
    assert_eq!(status, StatusCode::OK);
    let alpha = server.store().get_task("alpha").await.expect("task");
    server.state.task_feed.send(alpha).expect("a subscriber");
    let frame = next_frame(&mut stream).await;
    assert!(frame.starts_with("event: watch-task\ndata: {\"type\":\"tasks\",\"id\":\"alpha\","), "{frame}");
    assert!(!frame.contains("links"), "{frame}");
}
