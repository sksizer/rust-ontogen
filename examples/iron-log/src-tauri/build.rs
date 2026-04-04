use ontogen::servers::config::{ClientGenerator, Config, GeneratorConfig, ServerGenerator};
use ontogen::servers::types::NamingConfig;
use ontogen::CodegenError;

/// Unwrap a codegen result, emitting a cargo:warning before panicking on error.
fn unwrap_codegen<T>(result: Result<T, CodegenError>, stage: &str) -> T {
    result.unwrap_or_else(|e| {
        e.emit_cargo_warning();
        panic!("{stage}: {e}");
    })
}

fn main() {
    // ── Rerun directives ────────────────────────────────────────────────
    println!("cargo:rerun-if-changed=build.rs");
    ontogen::emit_rerun_directives(&std::path::PathBuf::from("src/schema"));
    ontogen::emit_rerun_directives_excluding(&std::path::PathBuf::from("src/api/v1"), &["generated"]);

    // ── Stage 1: Parse schema ────────────────────────────────────────────
    let schema = unwrap_codegen(
        ontogen::parse_schema(&ontogen::SchemaConfig {
            schema_dir: "src/schema".into(),
        }),
        "Stage 1: parse schema",
    );

    // ── Stage 2: Generate SeaORM entities ────────────────────────────────
    let seaorm = unwrap_codegen(
        ontogen::gen_seaorm(
            &schema.entities,
            &ontogen::SeaOrmConfig {
                entity_output: "src/persistence/db/entities/generated".into(),
                conversion_output: "src/persistence/db/conversions/generated".into(),
                skip_conversions: vec![],
            },
        ),
        "Stage 2: generate SeaORM entities",
    );

    // ── Stage 3: Generate DTOs ───────────────────────────────────────────
    unwrap_codegen(
        ontogen::gen_dtos(
            &schema.entities,
            &ontogen::DtoConfig {
                output_dir: "src/schema/dto".into(),
            },
        ),
        "Stage 3: generate DTOs",
    );

    // ── Stage 4: Generate Store layer ────────────────────────────────────
    unwrap_codegen(
        ontogen::gen_store(
            &schema.entities,
            Some(&seaorm),
            &ontogen::StoreConfig {
                output_dir: "src/store/generated".into(),
                hooks_dir: Some("src/store/hooks".into()),
            },
        ),
        "Stage 4: generate store",
    );

    // ── Stage 5: Generate API layer ──────────────────────────────────────
    unwrap_codegen(
        ontogen::gen_api(
            &schema.entities,
            &ontogen::ApiConfig {
                output_dir: "src/api/v1/generated".into(),
                exclude: vec![],
                scan_dirs: vec![],
                state_type: "AppState".to_string(),
                store_type: Some("Store".to_string()),
            },
        ),
        "Stage 5: generate API",
    );

    // ── Stage 6: Generate servers + clients ──────────────────────────────
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
            GeneratorConfig::Client(ClientGenerator::AdminRegistry {
                output: "../src-nuxt/app/admin/generated/admin-registry.ts".into(),
            }),
        ],
        rustfmt_edition: "2024".to_string(),
        sse_route_overrides: Default::default(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        schema_entities: schema.entities.clone(),
    };

    ontogen::servers::generate_transport(&config)
        .expect("Stage 6: failed to generate transports");

    tauri_build::build();
}
