//! Runs the full markdown pipeline at build time: schema → markdown_io →
//! dtos → store → api → HTTP transport. The backend is inferred (markdown is
//! the only persistence stage). Generated code is written into src/ and
//! committed, iron-log-style, so diffs are reviewable.

use ontogen::ServersConfig;
use ontogen::servers::{NamingConfig, ServerGenerator};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/schema/note.rs");
    println!("cargo:rerun-if-changed=src/schema/task.rs");
    println!("cargo:rerun-if-changed=src/schema/tag.rs");

    // HTTP transport over the generated service layer: proves in root CI
    // that the emitted axum handlers compile and the router builds against
    // the axum version in Cargo.toml (see tests/http_router.rs).
    let servers_config = ServersConfig {
        api_dir: "src/api/generated".into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators: vec![ServerGenerator::HttpAxum { output: "src/api/transport/http/generated.rs".into() }],
        rustfmt_edition: "2024".into(),
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: None,
    };

    ontogen::Pipeline::new("src/schema")
        .markdown_io(
            "src/persistence/markdown/generated",
            ontogen::MarkdownIoOptions {
                vault_root: "data/vault".into(),
                layout: ontogen::MarkdownLayout::PerEntityDir,
                id_strategy: ontogen::IdStrategy::SlugFromField("title".into()),
                list_cap: 10_000,
            },
        )
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<std::path::PathBuf>("src/store/hooks".into()))
        .api("src/api/generated", "AppState")
        .servers(servers_config)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
