//! Serve the generated HTTP API over the markdown vault in `data/vault/`.
//!
//! ```sh
//! cargo run                  # http://127.0.0.1:3001
//! PORT=39101 cargo run       # any other port
//! curl -s localhost:3001/api/workouts | jq
//! ```

use std::sync::Arc;

use iron_log_md::AppState;
use iron_log_md::persistence::markdown::generated::{VAULT_ROOT, open_vault};

#[tokio::main]
async fn main() {
    let addr = format!("127.0.0.1:{}", port(3001));
    let state = Arc::new(AppState::new(open_vault(VAULT_ROOT)));

    let app = iron_log_md::api::transport::http::generated::entity_routes().with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    let addr = listener.local_addr().expect("local address");
    println!("iron-log-md serving the vault at http://{addr} (try /api/workouts)");
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
