// TODO: review — doc comment updated from old crate reference
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
/// **Known issue:** This function is currently a no-op. Client generation (TypeScript
/// transport, HTTP client, admin registry) is handled inline by `servers::generate_transport()`,
/// which dispatches both `GeneratorConfig::Server` and `GeneratorConfig::Client` variants.
///
/// To generate clients today, use `servers::generate_transport()` directly with a `Config`
/// that includes `GeneratorConfig::Client(...)` entries. The `gen_clients` public API and
/// `ClientsConfig` type exist for forward compatibility but are not yet wired.
///
/// Fix: either wire this function to call transport generators using `ServersOutput` metadata,
/// or extend `ServersConfig.generators` to accept client variants so the full pipeline works
/// through `gen_servers`.
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
