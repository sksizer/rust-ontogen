//! Tests for the clients module: TypeScript bindings, HTTP/IPC transport,
//! HTTP-only client, and admin-registry generators.
//!
//! Mirrors the structure of [`crate::servers::tests`] for the server-side
//! generators. Most client-side cases still live there next to the
//! fixtures they share; the multi-surface cases live here.

use std::collections::HashMap;

use crate::clients::ClientGenerator;
use crate::clients::config::Config;
use crate::servers::tests::{two_surface_fixture, write_synthetic_api};
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
        schema_enums: Vec::new(),
        label_overrides: HashMap::new(),
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
    crate::clients::generators::admin::generate(
        &admin_out,
        &modules,
        &config,
        &config.schema_entities,
        &config.schema_enums,
    );

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

#[test]
fn surface_entities_come_from_the_surfaces_that_name_a_schema_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("athlete.rs"),
        r#"
        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Athlete {
            #[ontology(id)]
            pub id: String,
            pub display_name: String,
        }
        "#,
    )
    .unwrap();
    let surface = |schema_dir| ApiSurface {
        api_dir: std::path::PathBuf::from("unused"),
        service_import_path: String::new(),
        types_import_path: String::new(),
        store_accessor: None,
        store_type: None,
        pagination: None,
        paginated_modules: Vec::new(),
        schema_dir,
    };

    let entities =
        crate::clients::surface_schema(&[surface(None), surface(Some(dir.path().to_path_buf()))]).unwrap().entities;
    let names: Vec<_> = entities.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Athlete"]);
    assert!(entities[0].fields.iter().any(|f| f.name == "display_name"));
}

#[test]
fn the_registry_says_whether_list_takes_a_query() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "note.rs",
        "pub async fn list(store: &Store, query: ListNotesQuery, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Note>, anyhow::Error> { todo!() }
pub async fn count(store: &Store) -> Result<u64, anyhow::Error> { todo!() }
pub async fn get_by_id(store: &Store, id: &str) -> Result<Note, anyhow::Error> { todo!() }
pub async fn create(store: &Store, input: CreateNoteInput) -> Result<Note, anyhow::Error> { todo!() }
pub async fn update(store: &Store, id: &str, input: UpdateNoteInput) -> Result<Note, anyhow::Error> { todo!() }
pub async fn delete(store: &Store, id: &str) -> Result<(), anyhow::Error> { todo!() }
",
    );
    write_synthetic_api(&api_dir, "tag.rs", &crate::servers::tests::crud_module_source("tag", "Store"));
    let mut config = two_surface_client_config(vec![ApiSurface {
        api_dir,
        service_import_path: "crate::api".to_string(),
        types_import_path: "crate::schema".to_string(),
        store_accessor: None,
        store_type: Some("Store".to_string()),
        pagination: Some(PaginationConfig { default_limit: 20, max_limit: 100 }),
        paginated_modules: vec!["note".to_string()],
        schema_dir: None,
    }]);
    let admin_out = tmp.path().join("admin-registry.ts");
    config.generators = vec![ClientGenerator::AdminRegistry { output: admin_out.clone() }];

    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::clients::generators::admin::generate(
        &admin_out,
        &modules,
        &config,
        &config.schema_entities,
        &config.schema_enums,
    );

    let registry = std::fs::read_to_string(&admin_out).unwrap();
    let note = &registry[registry.find("key: 'note'").unwrap()..registry.find("key: 'tag'").unwrap()];
    assert!(note.contains("listQuery: true"), "note's list takes a query:\n{note}");
    let tag = &registry[registry.find("key: 'tag'").unwrap()..];
    assert!(!tag.contains("listQuery"), "tag's list does not:\n{tag}");
}

#[test]
fn the_registry_carries_enum_values_label_overrides_and_the_id_type() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "source.rs", &crate::servers::tests::crud_module_source("source", "Store"));
    write_synthetic_api(&api_dir, "reading.rs", &crate::servers::tests::crud_module_source("reading", "Store"));
    let schema = r#"
        #[derive(Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum Quality {
            PeerReviewed,
            Community,
        }

        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Source {
            #[ontology(id)]
            pub id: String,
            #[ontology(enum_field)]
            pub kind: Quality,
            pub quality: Option<Quality>,
            pub avg_hr_bpm: Option<i32>,
        }

        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Reading {
            #[ontology(id)]
            pub id: i64,
            pub avg_hr_bpm: Option<i32>,
        }
    "#;
    let path = std::path::Path::new("schema.rs");
    let mut config = two_surface_client_config(vec![ApiSurface {
        api_dir,
        service_import_path: "crate::api".to_string(),
        types_import_path: "crate::schema".to_string(),
        store_accessor: None,
        store_type: Some("Store".to_string()),
        pagination: None,
        paginated_modules: Vec::new(),
        schema_dir: None,
    }]);
    config.schema_entities = crate::schema::parse::parse_schema_source(schema, path).unwrap();
    config.schema_enums = crate::schema::parse::parse_schema_enums_source(schema, path).unwrap();
    config.label_overrides = HashMap::from([
        ("avg_hr_bpm".to_string(), "Average HR (bpm)".to_string()),
        ("reading.avg_hr_bpm".to_string(), "Heart rate".to_string()),
    ]);
    let admin_out = tmp.path().join("admin-registry.ts");
    config.generators = vec![ClientGenerator::AdminRegistry { output: admin_out.clone() }];

    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::clients::generators::admin::generate(
        &admin_out,
        &modules,
        &config,
        &config.schema_entities,
        &config.schema_enums,
    );

    let registry = std::fs::read_to_string(&admin_out).unwrap();
    let reading = &registry[registry.find("key: 'reading'").unwrap()..registry.find("key: 'source'").unwrap()];
    let source = &registry[registry.find("key: 'source'").unwrap()..];
    assert!(source.contains("idType: 'string'"), "a String id:\n{source}");
    assert!(reading.contains("idType: 'number'"), "an i64 id:\n{reading}");
    let kind = &source[source.find("key: 'kind'").unwrap()..source.find("key: 'quality'").unwrap()];
    assert!(
        kind.contains("type: 'enum', required: true") && kind.contains("enumValues: ['peer-reviewed', 'community']"),
        "a bare enum field is a required select:\n{kind}"
    );
    let quality = &source[source.find("key: 'quality'").unwrap()..source.find("key: 'avg_hr_bpm'").unwrap()];
    assert!(
        quality.contains("type: 'enum'")
            && !quality.contains("required")
            && quality.contains("enumValues: ['peer-reviewed', 'community']"),
        "an optional enum field is an optional select:\n{quality}"
    );
    assert!(source.contains("label: 'Average HR (bpm)'"), "the field-wide override:\n{source}");
    assert!(reading.contains("label: 'Heart rate'"), "the entity's own override wins:\n{reading}");
}
