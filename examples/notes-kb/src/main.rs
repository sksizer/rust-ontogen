//! Serve the notes API plus the graph view.
//!
//! ```sh
//! cargo run                            # http://127.0.0.1:3003 (graph at /)
//! cargo run --features vault-tags -- tags   # markdown-vault composition demo
//! ```

use std::sync::Arc;

use markdown_store::{IdStrategy, VaultHandle, VaultLayout};
use notes_kb::AppState;

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("tags") {
        #[cfg(feature = "vault-tags")]
        {
            for tag in notes_kb::vault_tags(std::path::Path::new("data/vault")) {
                println!("{tag}");
            }
            return;
        }
        #[cfg(not(feature = "vault-tags"))]
        {
            eprintln!("rebuild with --features vault-tags (requires the sibling rust-markdown checkout)");
            std::process::exit(2);
        }
    }

    let vault = VaultHandle::new("data/vault", VaultLayout::PerEntityDir, IdStrategy::SlugFromField("title".into()));
    let state = Arc::new(AppState::new(vault));

    let app = notes_kb::api::transport::http::generated::entity_routes()
        .fallback_service(tower_http::services::ServeDir::new("web"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3003").await.expect("bind 127.0.0.1:3003");
    println!("notes-kb at http://127.0.0.1:3003 — the wikilink graph is the index page");
    axum::serve(listener, app).await.expect("serve");
}
