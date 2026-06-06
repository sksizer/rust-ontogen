//! Run the tracker.
//!
//! ```sh
//! cargo run                      # HTTP API at 127.0.0.1:3002
//! cargo run -- mcp-tools         # list the generated MCP tool registry
//! cargo run -- mcp-call <tool> '<json-args>'   # dispatch one MCP tool
//! ```

use std::sync::Arc;

use markdown_store::{IdStrategy, VaultHandle, VaultLayout};
use tasks_tracker::AppState;

#[tokio::main]
async fn main() {
    let vault = VaultHandle::new("data/vault", VaultLayout::PerEntityDir, IdStrategy::SlugFromField("title".into()));
    let state = Arc::new(AppState::new(vault));

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
            let listener = tokio::net::TcpListener::bind("127.0.0.1:3002").await.expect("bind 127.0.0.1:3002");
            println!("tasks-tracker serving the vault at http://127.0.0.1:3002 (try /api/tasks)");
            axum::serve(listener, app).await.expect("serve");
        }
    }
}
