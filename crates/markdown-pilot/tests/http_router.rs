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

use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use markdown_pilot::api::transport::http::generated::entity_routes;
use markdown_pilot::persistence::markdown::generated::open_vault;
use markdown_pilot::schema::{Note, Tag};
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
        Server { _dir: dir, state: Arc::new(AppState { store }) }
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
