//! Run the tracker.
//!
//! ```sh
//! cargo run                      # HTTP API at 127.0.0.1:3002
//! PORT=39102 cargo run           # the HTTP API on another port
//! cargo run -- mcp-tools         # list the generated MCP tool registry
//! cargo run -- mcp-call <tool> '<json-args>'   # dispatch one MCP tool
//! ```

use std::sync::Arc;

use tasks_tracker::AppState;
use tasks_tracker::persistence::markdown::generated::{VAULT_ROOT, open_vault};

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new(open_vault(VAULT_ROOT)));

    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("mcp-tools") => {
            for tool in tasks_tracker::api::transport::mcp::generated::generated_tool_registry() {
                println!("{}\n  {}\n  schema: {}\n", tool.name, tool.description, (tool.schema_fn)());
            }
        }
        Some("mcp-call") => {
            let name = args.next().expect("usage: mcp-call <tool> <json-args>");
            let raw = args.next().unwrap_or_else(|| "{}".into());
            let json: serde_json::Value = serde_json::from_str(&raw).expect("args must be JSON");
            let registry = tasks_tracker::api::transport::mcp::generated::generated_tool_registry();
            let tool = registry.iter().find(|t| t.name == name).unwrap_or_else(|| {
                panic!("no such tool {name:?}; run `mcp-tools` to list");
            });
            match (tool.handler)(&state, &json).await {
                Ok(value) => println!("{value:#}"),
                Err(e) => {
                    eprintln!("tool error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some(other) => {
            eprintln!("unknown mode {other:?}; modes: (none)=HTTP, mcp-tools, mcp-call");
            std::process::exit(2);
        }
        None => {
            let app = tasks_tracker::api::transport::http::generated::entity_routes().with_state(state);
            let addr = format!("127.0.0.1:{}", port(3002));
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| panic!("bind {addr}: {e}"));
            let addr = listener.local_addr().expect("local address");
            println!("tasks-tracker serving the vault at http://{addr} (try /api/tasks)");
            axum::serve(listener, app).await.expect("serve");
        }
    }
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
