//! iron-log's generator set, markdown-backed. The byte-identical-downstream
//! invariant means everything from `api` outward is exactly what iron-log's
//! pipeline produces for the same schema — `diff -r` the two apps' generated
//! `api/v1` trees to see it (the mechanical proof lives in the root
//! workspace's tests/backend_parity.rs).
//!
//! One declared omission vs iron-log: the TauriIpc transport. Compiling the
//! Tauri stack for a headless demo buys nothing — the transport generators
//! are exercised by iron-log itself.

use std::path::PathBuf;

use ontogen::clients::ClientGenerator;
use ontogen::servers::{NamingConfig, ServerGenerator};
use ontogen::{ClientsConfig, IdStrategy, MarkdownIoOptions, MarkdownLayout, Pipeline, ServersConfig};

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
    };

    let clients_config = ClientsConfig {
        api_dir: "src/api/v1".into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api::v1".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators: vec![
            ClientGenerator::HttpTauriIpcSplit {
                output: "generated-ts/transport.ts".into(),
                bindings_path: "generated-ts/types.ts".into(),
            },
            ClientGenerator::AdminRegistry { output: "generated-ts/admin-registry.ts".into() },
        ],
        sse_route_overrides: Default::default(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: None,
        schema_entities: Vec::new(),
        pool_extra_roots: Vec::new(),
        pool_exclude_paths: Vec::new(),
    };

    Pipeline::new("src/schema")
        .markdown_io(
            "src/persistence/markdown/generated",
            MarkdownIoOptions {
                vault_root: "data/vault".into(),
                layout: MarkdownLayout::PerEntityDir,
                // Workout.name is Option<String>, so SlugFromField is out;
                // Provided keeps the example honest about where ids come from.
                id_strategy: IdStrategy::Provided,
                list_cap: 10_000,
            },
        )
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<PathBuf>("src/store/hooks".into()))
        .api("src/api/v1/generated", "AppState")
        .servers(servers_config)
        .clients(clients_config)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
