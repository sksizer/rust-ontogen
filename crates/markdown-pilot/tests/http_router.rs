//! Proof that the generated axum HTTP handlers compile against the axum
//! version in Cargo.toml AND that the router actually builds and routes.
//!
//! The construction half matters as much as the compile half: axum 0.8
//! rejects 0.7-style `/:param` paths with a panic at `Router::route` time,
//! not at compile time — a build-only CI check stays green while every
//! consumer panics at startup.

use std::sync::Arc;

use markdown_pilot::api::transport::http::generated::entity_routes;
use markdown_pilot::schema::Note;
use markdown_pilot::{AppState, Store};
use markdown_store::{IdStrategy, VaultHandle, VaultLayout};
use tower::util::ServiceExt;

#[test]
fn entity_routes_constructs_router() {
    // A panic here means the emitted route syntax is invalid for the axum
    // version this crate compiles against.
    let _router = entity_routes();
}

#[tokio::test]
async fn generated_routes_serve_requests() {
    let dir = tempfile::tempdir().expect("tempdir");
    let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir, IdStrategy::SlugFromField("title".into()));
    let store = Store::new(vault);
    let created = store
        .create_note(Note { id: String::new(), title: "Hello Vault".into(), body: "Body.\n".into() })
        .await
        .expect("create note");

    let state = Arc::new(AppState { store });

    // Collection route.
    let res = entity_routes()
        .with_state(Arc::clone(&state))
        .oneshot(axum::http::Request::get("/api/notes").body(axum::body::Body::empty()).unwrap())
        .await
        .expect("infallible");
    assert_eq!(res.status(), axum::http::StatusCode::OK);

    // Path-param route: proves the `{id}` segment actually captures.
    let res = entity_routes()
        .with_state(state)
        .oneshot(
            axum::http::Request::get(format!("/api/notes/{}", created.id)).body(axum::body::Body::empty()).unwrap(),
        )
        .await
        .expect("infallible");
    assert_eq!(res.status(), axum::http::StatusCode::OK);
}
