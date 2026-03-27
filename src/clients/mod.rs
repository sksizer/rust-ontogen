//! Client library generators — TypeScript, CLI, admin registry.
//!
//! These generators consume `ServersOutput` to mirror server endpoints exactly.
//! They can also use `ApiOutput` for additional metadata.

use std::path::PathBuf;

use crate::CodegenError;
use crate::ir::{ApiOutput, ServersOutput};

/// Which client generator to run.
pub enum ClientGeneratorConfig {
    /// TypeScript HTTP client.
    HttpTs { output: PathBuf, bindings_path: PathBuf },
    /// TypeScript split transport (HTTP + Tauri IPC auto-switching).
    HttpTauriIpcSplit { output: PathBuf, bindings_path: PathBuf },
    /// Admin entity registry (TypeScript).
    AdminRegistry { output: PathBuf },
}

/// Generate client libraries from server transport metadata.
///
/// Currently a passthrough; client generation is handled inline by the servers module.
/// Future versions will consume `ServersOutput` for structured endpoint metadata.
pub fn generate(
    _servers: &ServersOutput,
    _api: Option<&ApiOutput>,
    _config: &super::ClientsConfig,
) -> Result<(), CodegenError> {
    // Phase 1: client generation is handled inline by the servers module
    // (ts_client, transport, admin generators are currently server-side).
    // This will be split out in a later phase.
    Ok(())
}
