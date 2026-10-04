//! Runs the full markdown pipeline at build time: schema → markdown_io →
//! dtos → store → api → HTTP transport. The backend is inferred (markdown is
//! the only persistence stage). Generated code is written into src/ and
//! committed, iron-log-style, so diffs are reviewable.

use ontogen::ServersConfig;
use ontogen::servers::{NamingConfig, PaginationConfig, PrefixParam, RoutePrefix, ServerGenerator};

/// The transports over the API layer: proves in root CI that the emitted
/// axum handlers compile and the router builds against the axum version in
/// Cargo.toml (see tests/http_router.rs), and that the MCP tool registry
/// compiles and serves each op kind (tests/mcp_tools.rs). `src/api` holds hand-written
/// modules and the api stage's `generated/`; the scan reads both (and
/// nothing below `transport/` but its `mod.rs`, which it skips).
fn servers_config(generators: Vec<ServerGenerator>, route_prefix: Option<RoutePrefix>) -> ServersConfig {
    ServersConfig {
        api_dir: "src/api".into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators,
        sse_route_overrides: Default::default(),
        route_prefix,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        // Every module paginates (a primary surface's pagination covers all
        // of its modules), so the router test drives the page links live.
        pagination: Some(PaginationConfig { default_limit: 2, max_limit: 3 }),
        extra_surfaces: vec![],
        // Where `AppError` is declared. The pipeline would default to its
        // schema directory, but the scoped call below runs outside it.
        error_source_dir: Some("src/schema".into()),
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/schema/note.rs");
    println!("cargo:rerun-if-changed=src/schema/section.rs");
    println!("cargo:rerun-if-changed=src/schema/task.rs");
    println!("cargo:rerun-if-changed=src/schema/tag.rs");
    println!("cargo:rerun-if-changed=src/schema/bookmark.rs");
    println!("cargo:rerun-if-changed=src/schema/task_summary.rs");
    println!("cargo:rerun-if-changed=src/schema/mod.rs");
    // The hand-written API modules the servers stage scans beside generated/.
    println!("cargo:rerun-if-changed=src/api/bookmark.rs");
    println!("cargo:rerun-if-changed=src/api/outline.rs");
    println!("cargo:rerun-if-changed=src/api/section.rs");
    println!("cargo:rerun-if-changed=src/api/tag.rs");
    println!("cargo:rerun-if-changed=src/api/task.rs");

    ontogen::Pipeline::new("src/schema")
        .markdown_io(
            "src/persistence/markdown/generated",
            ontogen::MarkdownIoOptions {
                vault_root: "data/vault".into(),
                layout: ontogen::MarkdownLayout::PerEntityDir,
                list_cap: 10_000,
                // Knobs off: the pilot pins the default vault.
                okf: ontogen::OkfOptions::default(),
            },
        )
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<std::path::PathBuf>("src/store/hooks".into()))
        .store_id_strategy(ontogen::IdStrategy::SlugFromField("title".into()))
        .api("src/api/generated", "AppState")
        .api_paginated(vec!["note".into(), "section".into(), "tag".into(), "task".into()])
        .servers(servers_config(
            vec![
                ServerGenerator::HttpAxum { output: "src/api/transport/http/generated.rs".into() },
                ServerGenerator::Mcp { output: "src/api/transport/mcp/generated.rs".into() },
            ],
            None,
        ))
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));

    // The same API under a route prefix, generated after the pipeline has
    // written the `generated/` modules it scans. The scoped router is
    // compiled and driven in CI too (tests/http_router.rs), so a scoped
    // route that drifts from its unscoped twin fails there. MCP is generated
    // unscoped only: a scoped tool parses its scope argument as a UUID, and
    // the pilot's one project is a name.
    let prefix = RoutePrefix {
        segments: "projects/:project_id".into(),
        state_accessor: "store_for".into(),
        params: vec![PrefixParam { name: "project_id".into(), rust_type: "String".into(), ts_type: "string".into() }],
    };
    let schema = ontogen::parse_schema(&ontogen::SchemaConfig { schema_dir: "src/schema".into() })
        .unwrap_or_else(|e| panic!("ontogen schema parse failed: {e}"));
    ontogen::gen_servers(
        &schema.entities,
        None,
        &[],
        &servers_config(
            vec![ServerGenerator::HttpAxum { output: "src/api/transport/http_scoped/generated.rs".into() }],
            Some(prefix),
        ),
    )
    .unwrap_or_else(|e| panic!("ontogen scoped servers failed: {e}"));
}
