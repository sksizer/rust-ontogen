//! Planning tracker pipeline: markdown vault → store → api → HTTP + MCP.

use std::path::PathBuf;

use ontogen::servers::{NamingConfig, PaginationConfig, ServerGenerator};
use ontogen::{IdStrategy, MarkdownIoOptions, MarkdownLayout, OkfOptions, Pipeline, ServersConfig};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    ontogen::emit_rerun_directives_excluding(&PathBuf::from("src/api/v1"), &["generated"]);

    let servers_config = ServersConfig {
        api_dir: "src/api/v1".into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api::v1".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators: vec![
            ServerGenerator::HttpAxum { output: "src/api/transport/http/generated.rs".into() },
            ServerGenerator::Mcp { output: "src/api/transport/mcp/generated.rs".into() },
        ],
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        // A primary surface's pagination covers every one of its modules, so
        // all three lists page, and the api stage gives each a paged list.
        pagination: Some(PaginationConfig { default_limit: 20, max_limit: 100 }),
        extra_surfaces: vec![],
        // The pipeline scans its schema directory for `AppError`.
        error_source_dir: None,
    };

    Pipeline::new("src/schema")
        .markdown_io(
            "src/persistence/markdown/generated",
            MarkdownIoOptions {
                vault_root: "data/vault".into(),
                layout: MarkdownLayout::PerEntityDir,
                list_cap: 10_000,
                okf: OkfOptions::default(),
            },
        )
        // Every entity has a title: created tasks/epics/tags slug
        // their ids from it (POST without an id and watch the
        // filename appear).
        .store_id_strategy(IdStrategy::SlugFromField("title".into()))
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<PathBuf>("src/store/hooks".into()))
        .api("src/api/v1/generated", "AppState")
        .api_paginated(vec!["task".into(), "epic".into(), "tag".into()])
        .servers(servers_config)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
