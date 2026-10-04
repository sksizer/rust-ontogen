//! Serve the notes API plus the graph view.
//!
//! ```sh
//! cargo run                # http://127.0.0.1:3003 (graph at /)
//! PORT=39103 cargo run     # any other port
//! ```

use std::sync::Arc;

use notes_kb::AppState;
use notes_kb::persistence::markdown::generated::{VAULT_ROOT, open_vault};

#[tokio::main]
async fn main() {
    let addr = format!("127.0.0.1:{}", port(3003));
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

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    let addr = listener.local_addr().expect("local address");
    println!("notes-kb at http://{addr} — the wikilink graph is the index page");
    axum::serve(listener, app).await.expect("serve");
}

/// The port to serve on: `PORT` when set, so several examples can run side
/// by side, otherwise `default`. A value that is not a port number stops the
/// server instead of quietly binding somewhere else.
fn port(default: u16) -> u16 {
    match std::env::var("PORT") {
        Ok(v) => v.parse().unwrap_or_else(|_| panic!("PORT must be a port number (0-65535), got {v:?}")),
        Err(std::env::VarError::NotPresent) => default,
        Err(e) => panic!("PORT: {e}"),
    }
}
