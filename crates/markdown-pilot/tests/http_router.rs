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

use std::sync::Arc;

use std::time::Duration;

use axum::body::{Body, BodyDataStream, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use futures::StreamExt;
use markdown_pilot::api::transport::http::generated::entity_routes;
use markdown_pilot::persistence::markdown::generated::open_vault;
use markdown_pilot::schema::{Note, Tag, Task};
use markdown_pilot::{AppState, Store};
use serde_json::{Value, json};
use tower::util::ServiceExt;

const MEDIA_TYPE: &str = "application/vnd.api+json";

#[test]
fn entity_routes_constructs_router() {
    // A panic here means the emitted route syntax is invalid for the axum
    // version this crate compiles against.
    let _router = entity_routes();
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
        let response = entity_routes().with_state(Arc::clone(&self.state)).oneshot(request).await.expect("infallible");
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        let raw = String::from_utf8(bytes.to_vec()).expect("UTF-8 body");
        let body = if raw.is_empty() { Value::Null } else { serde_json::from_str(&raw).expect("JSON body") };
        Reply { status: parts.status, headers: parts.headers, body, raw }
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Request::get(uri).header(header::ACCEPT, MEDIA_TYPE).body(Body::empty()).unwrap()).await
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
            r#""links":{"self":"/api/notes/alpha"}},"#,
            r#"{"type":"notes","id":"beta","attributes":{"title":"Beta","body":"About Beta.\n"},"#,
            r#""links":{"self":"/api/notes/beta"}}]}"#,
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
    server.get("/api/notes?filter[title]=x").await.error(StatusCode::BAD_REQUEST, "invalid_query_parameter");

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
            r#""links":{"self":"/api/notes/hello-vault"}}}"#,
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
    // Duplicates collapse to their first occurrence (§5.4).
    assert!(
        reply.raw.contains(concat!(
            r#""relationships":{"parent":{"data":null},"subtasks":{"data":[]},"#,
            r#""tags":{"data":[{"type":"tags","id":"codegen"}]}},"links":{"self":"/api/tasks/ship-it"}}"#,
        )),
        "{}",
        reply.raw
    );
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

// ── Junction ops (§10.4) ──

#[tokio::test]
async fn junction_ops_are_served_as_custom_ops() {
    let server = Server::new();
    server.task("Alpha", "open").await;
    for (id, title) in [("a", "A"), ("b", "B"), ("c", "C")] {
        server.tag(id, title).await;
    }

    for tag_id in ["a", "b", "c", "a"] {
        server.op("POST", "/api/tasks/alpha/tags", &args(json!({ "tag_id": tag_id }))).await.no_content();
    }

    // Paginated in memory: `default_limit: 2`, `max_limit: 3`.
    server.get("/api/tasks/alpha/tags").await.ok_result(concat!(
        r#"{"items":[{"id":"a","title":"A"},{"id":"b","title":"B"}],"#,
        r#""total":3,"limit":2,"offset":0}"#,
    ));
    server
        .get("/api/tasks/alpha/tags?opArg[offset]=2&opArg[limit]=50")
        .await
        .ok_result(r#"{"items":[{"id":"c","title":"C"}],"total":3,"limit":3,"offset":2}"#);
    // The page is an `opArg`, not `page[…]`.
    let reply = server.get("/api/tasks/alpha/tags?page[limit]=1").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "page[limit]");
    let reply = server.get("/api/tasks/alpha/tags?opArg[limit]=two").await;
    assert_eq!(reply.parameter("invalid_query_parameter"), "opArg[limit]");

    server.send(Request::delete("/api/tasks/alpha/tags/b").body(Body::empty()).unwrap()).await.no_content();
    assert_eq!(server.store().get_task("alpha").await.expect("task").tags, ["a", "c"]);

    // The child argument is required, and the ops' `AppError`s map.
    let reply = server.op("POST", "/api/tasks/alpha/tags", &args(json!({}))).await;
    assert_eq!(reply.pointer(StatusCode::BAD_REQUEST, "invalid_document"), "/meta/args");
    let reply = server.op("POST", "/api/tasks/alpha/tags", &args(json!({ "tag_id": "nope" }))).await;
    reply.error(StatusCode::NOT_FOUND, "tag_not_found");
    server.get("/api/tasks/nope/tags").await.error(StatusCode::NOT_FOUND, "task_not_found");

    let reply = server.op("PATCH", "/api/tasks/alpha/tags", &args(json!({ "tag_id": "a" }))).await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD, POST");
    let reply = server.get("/api/tasks/alpha/tags/a").await;
    reply.error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert_eq!(reply.headers[header::ALLOW], "DELETE");
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
