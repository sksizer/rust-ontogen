//! Serve the generated HTTP API over the markdown vault in `data/vault/`.
//!
//! ```sh
//! cargo run
//! curl -s localhost:3001/api/workouts | jq
//! ```

use std::sync::Arc;

use iron_log_md::AppState;
use iron_log_md::persistence::markdown::generated::{VAULT_ROOT, open_vault};

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new(open_vault(VAULT_ROOT)));

    let app = iron_log_md::api::transport::http::generated::entity_routes().with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await.expect("bind 127.0.0.1:3001");
    println!("iron-log-md serving the vault at http://127.0.0.1:3001 (try /api/workouts)");
    axum::serve(listener, app).await.expect("serve");
}
