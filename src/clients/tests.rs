//! Tests for the clients module: TypeScript bindings, HTTP/IPC transport,
//! HTTP-only client, and admin-registry generators.
//!
//! Mirrors the structure of [`crate::servers::tests`] for the server-side
//! generators. Most client-side cases still live there next to the
//! fixtures they share; the multi-surface cases live here.

use std::collections::HashMap;

use crate::clients::ClientGenerator;
use crate::clients::config::Config;
use crate::servers::tests::two_surface_fixture;
use crate::servers::{ApiSurface, NamingConfig, PaginationConfig};

/// A clients `Config` whose primary surface is `surfaces[0]` and whose
/// `extra_surfaces` are the rest.
fn two_surface_client_config(surfaces: Vec<ApiSurface>) -> Config {
    let mut surfaces = surfaces.into_iter();
    let primary = surfaces.next().expect("at least one surface");
    Config {
        api_dir: primary.api_dir,
        state_type: "AppState".to_string(),
        service_import_path: primary.service_import_path,
        types_import_path: primary.types_import_path,
        state_import: "crate::AppState".to_string(),
        naming: NamingConfig::default(),
        generators: vec![],
        ts_formatter: crate::TsFormatter::None,
        sse_route_overrides: HashMap::new(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: primary.store_type,
        store_import: Some("crate::store::Store".to_string()),
        schema_entities: Vec::new(),
        pagination: primary.pagination,
        pool_extra_roots: Vec::new(),
        pool_exclude_paths: Vec::new(),
        extra_surfaces: surfaces.collect(),
    }
}

#[test]
fn test_two_surfaces_transport_merges_module_and_paginates_per_module() {
    let tmp = tempfile::tempdir().unwrap();
    let mut surfaces = two_surface_fixture(tmp.path());
    surfaces[1].pagination = Some(PaginationConfig { default_limit: 20, max_limit: 100 });
    surfaces[1].paginated_modules = vec!["exercise".to_string()];
    let config = two_surface_client_config(surfaces);

    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type Placeholder = unknown;\n").unwrap();
    let ts_out = tmp.path().join("transport.ts");
    crate::clients::generators::transport::generate(&ts_out, &bindings, &modules, &config);

    let ts = std::fs::read_to_string(&ts_out).unwrap();
    assert!(ts.contains("export interface PaginatedResult<T>"), "pagination on any surface emits the wrapper:\n{ts}");
    assert!(
        ts.contains("exerciseList(limit?: number, offset?: number): Promise<PaginatedResult<Exercise>>"),
        "listed module paginates:\n{ts}"
    );
    assert!(ts.contains("workoutList(): Promise<Workout[]>"), "unlisted module of the same surface does not:\n{ts}");
    assert!(ts.contains("athleteList(): Promise<Athlete[]>"), "primary surface is untouched:\n{ts}");
    for method in ["workoutStart(", "workoutGetSummary(", "workoutGetById(", "workoutCreate("] {
        assert!(ts.contains(method), "merged module keeps one method prefix, missing {method}:\n{ts}");
    }
}

#[test]
fn test_two_surfaces_admin_registry_reports_pagination_per_module() {
    let tmp = tempfile::tempdir().unwrap();
    let mut surfaces = two_surface_fixture(tmp.path());
    surfaces[1].pagination = Some(PaginationConfig { default_limit: 20, max_limit: 100 });
    surfaces[1].paginated_modules = vec!["exercise".to_string()];
    let mut config = two_surface_client_config(surfaces);
    let admin_out = tmp.path().join("admin-registry.ts");
    config.generators = vec![ClientGenerator::AdminRegistry { output: admin_out.clone() }];

    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::clients::generators::admin::generate(&admin_out, &modules, &config);

    let registry = std::fs::read_to_string(&admin_out).unwrap();
    let entry = |key: &str| {
        let start =
            registry.find(&format!("key: '{key}'")).unwrap_or_else(|| panic!("no entry for {key}:\n{registry}"));
        let end = registry[start..].find("},").map_or(registry.len(), |i| start + i);
        registry[start..end].to_string()
    };
    assert!(entry("exercise").contains("paginated: true"), "{registry}");
    assert!(entry("exercise").contains("defaultLimit: 20"), "{registry}");
    assert!(entry("workout").contains("paginated: false"), "{registry}");
    assert!(entry("workout").contains("listMethod: 'workoutList'"), "the merged module is a CRUD entity:\n{registry}");
    assert!(!registry.contains("key: 'athlete'"), "a list-only module is not a CRUD entity:\n{registry}");
}
