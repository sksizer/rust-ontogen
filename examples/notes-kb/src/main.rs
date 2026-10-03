//! Serve the notes API plus the graph view.
//!
//! ```sh
//! cargo run    # http://127.0.0.1:3003 (graph at /)
//! ```

use std::sync::Arc;

use notes_kb::AppState;
use notes_kb::persistence::markdown::generated::{VAULT_ROOT, open_vault};

#[tokio::main]
async fn main() {
    let vault = open_vault(VAULT_ROOT);
    // A failed index refresh is tracked only in memory, which a restart
    // forgets, and edits made outside the server never refresh one at all,
    // so startup repairs every index. A failure here is reported, not
    // fatal: records are unaffected, as when a refresh after a write fails.
    if vault.okf().index
        && let Err(e) = vault.rebuild_indexes()
    {
        eprintln!("notes-kb: could not rebuild the vault's index files: {e}");
    }
    let state = Arc::new(AppState::new(vault));

    let app = notes_kb::api::transport::http::generated::entity_routes()
        .fallback_service(tower_http::services::ServeDir::new("web"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3003").await.expect("bind 127.0.0.1:3003");
    println!("notes-kb at http://127.0.0.1:3003 — the wikilink graph is the index page");
    axum::serve(listener, app).await.expect("serve");
}
