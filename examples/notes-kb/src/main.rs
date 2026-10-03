//! Serve the notes API plus the graph view.
//!
//! ```sh
//! cargo run    # http://127.0.0.1:3003 (graph at /)
//! ```

use std::sync::Arc;

use notes_kb::AppState;
use notes_kb::persistence::markdown::generated::vault::{VAULT_ROOT, open_vault};

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new(open_vault(VAULT_ROOT)));

    let app = notes_kb::api::transport::http::generated::entity_routes()
        .fallback_service(tower_http::services::ServeDir::new("web"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3003").await.expect("bind 127.0.0.1:3003");
    println!("notes-kb at http://127.0.0.1:3003 — the wikilink graph is the index page");
    axum::serve(listener, app).await.expect("serve");
}
