//! Runs the whole pipeline over a schema whose names collide with what the
//! generated code imports. Generated output is committed so a regression
//! shows up as a reviewable diff as well as a compile or tsc failure.

use std::path::PathBuf;

use ontogen::clients::ClientGenerator;
use ontogen::servers::{NamingConfig, PaginationConfig, ServerGenerator};
use ontogen::{ClientsConfig, IdStrategy, MarkdownIoOptions, MarkdownLayout, OkfOptions, Pipeline, ServersConfig};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/schema");

    // Pagination is on so the paged helpers are emitted too.
    let pagination = Some(PaginationConfig { default_limit: 10, max_limit: 50 });

    let servers_config = ServersConfig {
        api_dir: "src/api".into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api".into(),
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
        pagination: pagination.clone(),
        extra_surfaces: vec![],
        error_source_dir: None,
    };

    let clients_config = ClientsConfig {
        generators: vec![
            ClientGenerator::HttpTs {
                output: "generated-ts/http.ts".into(),
                bindings_path: "generated-ts/types.ts".into(),
            },
            ClientGenerator::HttpTauriIpcSplit {
                output: "generated-ts/transport.ts".into(),
                bindings_path: "generated-ts/types.ts".into(),
            },
            ClientGenerator::AdminRegistry { output: "generated-ts/admin-registry.ts".into() },
        ],
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination,
        ..ClientsConfig::new("src/api", "AppState", "crate::api", "crate::schema", "crate::AppState")
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
        .store_id_strategy(IdStrategy::SlugFromField("title".into()))
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<PathBuf>("src/store/hooks".into()))
        .api("src/api/generated", "AppState")
        .api_paginated(
            [
                "doc",
                "document",
                "endpoint",
                "event",
                "links",
                "map",
                "match",
                "method",
                "relationship",
                "request",
                "response",
                "state",
                "value",
                "path",
                "serde",
                "axum",
                "markdown_store",
            ]
            .map(String::from)
            .to_vec(),
        )
        .servers(servers_config)
        .clients(clients_config)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
