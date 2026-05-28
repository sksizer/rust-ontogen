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

/// Render the schema-known TypeScript surface (entity types + generated
/// Create/Update DTOs) for the given entities.
///
/// This is a thin pass-through to the crate-internal
/// `generators::ts_bindings::emit`, exposed publicly so integration tests
/// (in `tests/`) can assert against the body the schema-known emitter
/// writes to `bindings.ts` without going through the full
/// [`generate`] pipeline. The crate-internal module stays
/// `pub(crate)` — this helper is the only public surface area for the
/// schema-known render.
#[doc(hidden)]
#[must_use]
pub fn emit_schema_known_ts_for_tests(entities: &[ontogen_core::model::EntityDef]) -> String {
    generators::ts_bindings::emit(entities)
}

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
        pool_exclude_paths: config.pool_exclude_paths.clone(),
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
        let (mut pool, mut imports) =
            ontogen_ts::scan_src_dir_with_imports(&src_dir).map_err(|e| format!("ontogen-ts pool scan failed: {e}"))?;
        for extra in &config.pool_extra_roots {
            let resolved = if extra.is_absolute() { extra.clone() } else { manifest_dir.join(extra) };
            let (sibling, sibling_imports) = ontogen_ts::scan_src_dir_with_imports(&resolved)
                .map_err(|e| format!("ontogen-ts pool scan failed for extra root `{}`: {e}", resolved.display()))?;
            for (key, item) in sibling {
                pool.entry(key).or_insert(item);
            }
            // Main root's `use` tables win on a module-path collision, same as
            // the pool above.
            imports.merge(sibling_imports);
        }

        // 1b. Drop pool entries that fall under any configured exclude path.
        //
        //     Each `pool_exclude_paths` entry is interpreted as a filesystem
        //     directory rooted at `CARGO_MANIFEST_DIR` (relative paths are
        //     joined; absolute paths are used as-is). We canonicalise once and
        //     translate the path to the module-segment prefix it corresponds
        //     to under `<manifest>/src/`, then strip every pool key whose
        //     segments start with that prefix.
        //
        //     Canonical use: ontogen's own `gen_seaorm` output. SeaORM emits
        //     a `Relation` enum per entity by convention; in a project with
        //     many entities those enums collide with any domain type also
        //     named `Relation` and the resolver below reports Ambiguous.
        //     Excluding the seaorm output directory removes the noise without
        //     touching the consumer's types.
        if !config.pool_exclude_paths.is_empty() {
            let src_canon = std::fs::canonicalize(&src_dir).unwrap_or_else(|_| src_dir.clone());
            let mut exclude_prefixes: Vec<Vec<String>> = Vec::new();
            for excl in &config.pool_exclude_paths {
                let resolved = if excl.is_absolute() { excl.clone() } else { manifest_dir.join(excl) };
                let abs = std::fs::canonicalize(&resolved).unwrap_or(resolved);
                let Ok(rel) = abs.strip_prefix(&src_canon) else {
                    println!(
                        "cargo:warning=ontogen-ts: pool_exclude_paths entry `{}` is not under \
                         `{}/src/` after canonicalisation; ignoring",
                        abs.display(),
                        manifest_dir.display(),
                    );
                    continue;
                };
                let segments: Vec<String> = rel
                    .components()
                    .filter_map(|c| match c {
                        std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
                        _ => None,
                    })
                    .collect();
                if !segments.is_empty() {
                    exclude_prefixes.push(segments);
                }
            }
            if !exclude_prefixes.is_empty() {
                pool.retain(|tp, _| {
                    let segs = tp.segments();
                    !exclude_prefixes.iter().any(|prefix| segs.len() >= prefix.len() && &segs[..prefix.len()] == prefix.as_slice())
                });
            }
        }

        // 2. Resolve each long-tail name to a TypePath via ontogen-ts's
        //    import-aware resolver. A bare name (`VaultConfig`) is resolved
        //    through the `use` table of the module that referenced it —
        //    following re-export chains — so it lands on the type the source
        //    actually meant, even when two crates define the same name. The
        //    resolver only reports `Ambiguous` when nothing disambiguates; we
        //    error in that case rather than guess.
        //
        //    To resolve through the right `use` table we need each referenced
        //    name's *referencing module*. For API-surface names that's the API
        //    file's module path relative to the scanned `src/` (e.g.
        //    `src/api/v1/vault.rs` → `api::v1::vault`). Names with no API
        //    referencing site (e.g. schema entity-field types) resolve with no
        //    module hint — same-module + unique-terminal, which suffices for
        //    those.
        let api_prefix: Vec<String> = config
            .api_dir
            .canonicalize()
            .ok()
            .zip(src_dir.canonicalize().ok())
            .and_then(|(api, src)| api.strip_prefix(&src).ok().map(|rel| rel.to_path_buf()))
            .map(|rel| rel.components().filter_map(|c| c.as_os_str().to_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let mut name_module: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for m in &modules {
            let mut module_path = api_prefix.clone();
            module_path.push(m.name.clone());
            for name in generators::ts_bindings::module_referenced_ts_types(m, config) {
                name_module.entry(name).or_insert_with(|| module_path.clone());
            }
        }

        let mut roots: Vec<ontogen_ts::TypePath> = Vec::with_capacity(long_tail.len());
        let mut missing: Vec<String> = Vec::new();
        let mut ambiguous: Vec<(String, Vec<ontogen_ts::TypePath>)> = Vec::new();
        for name in &long_tail {
            let module: &[String] = name_module.get(name).map_or(&[], Vec::as_slice);
            match ontogen_ts::resolve_reference(std::slice::from_ref(name), module, &pool, &imports) {
                ontogen_ts::Resolution::Resolved(key) => roots.push(key),
                ontogen_ts::Resolution::NotInPool => missing.push(name.clone()),
                ontogen_ts::Resolution::Ambiguous(candidates) => ambiguous.push((name.clone(), candidates)),
            }
        }
        if !missing.is_empty() {
            for name in &missing {
                println!("cargo:warning=ontogen-ts: long-tail type `{name}` not found in `{}`", src_dir.display());
            }
            return Err(format!("ontogen-ts: {} long-tail type(s) not found in pool", missing.len()));
        }
        if !ambiguous.is_empty() {
            for (name, candidates) in &ambiguous {
                let rendered: Vec<String> = candidates.iter().map(|p| p.to_string()).collect();
                println!(
                    "cargo:warning=ontogen-ts: long-tail type `{name}` is ambiguous — multiple pool entries share \
                     that name: {}; qualify it or rename one",
                    rendered.join(", ")
                );
            }
            return Err(format!("ontogen-ts: {} ambiguous long-tail type name(s)", ambiguous.len()));
        }

        // 3. Emit. Surface every error before failing so the build shows
        //    the full punch-list, not just the first issue. Pass the
        //    per-module `use` tables so bare field-type references resolve
        //    through their actual imports (including re-export chains).
        let emit_config = ontogen_ts::EmitConfig::default();
        let ts = match ontogen_ts::emit_with_imports(&roots, &pool, &imports, &emit_config) {
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
