use std::path::PathBuf;

use ontogen::servers::{ClientGenerator, NamingConfig, ServerGenerator};
use ontogen::{Pipeline, ServersConfig};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    ontogen::emit_rerun_directives_excluding(
        &PathBuf::from("src/api/v1"),
        &["generated"],
    );

    let servers_config = ServersConfig {
        api_dir: "src/api/v1".into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api::v1".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators: vec![
            ServerGenerator::HttpAxum {
                output: "src/api/transport/http/generated.rs".into(),
            },
            ServerGenerator::TauriIpc {
                output: "src/api/transport/ipc/generated.rs".into(),
            },
        ],
        client_generators: vec![
            ClientGenerator::HttpTauriIpcSplit {
                output: "../src-nuxt/app/generated/transport.ts".into(),
                bindings_path: "../src-nuxt/app/generated/types.ts".into(),
            },
            ClientGenerator::AdminRegistry {
                output: "../src-nuxt/app/admin/generated/admin-registry.ts".into(),
            },
        ],
        rustfmt_edition: "2024".into(),
        sse_route_overrides: Default::default(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: None,
        schema_entities: Vec::new(),
    };

    Pipeline::new("src/schema")
        .seaorm(
            "src/persistence/db/entities/generated",
            "src/persistence/db/conversions/generated",
        )
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<PathBuf>("src/store/hooks".into()))
        .api("src/api/v1/generated", "AppState")
        .servers(servers_config)
        .build()
        .unwrap_or_else(|e| {
            e.emit_cargo_warning();
            panic!("ontogen pipeline failed: {e}");
        });

    tauri_build::build();
}
