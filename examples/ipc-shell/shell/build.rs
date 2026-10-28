use ontogen::servers::{ImportPaths, NamingConfig, ServerGenerator};
use ontogen::{ServersConfig, gen_servers};

fn main() {
    // The handlers live in the API crate; scan its sources, and rebuild when they change.
    let api_dir = "../api/src/api/v1";
    println!("cargo:rerun-if-changed=build.rs");
    ontogen::emit_rerun_directives_excluding(&std::path::PathBuf::from(api_dir), &["generated"]);

    // An IPC-only servers pass: no schema, store, DTO or client stages. The
    // shared paths below describe the API crate as the shell sees it; the
    // generator's own imports override the ones that must name that crate.
    let config = ServersConfig {
        api_dir: api_dir.into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api::v1".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators: vec![ServerGenerator::TauriIpcWithImports {
            output: "src/ipc/generated.rs".into(),
            imports: ImportPaths {
                service_import_path: Some("ipc_shell_api::api::v1".into()),
                types_import_path: Some("ipc_shell_api::schema".into()),
                state_import: Some("ipc_shell_api::AppState".into()),
                store_import: Some("ipc_shell_api::store::Store".into()),
            },
        }],
        rustfmt_edition: "2024".into(),
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: None,
        extra_surfaces: vec![],
    };

    gen_servers(None, &[], &config).unwrap_or_else(|e| {
        e.emit_cargo_warning();
        panic!("ontogen servers pass failed: {e}");
    });
}
