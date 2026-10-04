//! notes-kb pipeline: markdown vault → store → api → HTTP + TS client.

use std::path::PathBuf;

use ontogen::clients::ClientGenerator;
use ontogen::servers::{NamingConfig, ServerGenerator};
use ontogen::{ClientsConfig, IdStrategy, MarkdownIoOptions, MarkdownLayout, OkfOptions, Pipeline, ServersConfig};

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
        generators: vec![ServerGenerator::HttpAxum { output: "src/api/transport/http/generated.rs".into() }],
        rustfmt_edition: "2024".into(),
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: None,
        extra_surfaces: vec![],
        // The pipeline scans its schema directory for `AppError`.
        error_source_dir: None,
    };

    let clients_config = ClientsConfig {
        generators: vec![ClientGenerator::HttpTauriIpcSplit {
            output: "generated-ts/transport.ts".into(),
            bindings_path: "generated-ts/types.ts".into(),
        }],
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        ..ClientsConfig::new("src/api/v1", "AppState", "crate::api::v1", "crate::schema", "crate::AppState")
    };

    Pipeline::new("src/schema")
        .markdown_io(
            "src/persistence/markdown/generated",
            MarkdownIoOptions {
                vault_root: "data/vault".into(),
                layout: MarkdownLayout::PerEntityDir,
                list_cap: 10_000,
                // An OKF-navigable vault: an index.md in every directory,
                // and each record names the notes-kb release that last
                // wrote it.
                okf: OkfOptions {
                    index: true,
                    generated_by: Some(format!("notes-kb/{}", env!("CARGO_PKG_VERSION"))),
                },
            },
        )
        .store_id_strategy(IdStrategy::SlugFromField("title".into()))
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<PathBuf>("src/store/hooks".into()))
        .api("src/api/v1/generated", "AppState")
        .servers(servers_config)
        .clients(clients_config)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
