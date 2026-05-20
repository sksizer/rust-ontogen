//! Server-side code generators - HTTP (Axum), IPC (Tauri), MCP.
//!
//! The client-side TypeScript + admin-registry generators are siblings under
//! [`crate::clients::generators`].

pub mod http;
pub mod ipc;
pub mod mcp;
