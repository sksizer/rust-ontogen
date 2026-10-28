use ontogen::servers::{NamingConfig, ServerGenerator};
use ontogen::{SchemaOutput, ServersConfig, gen_servers};

fn main() {
    // The handlers live in the API crate; scan its sources, and rebuild when they change.
    let api_dir = "../api/src/api/v1";
    println!("cargo:rerun-if-changed=build.rs");
    ontogen::emit_rerun_directives_excluding(&std::path::PathBuf::from(api_dir), &["generated"]);

    // An IPC-only servers pass: no schema, store, DTO or client stages. The
    // paths below describe the API crate from inside it, as its own build
    // would; `TauriIpcInCrate` re-roots every `crate::` at `ipc_shell_api`.
    let config = ServersConfig {
        api_dir: api_dir.into(),
        state_type: "AppState".into(),
        service_import_path: "crate::api::v1".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators: vec![ServerGenerator::TauriIpcInCrate {
            output: "src/ipc/generated.rs".into(),
            api_crate: "ipc_shell_api".into(),
        }],
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: None,
        extra_surfaces: vec![],
        error_source_dir: None,
    };

    gen_servers(&SchemaOutput::default(), None, &[], &config).unwrap_or_else(|e| {
        e.emit_cargo_warning();
        panic!("ontogen servers pass failed: {e}");
    });
}
