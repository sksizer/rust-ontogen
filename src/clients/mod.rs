//! Client SDK generators - TypeScript bindings, HTTP and HTTP+IPC clients, admin registry.
//!
//! Sibling of [`crate::servers`]: that module owns the server-side Rust
//! transport handlers (Axum, Tauri IPC, MCP), this module owns the
//! client-side TypeScript surface plus the admin-layer entity registry.
//!
//! Entry point: [`crate::gen_clients`] (re-exported here as
//! [`crate::clients::generate`]). The `Pipeline.clients(...)` stage wires
//! it into [`crate::Pipeline`].

pub(crate) mod config;
pub(crate) mod generators;
#[cfg(test)]
mod tests;

// Re-export the public client types at the clients module level so
// downstream consumers say `ontogen::clients::ClientGenerator` /
// `ontogen::clients::ClientsConfig`.
pub use config::ClientGenerator;

use std::path::PathBuf;

use crate::CodegenError;
use crate::ir::ApiOutput;
use crate::servers::ApiModule;
use crate::servers::parse;

/// Generate TypeScript client and admin-registry artefacts.
///
/// Mirrors the shape of [`crate::gen_servers`] - takes the parsed
/// [`ApiOutput`] (or scans `api_dir` itself, as a fallback), the additional
/// scan dirs (reserved for future enrichment), and a [`crate::ClientsConfig`].
///
/// Emits the schema-known TypeScript bindings first (always), then runs
/// [`ontogen_ts`] over the consuming crate's `src/` to discover and emit
/// the long-tail closure of referenced types, then runs each configured
/// client generator ([`ClientGenerator::HttpTs`],
/// [`ClientGenerator::HttpTauriIpcSplit`], [`ClientGenerator::AdminRegistry`])
/// in turn.
///
/// # Errors
///
/// Returns [`CodegenError::Server`] for parse, I/O, or formatting failure.
/// (The error variant predates the split and remains shared with the
/// server pipeline; renaming it is out of scope for this refactor.)
pub fn generate(
    _api: Option<&ApiOutput>,
    _scan_dirs: &[PathBuf],
    config: &crate::ClientsConfig,
) -> Result<(), CodegenError> {
    // Convert public ClientsConfig → internal Config
    let internal = config::Config {
        api_dir: config.api_dir.clone(),
        state_type: config.state_type.clone(),
        service_import_path: config.service_import_path.clone(),
        types_import_path: config.types_import_path.clone(),
        state_import: config.state_import.clone(),
        naming: config.naming.clone(),
        generators: config.generators.clone(),
        sse_route_overrides: config.sse_route_overrides.clone(),
        ts_skip_commands: config.ts_skip_commands.clone(),
        route_prefix: config.route_prefix.clone(),
        store_type: config.store_type.clone(),
        store_import: config.store_import.clone(),
        schema_entities: config.schema_entities.clone(),
        pagination: config.pagination.clone(),
        pool_extra_roots: config.pool_extra_roots.clone(),
    };

    generate_clients(&internal).map(|_| ()).map_err(CodegenError::Server)
}

/// Run the client-side generation pipeline.
///
/// Parses API modules from `config.api_dir`, emits the schema-known
/// TypeScript aliases to every distinct `bindings_path`, optionally runs
/// [`ontogen_ts`] to append the long-tail closure, then dispatches to
/// each configured client generator.
fn generate_clients(config: &config::Config) -> Result<Vec<ApiModule>, String> {
    if !config.api_dir.exists() {
        return Err(format!("API directory does not exist: {}", config.api_dir.display()));
    }

    let scanned = parse::scan_api_dir(&config.api_dir, &config.state_type, config.store_type.as_deref());

    for record in &scanned.skips {
        println!("cargo:warning={record}");
    }

    let mut modules = scanned.modules;
    parse::apply_singleton_overlay(&mut modules, &config.naming);
    parse::apply_command_overrides(&mut modules, &config.naming);
    if modules.is_empty() {
        return Ok(modules);
    }

    // OF-014 spike (option 1 half): emit schema-known TS aliases (entities +
    // generated DTOs) to every distinct `bindings_path` declared by client
    // generators *before* the client generators run. They then partition
    // referenced types into schema-known (now satisfied) vs long-tail.
    let mut written_bindings: std::collections::HashSet<std::path::PathBuf> = Default::default();
    for generator in &config.generators {
        let bp = match generator {
            ClientGenerator::HttpTs { bindings_path, .. }
            | ClientGenerator::HttpTauriIpcSplit { bindings_path, .. } => Some(bindings_path.clone()),
            ClientGenerator::AdminRegistry { .. } => None,
        };
        if let Some(path) = bp
            && written_bindings.insert(path.clone())
        {
            let body = generators::ts_bindings::emit(&config.schema_entities);
            crate::write_and_format_ts(&path, body).expect("Failed to write schema-known bindings");
        }
    }

    // OF-015 (PR 5+6): long-tail TS emission via the `ontogen-ts` crate.
    // For the long tail, scan the user crate's `src/` into a type pool,
    // resolve each long-tail name to a `TypePath`, and call
    // `ontogen_ts::emit` to produce the TS for the reachable closure.
    // Append the result to every bindings.ts. The walker reads `syn::Item`
    // directly — no cargo invocation, no side-car binary, no target-dir
    // contention, no recursion guard.
    let long_tail = generators::ts_bindings::long_tail(&modules, config, &config.schema_entities);
    if !long_tail.is_empty() && !written_bindings.is_empty() {
        let manifest_dir = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR")
                .map_err(|_| "CARGO_MANIFEST_DIR not set; ontogen-ts only runs from a build script")?,
        );
        let src_dir = manifest_dir.join("src");

        // 1. Build the type pool from src/, then merge in any configured
        //    extra source roots (workspace-sibling crates the consuming crate
        //    re-exports types from). Main pool wins on key collision so the
        //    consuming crate's own definitions take precedence over a sibling
        //    that happens to share a module path.
        let mut pool = ontogen_ts::scan_src_dir(&src_dir).map_err(|e| format!("ontogen-ts pool scan failed: {e}"))?;
        for extra in &config.pool_extra_roots {
            let resolved = if extra.is_absolute() { extra.clone() } else { manifest_dir.join(extra) };
            let sibling = ontogen_ts::scan_src_dir(&resolved)
                .map_err(|e| format!("ontogen-ts pool scan failed for extra root `{}`: {e}", resolved.display()))?;
            for (key, item) in sibling {
                pool.entry(key).or_insert(item);
            }
        }

        // 2. Resolve each long-tail name to a TypePath. Try a single-
        //    segment match first (matches items defined in src/lib.rs);
        //    fall back to any pool entry whose terminal segment matches
        //    (covers items in nested modules).
        let mut roots: Vec<ontogen_ts::TypePath> = Vec::with_capacity(long_tail.len());
        let mut missing: Vec<String> = Vec::new();
        for name in &long_tail {
            let bare = ontogen_ts::TypePath::new(vec![name.clone()]).expect("long_tail names are non-empty idents");
            if pool.contains_key(&bare) {
                roots.push(bare);
            } else if let Some(matched) = pool.keys().find(|p| p.terminal() == name.as_str()).cloned() {
                roots.push(matched);
            } else {
                missing.push(name.clone());
            }
        }
        if !missing.is_empty() {
            for name in &missing {
                println!("cargo:warning=ontogen-ts: long-tail type `{name}` not found in `{}`", src_dir.display());
            }
            return Err(format!("ontogen-ts: {} long-tail type(s) not found in pool", missing.len()));
        }

        // 3. Emit. Surface every error before failing so the build shows
        //    the full punch-list, not just the first issue.
        let emit_config = ontogen_ts::EmitConfig::default();
        let ts = match ontogen_ts::emit(&roots, &pool, &emit_config) {
            Ok(ts) => ts,
            Err(errors) => {
                for e in &errors {
                    println!("cargo:warning=ontogen-ts: {e}");
                }
                return Err(format!("ontogen-ts emit failed with {} error(s)", errors.len()));
            }
        };

        // 4. Append to every bindings file written above.
        for path in &written_bindings {
            append_long_tail_to_bindings(path, &ts)?;
        }

        // 5. Tell cargo to rerun if any .rs under src/ changes. Coarse but
        //    correct — the pool's reach-set is a subset of src/, so this
        //    over-includes but never misses.
        rerun_if_changed_under(&src_dir);
    }

    for generator in &config.generators {
        match generator {
            ClientGenerator::HttpTs { output, bindings_path } => {
                let fallbacks = generators::ts_client::generate(output, bindings_path, &modules, config);
                for record in &fallbacks {
                    println!("cargo:warning={record}");
                }
            }
            ClientGenerator::HttpTauriIpcSplit { output, bindings_path } => {
                let fallbacks = generators::transport::generate(output, bindings_path, &modules, config);
                for record in &fallbacks {
                    println!("cargo:warning={record}");
                }
            }
            ClientGenerator::AdminRegistry { output } => {
                generators::admin::generate(output, &modules, config);
            }
        }
    }

    Ok(modules)
}

/// Append `ts` to the bindings file at `bindings_path`, prefixed with a
/// short comment that identifies the source. Creates the file if missing
/// (the schema-known emitter writes it first, but be defensive).
fn append_long_tail_to_bindings(bindings_path: &std::path::Path, ts: &str) -> Result<(), String> {
    let mut existing = std::fs::read_to_string(bindings_path).unwrap_or_default();
    existing.push_str("\n// Long-tail types (emitted via ontogen-ts AST walker).\n");
    existing.push_str(ts);
    if !existing.ends_with('\n') {
        existing.push('\n');
    }
    std::fs::write(bindings_path, existing).map_err(|e| format!("failed to append to {}: {e}", bindings_path.display()))
}

/// Emit `cargo:rerun-if-changed=<path>` for every `.rs` file recursively
/// under `dir`. Coarser than reading exactly the file set the pool walker
/// touched, but correct — the reach-set is a subset of the file set, so
/// this over-includes (extra rebuilds for unused files) but never misses
/// (no stale bindings.ts after a real change).
fn rerun_if_changed_under(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rerun_if_changed_under(&path);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
