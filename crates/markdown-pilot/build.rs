//! Runs the full markdown pipeline at build time: schema → markdown_io →
//! dtos → store → api → HTTP transport. The backend is inferred (markdown is
//! the only persistence stage). Generated code is written into src/ and
//! committed, iron-log-style, so diffs are reviewable.

use ontogen::ServersConfig;
use ontogen::servers::{NamingConfig, ServerGenerator};

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
    println!("cargo:rerun-if-changed=src/api/task.rs");

    // HTTP transport over the API layer: proves in root CI that the emitted
    // axum handlers compile and the router builds against the axum version
    // in Cargo.toml (see tests/http_router.rs). `src/api` holds hand-written
    // modules and the api stage's `generated/`; the scan reads both (and
    // nothing below `transport/` but its `mod.rs`, which it skips).
    let servers_config = ServersConfig {
        api_dir: "src/api".into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators: vec![ServerGenerator::HttpAxum { output: "src/api/transport/http/generated.rs".into() }],
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        // Every module paginates (a primary surface's pagination covers all
        // of its modules), so the router test drives the page links live.
        pagination: Some(ontogen::servers::PaginationConfig { default_limit: 2, max_limit: 3 }),
        extra_surfaces: vec![],
        // The pipeline scans its schema directory for `AppError`.
        error_source_dir: None,
    };

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
        .servers(servers_config)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
