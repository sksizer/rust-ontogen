use ontogen::servers::config::{ClientGenerator, Config, GeneratorConfig, ServerGenerator};
use ontogen::servers::types::NamingConfig;

fn main() {
    // ── Stage 1: Parse schema ────────────────────────────────────────────
    let schema = ontogen::parse_schema(&ontogen::SchemaConfig {
        schema_dir: "src/schema".into(),
    })
    .expect("Failed to parse schema");

    // ── Stage 2: Generate SeaORM entities ────────────────────────────────
    let seaorm = ontogen::gen_seaorm(
        &schema.entities,
        &ontogen::SeaOrmConfig {
            entity_output: "src/persistence/db/entities/generated".into(),
            conversion_output: "src/persistence/db/conversions/generated".into(),
            skip_conversions: vec![],
        },
    )
    .expect("Failed to generate SeaORM entities");

    // ── Stage 3: Generate DTOs ───────────────────────────────────────────
    ontogen::gen_dtos(
        &schema.entities,
        &ontogen::DtoConfig {
            output_dir: "src/schema/dto".into(),
        },
    )
    .expect("Failed to generate DTOs");

    // ── Stage 4: Generate Store layer ────────────────────────────────────
    ontogen::gen_store(
        &schema.entities,
        Some(&seaorm),
        &ontogen::StoreConfig {
            output_dir: "src/store/generated".into(),
            hooks_dir: Some("src/store/hooks".into()),
        },
    )
    .expect("Failed to generate store");

    // ── Stage 5: Generate API layer ──────────────────────────────────────
    ontogen::gen_api(
        &schema.entities,
        &ontogen::ApiConfig {
            output_dir: "src/api/v1/generated".into(),
            exclude: vec![],
            scan_dirs: vec![],
            state_type: "AppState".to_string(),
            store_type: Some("Store".to_string()),
        },
    )
    .expect("Failed to generate API");

    // ── Stage 6: Generate servers + clients ──────────────────────────────
    // Note: gen_clients is currently a no-op. Client generation is handled
    // inline by servers::generate_transport. See ontogen/src/clients/mod.rs.
    let config = Config {
        api_dir: "src/api/v1".into(),
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming: NamingConfig::default(),
        generators: vec![
            GeneratorConfig::Server(ServerGenerator::HttpAxum {
                output: "src/api/transport/http/generated.rs".into(),
            }),
            GeneratorConfig::Server(ServerGenerator::TauriIpc {
                output: "src/api/transport/ipc/generated.rs".into(),
            }),
            GeneratorConfig::Client(ClientGenerator::HttpTauriIpcSplit {
                output: "../src-nuxt/app/generated/transport.ts".into(),
                bindings_path: "../src-nuxt/app/generated/types.ts".into(),
            }),
        ],
        rustfmt_edition: "2024".to_string(),
        sse_route_overrides: Default::default(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
    };

    ontogen::servers::generate_transport(&config).expect("Failed to generate transports");

    tauri_build::build();
}
