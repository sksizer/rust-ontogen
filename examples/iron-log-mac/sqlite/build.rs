//! The shared training-log schema on the SeaORM (SQLite) backend, exported
//! through the native (UniFFI) transport.

use std::path::PathBuf;

use ontogen::servers::{NamingConfig, ServerGenerator};
use ontogen::{IdStrategy, Pipeline, ServersConfig};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../shared");

    let servers_config = ServersConfig {
        api_dir: "src/api/v1".into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api::v1".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        // EXPERIMENTAL: the UniFFI export the Swift app links.
        generators: vec![ServerGenerator::Uniffi { output: "src/native/generated.rs".into() }],
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: None,
        extra_surfaces: vec![],
        // The pipeline scans its schema directory for `AppError`.
        error_source_dir: None,
    };

    Pipeline::new("src/schema")
        .seaorm("src/persistence/db/entities/generated", "src/persistence/db/conversions/generated")
        .store_id_strategy(IdStrategy::Provided)
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<PathBuf>("src/store/hooks".into()))
        .api("src/api/v1/generated", "AppState")
        .servers(servers_config)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
