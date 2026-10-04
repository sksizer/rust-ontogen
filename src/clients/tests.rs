//! Tests for the clients module: TypeScript bindings, HTTP/IPC transport,
//! HTTP-only client, and admin-registry generators.
//!
//! Mirrors the structure of [`crate::servers::tests`] for the server-side
//! generators. Client-side test cases were relocated here as part of the
//! servers→clients split.

use std::collections::HashMap;
use std::fs;

use ontogen_core::utils::TsFormatter;

use super::{
    LONG_TAIL_MARKER, append_long_tail_to_bindings, extra_root_crate_name, package_name_from_manifest, strip_long_tail,
};

/// The long-tail emitter's raw style — single-quoted literals — standing in
/// for what `ontogen_ts::emit` produces.
const LONG_TAIL_TS: &str = "export type RuleCategory = 'Heading' | 'List' | 'Other';";

/// A schema-known bindings file as the earlier `write_and_format_ts` pass
/// would have left it.
const SCHEMA_KNOWN: &str = "export type Note = { id: string };\n";

/// Stand-in for a real formatter's quote normalization (biome and prettier
/// both canonicalize to double quotes by default).
fn double_quote_formatter() -> TsFormatter {
    TsFormatter::custom(|src: &str, _path: &std::path::Path| Ok(src.replace('\'', "\"")))
}

/// A formatter that also collapses blank lines — the adversarial case for
/// marker detection, since it rewrites the whitespace the marker sits in.
fn collapsing_formatter() -> TsFormatter {
    TsFormatter::custom(|src: &str, _path: &std::path::Path| {
        let mut out = src.replace('\'', "\"");
        while out.contains("\n\n") {
            out = out.replace("\n\n", "\n");
        }
        Ok(out)
    })
}

/// Write `SCHEMA_KNOWN` into a fresh tempdir and return `(dir, path)`.
fn seeded_bindings() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("types.ts");
    fs::write(&path, SCHEMA_KNOWN).expect("seed bindings");
    (dir, path)
}

#[test]
fn long_tail_section_goes_through_the_formatter() {
    // Issue #123: the base was written through `write_and_format_ts`, then
    // the long-tail chunk was appended with a plain `write_if_changed` —
    // after the formatter pass — so the appended section kept ontogen-ts's
    // raw emit style inside an otherwise formatter-canonical file.
    let (_dir, path) = seeded_bindings();
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &double_quote_formatter()).expect("append");

    let written = fs::read_to_string(&path).expect("read back");
    assert!(written.contains(LONG_TAIL_MARKER), "marker missing; file was:\n{written}");
    assert!(written.contains(r#""Heading""#), "long-tail section was not formatted; file was:\n{written}");
    assert!(!written.contains('\''), "single quotes survived the formatter; file was:\n{written}");
}

#[test]
fn unformatted_config_still_writes_the_section_verbatim() {
    // `TsFormatter::None` must stay a pass-through — routing through the
    // formatter should not change output for consumers who opted out.
    let (_dir, path) = seeded_bindings();
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &TsFormatter::None).expect("append");

    let written = fs::read_to_string(&path).expect("read back");
    assert!(written.starts_with(SCHEMA_KNOWN), "base was altered; file was:\n{written}");
    assert!(written.contains(LONG_TAIL_TS), "raw long-tail missing; file was:\n{written}");
}

#[test]
fn rerunning_replaces_the_section_rather_than_doubling_it() {
    for formatter in [TsFormatter::None, double_quote_formatter(), collapsing_formatter()] {
        let (_dir, path) = seeded_bindings();
        append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("first append");
        let first = fs::read_to_string(&path).expect("read back");

        append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("second append");
        let second = fs::read_to_string(&path).expect("read back");

        assert_eq!(first, second, "rebuild was not a fixpoint for {formatter:?}");
        assert_eq!(second.matches(LONG_TAIL_MARKER).count(), 1, "section doubled for {formatter:?}:\n{second}");
    }
}

#[test]
fn rerunning_after_a_blank_line_collapsing_formatter_still_finds_the_marker() {
    // Regression guard specific to formatting after assembly: the marker
    // used to be matched together with its surrounding newlines, so a
    // formatter that rewrote that whitespace would make the strip miss and
    // every rebuild would append another copy of the section.
    let (_dir, path) = seeded_bindings();
    let formatter = collapsing_formatter();
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("first append");

    let after_first = fs::read_to_string(&path).expect("read back");
    assert!(!after_first.contains("\n\n"), "test formatter should have collapsed blank lines:\n{after_first}");

    // The base is still recoverable from the collapsed file.
    assert_eq!(strip_long_tail(&after_first), SCHEMA_KNOWN.trim_end());

    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("second append");
    let after_second = fs::read_to_string(&path).expect("read back");
    assert_eq!(after_first, after_second);
}

#[test]
fn changed_long_tail_replaces_the_previous_section() {
    let (_dir, path) = seeded_bindings();
    let formatter = double_quote_formatter();
    append_long_tail_to_bindings(&path, "export type Old = 'a';", &formatter).expect("first append");
    append_long_tail_to_bindings(&path, "export type New = 'b';", &formatter).expect("second append");

    let written = fs::read_to_string(&path).expect("read back");
    assert!(written.contains("export type New"), "new section missing; file was:\n{written}");
    assert!(!written.contains("export type Old"), "stale section survived; file was:\n{written}");
    assert!(written.starts_with(SCHEMA_KNOWN), "base was lost; file was:\n{written}");
}

#[test]
fn missing_bindings_file_is_created() {
    // The schema-known emitter writes the file first, but the append path is
    // defensive about it. With no base there is nothing to separate from, so
    // the marker leads the file.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("types.ts");
    let formatter = double_quote_formatter();

    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("append");
    let first = fs::read_to_string(&path).expect("read back");
    assert!(first.starts_with(LONG_TAIL_MARKER), "file was:\n{first}");

    // And the empty base still round-trips.
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("second append");
    assert_eq!(first, fs::read_to_string(&path).expect("read back"));
}

#[test]
fn append_does_not_rewrite_an_unchanged_file() {
    // `write_and_format_ts` writes only on change. Without that, every
    // rebuild bumps the mtime and file watchers (e.g. `tauri dev`) loop.
    let (_dir, path) = seeded_bindings();
    let formatter = double_quote_formatter();
    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("first append");
    let mtime = fs::metadata(&path).expect("metadata").modified().expect("mtime");

    append_long_tail_to_bindings(&path, LONG_TAIL_TS, &formatter).expect("second append");
    let after = fs::metadata(&path).expect("metadata").modified().expect("mtime");
    assert_eq!(mtime, after, "unchanged rebuild touched the file");
}

#[test]
fn strip_long_tail_handles_a_file_without_the_marker() {
    assert_eq!(strip_long_tail(SCHEMA_KNOWN), SCHEMA_KNOWN.trim_end());
    assert_eq!(strip_long_tail(""), "");
}

// ── pool_extra_roots crate naming (issue #84) ────────────────────────────

/// Lay out `<dir>/<crate_name>/{Cargo.toml,src}` and return the `src` path.
fn sibling_crate(manifest: Option<&str>, dir_name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let crate_dir = dir.path().join(dir_name);
    let src = crate_dir.join("src");
    fs::create_dir_all(&src).expect("create src");
    if let Some(manifest) = manifest {
        fs::write(crate_dir.join("Cargo.toml"), manifest).expect("write manifest");
    }
    (dir, src)
}

#[test]
fn extra_root_crate_name_prefers_the_manifest_package_name() {
    // The package name is what a consuming crate writes in a `use`, so it's
    // what the sibling's pool keys have to be rooted at. The directory can
    // differ from it.
    let (_dir, src) =
        sibling_crate(Some("[package]\nname = \"vaultpolish-core\"\nversion = \"0.1.0\"\n"), "core-checkout");
    assert_eq!(extra_root_crate_name(&src), "vaultpolish_core");
}

#[test]
fn extra_root_crate_name_falls_back_to_the_directory() {
    // No readable manifest — a non-standard layout. Sharing a namespace with
    // the consuming crate would be worse than a slightly wrong name.
    let (_dir, src) = sibling_crate(None, "vaultpolish-core");
    assert_eq!(extra_root_crate_name(&src), "vaultpolish_core");
}

#[test]
fn package_name_ignores_names_outside_the_package_table() {
    // `name` appears under plenty of other tables; only `[package]` counts.
    let manifest = "\
[dependencies]
name = \"not-the-package\"

[package]
name = \"real-package\"

[[bin]]
name = \"some-binary\"
";
    assert_eq!(package_name_from_manifest(manifest).as_deref(), Some("real-package"));
}

#[test]
fn package_name_absent_yields_none() {
    assert_eq!(package_name_from_manifest("[workspace]\nmembers = [\"a\"]\n"), None);
}

// ── admin registry: listHasQuery flag (Issue 2 follow-up) ───────────────────
//
// transport.rs's OpKind::List branch puts a `query?: T` param ahead of the
// pagination args only when the Rust `list` fn takes a `Query`-typed
// parameter — see transport.rs's `generate_transport_interface`. The admin
// registry has to record which shape a given entity's list method uses, so
// `useAdminEntity.fetchList` can call it correctly instead of guessing.

use std::path::PathBuf as StdPathBuf;

use crate::clients::config::Config as ClientsConfig;
use crate::clients::generators::admin;
use crate::servers::PaginationConfig;
use crate::servers::parse::{ApiFn, ApiModule, Param};
use crate::servers::types::NamingConfig;

/// Build a `Param` from a name and a type string. Panics if the type fails to
/// parse as a `syn::Type`. Mirrors `servers::tests::param`, duplicated here
/// because that module's `#[cfg(test)] mod tests` is private to `servers`.
fn admin_test_param(name: &str, ty: &str) -> Param {
    let ty_ast: syn::Type = syn::parse_str(ty).expect("test param type must parse as syn::Type");
    Param { name: name.to_string(), ty: ty.to_string(), ty_ast }
}

fn admin_test_ty_ast(ty: &str) -> syn::Type {
    syn::parse_str(ty).expect("test return type must parse as syn::Type")
}

/// A minimal CRUD module (list/get_by_id/create/update/delete — the shape
/// `admin::generate`'s `crud_modules` filter requires), with `list` optionally
/// taking a `<Name>Query`-typed parameter.
fn admin_test_crud_module(name: &str, list_has_query: bool) -> ApiModule {
    let mut list_params = Vec::new();
    if list_has_query {
        list_params.push(admin_test_param("query", "Option<TimerSessionQuery>"));
    }
    ApiModule {
        name: name.to_string(),
        functions: vec![
            ApiFn {
                name: "list".to_string(),
                is_async: true,
                doc: format!("List all {name}s."),
                params: list_params,
                return_type: format!("Vec<{name}>"),
                return_type_ast: admin_test_ty_ast(&format!("Vec<{name}>")),
                ..Default::default()
            },
            ApiFn {
                name: "get_by_id".to_string(),
                is_async: true,
                doc: format!("Get a {name} by ID."),
                params: vec![admin_test_param("id", "&str")],
                return_type: name.to_string(),
                return_type_ast: admin_test_ty_ast(name),
                ..Default::default()
            },
            ApiFn {
                name: "create".to_string(),
                is_async: true,
                doc: format!("Create a new {name}."),
                params: vec![admin_test_param("input", &format!("Create{name}Input"))],
                return_type: name.to_string(),
                return_type_ast: admin_test_ty_ast(name),
                ..Default::default()
            },
            ApiFn {
                name: "update".to_string(),
                is_async: true,
                doc: format!("Update a {name}."),
                params: vec![admin_test_param("id", "&str"), admin_test_param("input", &format!("Update{name}Input"))],
                return_type: name.to_string(),
                return_type_ast: admin_test_ty_ast(name),
                ..Default::default()
            },
            ApiFn {
                name: "delete".to_string(),
                is_async: true,
                doc: format!("Delete a {name}."),
                params: vec![admin_test_param("id", "&str")],
                ..Default::default()
            },
        ],
        events: vec![],
        is_singleton: false,
        has_count: false,
    }
}

/// A paginated admin-registry `ClientsConfig`, with `list_has_query`
/// controlling whether the fixture `list` fn takes a query-struct param.
fn admin_test_config() -> ClientsConfig {
    ClientsConfig {
        api_dir: StdPathBuf::from("src/api/v1"),
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming: NamingConfig::default(),
        generators: vec![],
        ts_formatter: crate::TsFormatter::None,
        sse_route_overrides: HashMap::new(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        entities: Vec::new(),
        resources: Default::default(),
        pagination: Some(PaginationConfig { default_limit: 50, max_limit: 200 }),
        pool_extra_roots: Vec::new(),
        pool_exclude_paths: Vec::new(),
        extra_surfaces: Vec::new(),
        schema_enums: Vec::new(),
        label_overrides: HashMap::new(),
    }
}

#[test]
fn paginated_list_with_a_query_struct_param_emits_list_has_query() {
    let module = admin_test_crud_module("timer_session", true);
    let dir = tempfile::tempdir().expect("tempdir");
    let output = dir.path().join("admin-registry.ts");

    admin::generate(&output, std::slice::from_ref(&module), &admin_test_config(), &[], &[]);

    let written = fs::read_to_string(&output).expect("read admin registry");
    assert!(written.contains("paginated: true"), "registry was:\n{written}");
    assert!(written.contains("listHasQuery: true"), "registry was:\n{written}");
}

#[test]
fn paginated_list_without_a_query_struct_param_omits_list_has_query() {
    let module = admin_test_crud_module("timer_session", false);
    let dir = tempfile::tempdir().expect("tempdir");
    let output = dir.path().join("admin-registry.ts");

    admin::generate(&output, std::slice::from_ref(&module), &admin_test_config(), &[], &[]);

    let written = fs::read_to_string(&output).expect("read admin registry");
    assert!(written.contains("paginated: true"), "registry was:\n{written}");
    assert!(!written.contains("listHasQuery"), "registry was:\n{written}");
}

// ─── Multi-surface clients ────────────────────────────────────────────────────

use crate::clients::ClientGenerator;
use crate::clients::config::Config;
use crate::servers::ApiSurface;
use crate::servers::tests::{two_surface_fixture, write_synthetic_api};

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
        entities: Vec::new(),
        resources: Default::default(),
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
    crate::clients::generators::admin::generate(&admin_out, &modules, &config, &config.entities, &config.schema_enums);

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
    crate::clients::generators::admin::generate(&admin_out, &modules, &config, &config.entities, &config.schema_enums);

    let registry = std::fs::read_to_string(&admin_out).unwrap();
    let note = &registry[registry.find("key: 'note'").unwrap()..registry.find("key: 'tag'").unwrap()];
    assert!(note.contains("listHasQuery: true"), "note's list takes a query:\n{note}");
    let tag = &registry[registry.find("key: 'tag'").unwrap()..];
    assert!(!tag.contains("listHasQuery"), "tag's list does not:\n{tag}");
}

#[test]
fn test_two_surfaces_same_entity_name_is_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut surfaces = two_surface_fixture(tmp.path());
    let entity = |name: &str| {
        format!(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct {name} {{\n    #[ontology(id)]\n    pub id: \
             String,\n}}\n"
        )
    };
    let primary_schema = tmp.path().join("primary_schema");
    std::fs::create_dir_all(&primary_schema).unwrap();
    std::fs::write(primary_schema.join("workout.rs"), entity("Workout")).unwrap();
    let fitness_schema = tmp.path().join("fitness_schema");
    std::fs::create_dir_all(&fitness_schema).unwrap();
    std::fs::write(fitness_schema.join("workout.rs"), entity("Workout")).unwrap();
    surfaces[1].schema_dir = Some(fitness_schema);

    let mut config = two_surface_client_config(surfaces);
    config.entities = crate::parse_schema(&crate::SchemaConfig { schema_dir: primary_schema }).unwrap().entities;
    config.generators = vec![ClientGenerator::AdminRegistry { output: tmp.path().join("admin-registry.ts") }];

    let err = crate::clients::generate_clients(&config).expect_err("an entity name shared by two surfaces must fail");
    assert!(err.contains("entity `Workout`"), "{err}");
    assert!(err.contains("more than one API surface"), "{err}");
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
    config.entities = crate::schema::parse::parse_schema_source(schema, path).unwrap();
    config.schema_enums = crate::schema::parse::parse_schema_enums_source(schema, path).unwrap();
    config.label_overrides = HashMap::from([
        ("avg_hr_bpm".to_string(), "Average HR (bpm)".to_string()),
        ("reading.avg_hr_bpm".to_string(), "Heart rate".to_string()),
    ]);
    let admin_out = tmp.path().join("admin-registry.ts");
    config.generators = vec![ClientGenerator::AdminRegistry { output: admin_out.clone() }];

    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::clients::generators::admin::generate(&admin_out, &modules, &config, &config.entities, &config.schema_enums);

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

/// The guard on [`crate::ClientsConfig::new`]: a consumer that supplies only
/// the five required inputs still compiles and still gets an inert
/// configuration, so adding a field to `ClientsConfig` costs consumers
/// nothing. Drop the `..new(..)` base from a consumer and this test stops
/// compiling; give a defaulted field a non-inert value in `new` and the
/// assertions below fail.
#[test]
fn a_config_built_from_only_its_required_inputs_is_inert() {
    let config =
        crate::ClientsConfig::new("src/api/v1", "AppState", "crate::api::v1", "crate::schema", "crate::AppState");

    assert_eq!(config.api_dir, std::path::Path::new("src/api/v1"));
    assert_eq!(config.state_type, "AppState");
    assert_eq!(config.service_import_path, "crate::api::v1");
    assert_eq!(config.types_import_path, "crate::schema");
    assert_eq!(config.state_import, "crate::AppState");

    assert!(config.generators.is_empty(), "nothing is generated until a generator is named");
    assert!(matches!(config.ts_formatter, TsFormatter::None), "TypeScript is emitted as generated");
    assert!(config.sse_route_overrides.is_empty());
    assert!(config.ts_skip_commands.is_empty());
    assert!(config.route_prefix.is_none());
    assert!(config.store_type.is_none() && config.store_import.is_none());
    assert!(config.pagination.is_none());
    assert!(config.schema_enums.is_empty());
    assert!(config.label_overrides.is_empty());
    assert!(config.pool_extra_roots.is_empty() && config.pool_exclude_paths.is_empty());
    assert!(config.extra_surfaces.is_empty(), "one surface, the primary");
}

/// The shape every consuming `build.rs` is meant to use: override the handful
/// of fields the project cares about, inherit the rest. The overrides must
/// survive the update syntax, and the base must not leak back over them.
#[test]
fn a_partial_literal_over_new_keeps_its_overrides() {
    let config = crate::ClientsConfig {
        generators: vec![crate::clients::ClientGenerator::AdminRegistry { output: "app/admin-registry.ts".into() }],
        store_type: Some("Store".into()),
        ..crate::ClientsConfig::new("src/api/v1", "AppState", "crate::api::v1", "crate::schema", "crate::AppState")
    };

    assert_eq!(config.generators.len(), 1);
    assert_eq!(config.store_type.as_deref(), Some("Store"));
    assert!(config.store_import.is_none(), "an unmentioned field stays at the base's default");
}

// ─── JSON:API HTTP clients (wire contract §14) ──────────────────────────────

/// Every relationship shape §5.4 names: an optional and a required
/// `belongs_to`, `has_many` under both, `many_to_many`, and resources with no
/// relationships, one of them with an id field not called `id`.
const JSONAPI_SCHEMA: &str = r#"
#[derive(OntologyEntity)]
#[ontology(entity)]
pub struct Task {
    #[ontology(id)]
    pub id: String,
    pub title: String,
    pub estimate: Option<u32>,
    #[ontology(relation(belongs_to, target = "Task"))]
    pub parent_id: Option<String>,
    #[ontology(relation(has_many, target = "Task", foreign_key = "parent_id"))]
    pub subtasks: Vec<String>,
    #[ontology(relation(many_to_many, target = "Tag"))]
    pub tags: Vec<String>,
    #[ontology(body)]
    pub body: String,
}

#[derive(OntologyEntity)]
#[ontology(entity)]
pub struct WorkoutSet {
    #[ontology(id)]
    pub id: String,
    pub reps: u32,
    #[ontology(relation(belongs_to, target = "Tag"))]
    pub tag_id: String,
}

#[derive(OntologyEntity)]
#[ontology(entity)]
pub struct Tag {
    #[ontology(id)]
    pub slug: String,
    pub title: String,
}
"#;

/// `board`, a module with no entity behind it, beside the generated resource
/// modules: CRUD-named and junction ops served as custom ops, a custom GET
/// with a path and an optional argument, custom POSTs with and without
/// arguments, and an event op whose item type is an entity.
const BOARD_MODULE: &str = "\
use crate::schema::{CreateTaskInput, Tag, Task, UpdateTaskInput};
use crate::store::Store;
use crate::AppState;

pub async fn list(store: &Store, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Task>, anyhow::Error> { todo!() }
pub async fn count(store: &Store) -> Result<u64, anyhow::Error> { todo!() }
pub async fn get_by_id(store: &Store, id: &str) -> Result<Task, anyhow::Error> { todo!() }
pub async fn create(store: &Store, input: CreateTaskInput) -> Result<Task, anyhow::Error> { todo!() }
pub async fn update(store: &Store, id: &str, input: UpdateTaskInput) -> Result<Task, anyhow::Error> { todo!() }
pub async fn delete(store: &Store, id: &str) -> Result<(), anyhow::Error> { todo!() }
pub async fn list_tags(store: &Store, board_id: &str) -> Result<Vec<Tag>, anyhow::Error> { todo!() }
pub async fn add_tag(store: &Store, board_id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }
pub async fn remove_tag(store: &Store, board_id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }
pub async fn get_summary(store: &Store, id: &str, include_done: Option<bool>) -> Result<Task, anyhow::Error> { todo!() }
pub async fn archive(store: &Store, task_id: &str, reason: Option<String>) -> Result<Task, anyhow::Error> { todo!() }
pub async fn import(store: &Store, input: CreateTaskInput, dry_run: Option<bool>) -> Result<u64, anyhow::Error> { todo!() }
pub async fn reset(store: &Store) -> Result<(), anyhow::Error> { todo!() }
pub async fn task_changes(state: &AppState, resume: Option<String>) -> Result<tokio::sync::broadcast::Receiver<Task>, anyhow::Error> { todo!() }
";

/// What [`jsonapi_clients`] generated.
struct JsonApiClients {
    transport: String,
    http: String,
    bindings: String,
}

/// The `HttpTauriIpcSplit` and `HttpTs` output for [`JSONAPI_SCHEMA`] and
/// [`BOARD_MODULE`], through
/// the public `gen_api` → `gen_clients` path. With `paginated`, every list
/// pages. `adjust` edits the clients config before generation.
fn jsonapi_clients(paginated: bool, adjust: impl FnOnce(&mut crate::ClientsConfig)) -> JsonApiClients {
    let tmp = tempfile::tempdir().unwrap();
    let entities =
        crate::schema::parse::parse_schema_source(JSONAPI_SCHEMA, std::path::Path::new("schema.rs")).unwrap();
    let api_dir = tmp.path().join("api");
    let api = crate::gen_api(
        &entities,
        &crate::ApiConfig {
            output_dir: api_dir.clone(),
            exclude: Vec::new(),
            scan_dirs: Vec::new(),
            state_type: "AppState".into(),
            store_type: Some("Store".into()),
            schema_module_path: "crate::schema".into(),
            paginated: if paginated {
                entities.iter().map(|e| crate::to_snake_case(&e.name)).collect()
            } else {
                vec![]
            },
        },
    )
    .unwrap();
    fs::write(api_dir.join("board.rs"), BOARD_MODULE).unwrap();
    let ts = tmp.path().join("ts");
    fs::create_dir_all(&ts).unwrap();
    // Named for the admin-layer fixture, which sits beside other bindings.
    let bindings_path = ts.join("jsonapi-bindings.ts");
    let mut config = crate::ClientsConfig {
        generators: vec![
            ClientGenerator::HttpTauriIpcSplit {
                output: ts.join("transport.ts"),
                bindings_path: bindings_path.clone(),
            },
            ClientGenerator::HttpTs { output: ts.join("http.ts"), bindings_path: bindings_path.clone() },
        ],
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: paginated.then_some(PaginationConfig { default_limit: 20, max_limit: 100 }),
        ..crate::ClientsConfig::new(api_dir, "AppState", "crate::api", "crate::schema", "crate::AppState")
    };
    adjust(&mut config);
    crate::gen_clients(&entities, Some(&api), &[], &config).unwrap();
    let read = |path: &std::path::Path| fs::read_to_string(path).unwrap();
    JsonApiClients {
        transport: read(&ts.join("transport.ts")),
        http: read(&ts.join("http.ts")),
        bindings: read(&bindings_path),
    }
}

/// The text of `function name(` or `function name<` through its closing `}` line.
fn ts_function<'a>(ts: &'a str, name: &str) -> &'a str {
    let start = [format!("function {name}("), format!("function {name}<")]
        .iter()
        .find_map(|head| ts.find(head.as_str()))
        .unwrap_or_else(|| panic!("no `{name}` in:\n{ts}"));
    let end = ts[start..].find("\n}\n").map_or(ts.len(), |i| start + i + 3);
    &ts[start..end]
}

/// The text of the generated method `name` through its closing brace.
fn ts_method<'a>(ts: &'a str, name: &str) -> &'a str {
    let start = ts.find(&format!("async {name}(")).unwrap_or_else(|| panic!("no method `{name}` in:\n{ts}"));
    let end = ts[start..].find("},\n").map_or(ts.len(), |i| start + i + 3);
    &ts[start..end]
}

#[test]
fn each_resource_gets_a_flatten_pair_from_its_relationship_table() {
    let clients = jsonapi_clients(false, |_| {});
    for ts in [&clients.transport, &clients.http] {
        // Optional belongs_to, has_many and many_to_many on one type.
        assert!(
            ts.contains(
                "const TASK_RESOURCE: JsonApiResourceDef = {\n  type: 'tasks',\n  idField: 'id',\n  relationships: {\n    \
                 parent: { field: 'parent_id', type: 'tasks', many: false },\n    subtasks: { field: 'subtasks', type: \
                 'tasks', many: true },\n    tags: { field: 'tags', type: 'tags', many: true },\n  },\n};"
            ),
            "{ts}"
        );
        assert_eq!(
            ts_function(ts, "flattenTask"),
            "function flattenTask(r: JsonApiResource): Task {\n  return {\n    id: r.id,\n    ...r.attributes,\n    \
             parent_id: toOneId(r.relationships?.['parent']),\n    subtasks: toManyIds(r.relationships?.['subtasks']),\n    \
             tags: toManyIds(r.relationships?.['tags']),\n  } as Task;\n}\n"
        );
        assert!(ts.contains(
            "function unflattenTask(input: object, id?: string): JsonApiWriteDocument {\n  return \
             unflattenResource(TASK_RESOURCE, input, id);\n}"
        ));

        // A required belongs_to reads the same way; the type is the kebab plural.
        assert!(ts.contains("  type: 'workout-sets',\n  idField: 'id',\n  relationships: {\n    tag: { field: 'tag_id', type: 'tags', many: false },\n  },"));
        assert!(ts_function(ts, "flattenWorkoutSet").contains("    tag_id: toOneId(r.relationships?.['tag']),\n"));

        // No relationships, and an id field not called `id`.
        assert!(ts.contains(
            "const TAG_RESOURCE: JsonApiResourceDef = {\n  type: 'tags',\n  idField: 'slug',\n  relationships: {},\n};"
        ));
        assert_eq!(
            ts_function(ts, "flattenTag"),
            "function flattenTag(r: JsonApiResource): Tag {\n  return {\n    slug: r.id,\n    ...r.attributes,\n  } as Tag;\n}\n"
        );
    }
    assert_eq!(clients.transport.matches("function unflattenResource(").count(), 1);
}

#[test]
fn unflatten_follows_the_id_and_null_rules() {
    let ts = jsonapi_clients(false, |_| {}).transport;
    let unflatten = ts_function(&ts, "unflattenResource");
    for rule in [
        // undefined is omitted; the id never reaches attributes
        "if (value === undefined) continue;",
        "if (key === def.idField) {",
        // an update's id argument wins; a create id only when non-empty
        "if (id === undefined && typeof value === 'string' && value !== '') resourceId = value;",
        // to-one null is kept as `data: null`
        "relationships[rel.name] = { data: value === null ? null : { type: rel.type, id: String(value) } };",
        // to-many becomes identifiers
        "relationships[rel.name] = { data: value.map((v) => ({ type: rel.type, id: String(v) })) };",
        "...(resourceId !== undefined ? { id: resourceId } : {}),",
    ] {
        assert!(unflatten.contains(rule), "missing `{rule}` in:\n{unflatten}");
    }
}

#[test]
fn resource_crud_methods_speak_json_api() {
    let clients = jsonapi_clients(false, |_| {});
    let ts = &clients.transport;
    assert_eq!(
        ts_method(ts, "taskList"),
        "async taskList(): Promise<Task[]> {\n      const { data } = await \
         httpGet<JsonApiCollectionDocument>('/tasks');\n      return data.map(flattenTask);\n    },\n"
    );
    assert!(ts_method(ts, "taskGetById").contains(
        "const { data } = await httpGet<JsonApiResourceDocument>(`/tasks/${encodeURIComponent(id)}`);\n      return \
         flattenTask(data);"
    ));
    assert!(ts_method(ts, "taskCreate").contains(
        "const { data } = await httpPost<JsonApiResourceDocument>('/tasks', unflattenTask(input));\n      return \
         flattenTask(data);"
    ));
    assert!(ts_method(ts, "taskUpdate").contains(
        "const { data } = await httpPatch<JsonApiResourceDocument>(\n        `/tasks/${encodeURIComponent(id)}`,\n        \
         unflattenTask(input, id),\n      );\n      return flattenTask(data);"
    ));
    assert!(ts_method(ts, "taskDelete").contains("await httpDelete(`/tasks/${encodeURIComponent(id)}`);"));
    assert!(ts_method(ts, "workoutSetList").contains("httpGet<JsonApiCollectionDocument>('/workout-sets')"));

    let http = &clients.http;
    assert!(ts_method(http, "workoutSetList").contains("httpGet<JsonApiCollectionDocument>('/workout-sets')"));
    assert!(ts_method(http, "workoutSetDelete").contains("await httpDelete(`/workout-sets/"), "{http}");
    assert!(ts_method(http, "taskUpdate").contains("httpPatch<JsonApiResourceDocument>"));

    for ts in [ts, http] {
        assert!(ts.contains("async function httpPatch<T>("));
        assert!(!ts.contains("httpPut"), "no module without an entity, so no PUT:\n{ts}");
    }
    // The IPC transport stays flat.
    let ipc = &ts[ts.find("export function createIpcTransport").unwrap()..];
    assert!(ipc.contains("return invoke('task_create', { input });"), "{ipc}");
    assert!(!ipc.contains("flatten"), "{ipc}");
}

#[test]
fn a_paginated_resource_list_pages_with_the_page_family_and_rebuilds_paginated_result() {
    let clients = jsonapi_clients(true, |_| {});
    assert_eq!(
        ts_method(&clients.transport, "taskList"),
        "async taskList(limit?: number, offset?: number): Promise<PaginatedResult<Task>> {\n      const { data, meta } \
         = await httpGet<JsonApiPageDocument>(`/tasks${toQueryString({ page: { offset, limit } })}`);\n      return { \
         items: data.map(flattenTask), total: meta.total, limit: meta.limit, offset: meta.offset };\n    },\n"
    );
    // The family form brackets percent-encoded member names.
    let qs = ts_function(&clients.transport, "toQueryString");
    assert!(qs.contains("push(`${encodeURIComponent(key)}%5B${encodeURIComponent(member)}%5D`, v);"), "{qs}");
    // `HttpTs` pages the same way.
    assert_eq!(
        ts_method(&clients.http, "taskList"),
        "async taskList(limit?: number, offset?: number): Promise<PaginatedResult<Task>> {\n    const { data, meta } = \
         await httpGet<JsonApiPageDocument>(`/tasks${toQueryString({ page: { offset, limit } })}`);\n    return { items: \
         data.map(flattenTask), total: meta.total, limit: meta.limit, offset: meta.offset };\n  },\n"
    );
    assert!(clients.http.contains("export interface PaginatedResult<T> {"), "{}", clients.http);
    assert!(ts_function(&clients.http, "toQueryString").contains("%5B"));
}

#[test]
fn a_scoped_resource_route_keeps_its_prefix() {
    let ts = jsonapi_clients(true, |config| {
        config.route_prefix = Some(crate::servers::RoutePrefix {
            segments: "projects/:project_id".to_string(),
            state_accessor: "store_for".to_string(),
            params: vec![crate::servers::PrefixParam {
                name: "project_id".to_string(),
                rust_type: "String".to_string(),
                ts_type: "string".to_string(),
            }],
        });
    })
    .transport;
    assert!(ts_method(&ts, "taskList").contains(
        "httpGet<JsonApiPageDocument>(scopedPath(projectId, `/tasks${toQueryString({ page: { offset, limit } })}`))"
    ));
    assert!(
        ts_method(&ts, "taskCreate")
            .contains("httpPost<JsonApiResourceDocument>(scopedPath(projectId, '/tasks'), unflattenTask(input))")
    );
}

#[test]
fn every_http_call_throws_json_api_error() {
    let clients = jsonapi_clients(false, |_| {});
    for ts in [&clients.transport, &clients.http] {
        assert!(ts.contains("export class JsonApiError extends Error {\n  override readonly name = 'JsonApiError';"));
        assert!(ts.contains("constructor(status: number, errors: JsonApiErrorObject[], message: string) {"));
        let error = ts_function(ts, "toJsonApiError");
        assert!(error.contains("Array.isArray((body as { errors?: unknown }).errors)"), "{error}");
        assert!(error.contains(": [];"), "a body that is not an error document gives no errors:\n{error}");
        assert!(error.contains("first?.detail ?? first?.title ?? res.statusText"), "{error}");

        let request = ts_function(ts, "httpRequest");
        assert!(request.contains("{ Accept: JSON_API_MEDIA_TYPE }"), "{request}");
        assert!(request.contains("if (body != null) headers['Content-Type'] = JSON_API_MEDIA_TYPE;"), "{request}");
        assert!(request.contains("if (!res.ok) throw await toJsonApiError(res);"), "{request}");
        assert_eq!(ts.matches("fetch(").count(), 1, "every request goes through httpRequest:\n{ts}");
        assert!(!ts.contains("new Error("), "{ts}");
    }
}

#[test]
fn a_module_with_no_entity_serves_its_crud_as_custom_ops() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "widget.rs", &crate::servers::tests::paged_crud_module_source("widget", "Store"));
    let config = two_surface_client_config(vec![ApiSurface {
        api_dir,
        service_import_path: "crate::api".to_string(),
        types_import_path: "crate::schema".to_string(),
        store_accessor: None,
        store_type: Some("Store".to_string()),
        pagination: Some(PaginationConfig { default_limit: 20, max_limit: 100 }),
        paginated_modules: Vec::new(),
        schema_dir: None,
    }]);
    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let bindings = tmp.path().join("bindings.ts");
    fs::write(&bindings, "export type Widget = { id: string };\n").unwrap();
    let transport_out = tmp.path().join("transport.ts");
    let http_out = tmp.path().join("http.ts");
    crate::clients::generators::transport::generate(&transport_out, &bindings, &modules, &config);
    crate::clients::generators::ts_client::generate(&http_out, &bindings, &modules, &config);
    let ts = fs::read_to_string(&transport_out).unwrap();
    let http = fs::read_to_string(&http_out).unwrap();

    for ts in [&ts, &http] {
        assert!(
            ts_method(ts, "widgetList").contains(
                "return callOp<PaginatedResult<Widget>>('GET', `/widgets${toQueryString({ opArg: { limit, offset } \
                 })}`);"
            ),
            "{ts}"
        );
        assert!(
            ts_method(ts, "widgetUpdate")
                .contains("return callOp<Widget>('PATCH', `/widgets/${encodeURIComponent(id)}`, { input });"),
            "{ts}"
        );
        assert!(!ts.contains("httpPut"), "no generated route takes a PUT:\n{ts}");
        assert!(!ts.contains("flatten"), "no resource, no flattener:\n{ts}");
        assert!(ts.contains("export class JsonApiError"), "errors are JSON:API documents on every route:\n{ts}");
    }
}

#[test]
fn call_op_sends_meta_args_and_reads_meta_result() {
    let clients = jsonapi_clients(false, |_| {});
    for ts in [&clients.transport, &clients.http] {
        let call = ts_function(ts, "callOp");
        assert!(
            call.contains("function callOp<T>(method: string, path: string, args?: Record<string, unknown>)"),
            "{call}"
        );
        assert!(
            call.contains("httpRequest(method, path, args === undefined ? undefined : { meta: { args } })"),
            "no arguments, no body:\n{call}"
        );
        assert!(call.contains("if (res.status === 204) return null as T;"), "{call}");
        assert!(call.contains("return doc.meta.result;"), "{call}");
        assert!(!ts.contains("httpPut"), "{ts}");
    }
}

/// The `board` methods of both clients: each line of `expected` appears in
/// the method of the same name, in either client.
fn assert_board_calls(clients: &JsonApiClients, expected: &[(&str, &str)]) {
    for ts in [&clients.transport, &clients.http] {
        for (name, call) in expected {
            let method = ts_method(ts, name);
            assert!(method.contains(call), "expected `{call}` in:\n{method}");
        }
    }
}

#[test]
fn a_custom_get_puts_required_args_in_the_path_and_optional_ones_in_op_arg() {
    assert_board_calls(
        &jsonapi_clients(false, |_| {}),
        &[(
            "boardGetSummary",
            "return callOp<Task>('GET', `/boards/summary/${encodeURIComponent(id)}${toQueryString({ opArg: { \
             include_done: includeDone } })}`);",
        )],
    );
}

#[test]
fn a_custom_post_sends_every_arg_as_meta_args_and_no_query() {
    let clients = jsonapi_clients(false, |_| {});
    assert_board_calls(
        &clients,
        &[
            // Optional args too, keyed by their Rust names.
            ("boardArchive", "return callOp<Task>('POST', '/boards/archive', { task_id: taskId, reason });"),
            // An `*Input` arg under its Rust name, beside an optional one.
            ("boardImport", "return callOp<number>('POST', '/boards/import', { input, dry_run: dryRun });"),
            // No args, no body; `()` resolves to null.
            ("boardReset", "return callOp<null>('POST', '/boards/reset');"),
        ],
    );
    for ts in [&clients.transport, &clients.http] {
        assert!(ts_method(ts, "boardReset").contains("Promise<null>"), "{ts}");
    }
}

#[test]
fn crud_ops_with_no_entity_are_served_as_custom_ops() {
    assert_board_calls(
        &jsonapi_clients(true, |_| {}),
        &[
            (
                "boardList",
                "return callOp<PaginatedResult<Task>>('GET', `/boards${toQueryString({ opArg: { limit, offset } })}`);",
            ),
            ("boardGetById", "return callOp<Task>('GET', `/boards/${encodeURIComponent(id)}`);"),
            ("boardCreate", "return callOp<Task>('POST', '/boards', { input });"),
            ("boardUpdate", "return callOp<Task>('PATCH', `/boards/${encodeURIComponent(id)}`, { input });"),
            ("boardDelete", "return callOp<null>('DELETE', `/boards/${encodeURIComponent(id)}`);"),
        ],
    );
    // Unpaginated, the list is the plain route.
    assert_board_calls(&jsonapi_clients(false, |_| {}), &[("boardList", "return callOp<Task[]>('GET', '/boards');")]);
}

#[test]
fn junction_ops_are_served_as_custom_ops_at_their_nested_routes() {
    let parent = "`/boards/${encodeURIComponent(boardId)}/tags";
    assert_board_calls(
        &jsonapi_clients(true, |_| {}),
        &[
            (
                "boardListTags",
                &format!(
                    "return callOp<PaginatedResult<Tag>>('GET', {parent}${{toQueryString({{ opArg: {{ limit, offset }} }})}}`);"
                ),
            ),
            ("boardAddTag", &format!("return callOp<null>('POST', {parent}`, {{ tag_id: tagId }});")),
            ("boardRemoveTag", &format!("return callOp<null>('DELETE', {parent}/${{encodeURIComponent(tagId)}}`);")),
        ],
    );
    assert_board_calls(
        &jsonapi_clients(false, |_| {}),
        &[("boardListTags", &format!("return callOp<Tag[]>('GET', {parent}`);"))],
    );
}

#[test]
fn a_scoped_junction_op_keeps_its_unscoped_path_under_the_prefix() {
    let ts = jsonapi_clients(false, |config| {
        config.route_prefix = Some(crate::servers::RoutePrefix {
            segments: "projects/:project_id".to_string(),
            state_accessor: "store_for".to_string(),
            params: vec![crate::servers::PrefixParam {
                name: "project_id".to_string(),
                rust_type: "String".to_string(),
                ts_type: "string".to_string(),
            }],
        });
    })
    .transport;
    assert!(
        ts_method(&ts, "boardAddTag").contains(
            "async boardAddTag(boardId: string, tagId: string, projectId?: string): Promise<null> {\n      return \
             callOp<null>('POST', scopedPath(projectId, `/boards/${encodeURIComponent(boardId)}/tags`), { tag_id: \
             tagId });"
        ),
        "{ts}"
    );
    assert!(ts_method(&ts, "boardArchive").contains("callOp<Task>('POST', scopedPath(projectId, '/boards/archive'), "));
}

#[test]
fn an_entity_event_frame_is_flattened() {
    let ts = jsonapi_clients(false, |_| {}).transport;
    let subscribe = ts_method(&ts, "subscribeTaskChanges");
    assert!(
        subscribe.contains("        (frame) => flattenTask(frame as JsonApiResource),\n        handlers,"),
        "{subscribe}"
    );
    let sse = ts_function(&ts, "subscribeSse");
    assert!(sse.contains("  decode: (frame: unknown) => T,\n  handlers: SubscriptionHandlers<T>,"), "{sse}");
    assert!(sse.contains("data = decode(JSON.parse(event.data));"), "{sse}");
    // IPC frames stay flat.
    let ipc = &ts[ts.find("export function createIpcTransport").unwrap()..];
    assert!(!ipc.contains("flatten"), "{ipc}");
}

/// A transport for `modules` over the [`JSONAPI_SCHEMA`] resources.
fn transport_over_resources(modules: &[ApiModule], bindings: &str) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let entities =
        crate::schema::parse::parse_schema_source(JSONAPI_SCHEMA, std::path::Path::new("schema.rs")).unwrap();
    let mut config = two_surface_client_config(vec![ApiSurface {
        api_dir: tmp.path().join("api"),
        service_import_path: "crate::api".to_string(),
        types_import_path: "crate::schema".to_string(),
        store_accessor: None,
        store_type: Some("Store".to_string()),
        pagination: None,
        paginated_modules: Vec::new(),
        schema_dir: None,
    }]);
    config.resources = crate::resource::ResourceModel::build(&entities, &config.naming).unwrap();
    let (bindings_path, output) = (tmp.path().join("bindings.ts"), tmp.path().join("transport.ts"));
    fs::write(&bindings_path, bindings).unwrap();
    crate::clients::generators::transport::generate(&output, &bindings_path, modules, &config);
    fs::read_to_string(&output).unwrap()
}

/// `{module}` with one parameterless event op per `(name, item type)`.
fn event_module(module: &str, events: &[(&str, syn::Type)]) -> ApiModule {
    ApiModule {
        name: module.to_string(),
        functions: Vec::new(),
        events: events
            .iter()
            .map(|(name, ty)| crate::servers::parse::EventFn {
                name: (*name).to_string(),
                item_type: crate::servers::types::norm_type(ty),
                item_type_ast: ty.clone(),
                ..Default::default()
            })
            .collect(),
        is_singleton: false,
        has_count: false,
    }
}

#[test]
fn an_entity_only_an_event_carries_still_gets_a_flattener() {
    let module = event_module(
        "feed",
        &[("tag_added", syn::parse_quote!(crate::schema::Tag)), ("pulse", syn::parse_quote!(Pulse))],
    );
    let ts = transport_over_resources(
        &[module],
        "export type Tag = { slug: string; title: string };\nexport type Pulse = { n: number };\n",
    );
    assert!(ts.contains("const TAG_RESOURCE: JsonApiResourceDef = {"), "{ts}");
    assert!(ts.contains("function flattenTag(r: JsonApiResource): Tag {"), "{ts}");
    assert!(!ts.contains("TASK_RESOURCE"), "only the resources the transport reads:\n{ts}");
    assert!(ts_method(&ts, "subscribeTagAdded").contains("(frame) => flattenTag(frame as JsonApiResource),"), "{ts}");
    // Any other item is the frame's `meta.result`.
    assert!(ts_method(&ts, "subscribePulse").contains("(frame) => metaResult<Pulse>(frame),"), "{ts}");
    assert!(ts_function(&ts, "metaResult").contains("return (frame as { meta: { result: T } }).meta.result;"));
    // A parameterless op's global listener decodes its frames the same way.
    assert!(
        ts_method(&ts, "onTagAdded")
            .contains("try { callback(flattenTag(JSON.parse(event.data) as JsonApiResource)); }"),
        "{ts}"
    );
    assert!(ts_method(&ts, "onPulse").contains("try { callback(metaResult<Pulse>(JSON.parse(event.data))); }"), "{ts}");
}

#[test]
fn the_http_only_client_calls_the_server_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "auto_start.rs",
        "// ontogen:singleton\npub async fn get_status(store: &Store) -> Result<u64, anyhow::Error> { todo!() }\n",
    );
    write_synthetic_api(
        &api_dir,
        "skill_file.rs",
        "pub async fn publish(store: &Store, id: &str) -> Result<(), anyhow::Error> { todo!() }\n",
    );
    let config = two_surface_client_config(vec![ApiSurface {
        api_dir,
        service_import_path: "crate::api".to_string(),
        types_import_path: "crate::schema".to_string(),
        store_accessor: None,
        store_type: Some("Store".to_string()),
        pagination: None,
        paginated_modules: Vec::new(),
        schema_dir: None,
    }]);
    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let (bindings, output) = (tmp.path().join("bindings.ts"), tmp.path().join("http.ts"));
    fs::write(&bindings, "").unwrap();
    crate::clients::generators::ts_client::generate(&output, &bindings, &modules, &config);
    let http = fs::read_to_string(&output).unwrap();
    // A singleton keeps its singular segment; a multi-word module is kebab-case.
    assert!(ts_method(&http, "autoStartGetStatus").contains("callOp<number>('GET', '/auto-start/status');"), "{http}");
    assert!(
        ts_method(&http, "skillFilePublish").contains("callOp<null>('POST', '/skill-files/publish', { id });"),
        "{http}"
    );
}

/// The generated transport `packages/nuxt_admin_layer/tests/jsonapi-transport.test.ts`
/// drives against a stubbed `fetch`. It must match the generator: rerun with
/// `UPDATE_TS_FIXTURES=1` after an intended change and commit the result.
#[test]
fn ts_jsonapi_transport_fixture_is_current() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packages/nuxt_admin_layer/tests/fixtures");
    let clients = jsonapi_clients(true, |_| {});
    for (name, fresh) in
        [("jsonapi-transport.generated.ts", &clients.transport), ("jsonapi-bindings.ts", &clients.bindings)]
    {
        let committed_path = fixture_dir.join(name);
        if std::env::var_os("UPDATE_TS_FIXTURES").is_some() {
            fs::write(&committed_path, fresh).unwrap();
        }
        let committed = fs::read_to_string(&committed_path).unwrap_or_default();
        assert_eq!(&committed, fresh, "stale fixture {name}: rerun with UPDATE_TS_FIXTURES=1");
    }
}

/// `tag::list(store, title: &str)`, with the page when `paginated`, in the
/// resource fixture: the server's handlers, and the `HttpTauriIpcSplit` and
/// `HttpTs` output for the same modules.
fn filtered_tag_list(paginated: bool) -> (String, JsonApiClients) {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = crate::servers::tests::resource_fixture(tmp.path(), true);
    let page = if paginated { ", limit: Option<u64>, offset: Option<u64>" } else { "" };
    let mut tag = crate::servers::tests::app_error_crud_source("tag")
        .replace("store: &Store, limit: Option<u64>, offset: Option<u64>", &format!("store: &Store, title: &str{page}"))
        .replace("count(store: &Store)", "count(store: &Store, title: &str)");
    if !paginated {
        server.pagination = None;
        tag = tag.lines().filter(|l| !l.contains("fn count(")).map(|l| format!("{l}\n")).collect();
    }
    write_synthetic_api(&server.api_dir, "tag.rs", &tag);
    let http = crate::servers::tests::generate_http(tmp.path(), server.clone());

    let mut config = two_surface_client_config(vec![ApiSurface {
        api_dir: server.api_dir.clone(),
        service_import_path: "crate::api".to_string(),
        types_import_path: "crate::schema".to_string(),
        store_accessor: None,
        store_type: Some("Store".to_string()),
        pagination: server.pagination.clone(),
        paginated_modules: Vec::new(),
        schema_dir: None,
    }]);
    config.resources = server.resources.clone();
    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let bindings = tmp.path().join("bindings.ts");
    fs::write(&bindings, "export type Tag = { id: string; title: string };\n").unwrap();
    let (transport_out, http_out) = (tmp.path().join("transport.ts"), tmp.path().join("http.ts"));
    crate::clients::generators::transport::generate(&transport_out, &bindings, &modules, &config);
    crate::clients::generators::ts_client::generate(&http_out, &bindings, &modules, &config);
    let read = |path: &std::path::Path| fs::read_to_string(path).unwrap();
    let clients = JsonApiClients { transport: read(&transport_out), http: read(&http_out), bindings: String::new() };
    (http, clients)
}

/// The server's handler for the other `tag` CRUD ops, and the client calls
/// that reach them: all served as the `tags` resource.
fn assert_tag_crud_is_a_resource(http: &str, clients: &JsonApiClients) {
    let flat = crate::servers::tests::compact(http);
    assert!(
        flat.contains(&crate::servers::tests::compact(
            ".route(\"/api/tags/{id}\", get(tag_get_by_id).patch(tag_update).delete(tag_delete)"
        )),
        "{http}"
    );
    assert!(http.contains("fn tag_as_resource<'a>("), "{http}");
    for ts in [&clients.transport, &clients.http] {
        assert!(ts_method(ts, "tagGetById").contains("httpGet<JsonApiResourceDocument>(`/tags/"), "{ts}");
        assert!(ts_method(ts, "tagUpdate").contains("httpPatch<JsonApiResourceDocument>("), "{ts}");
        assert!(ts.contains("function flattenTag("), "{ts}");
    }
}

#[test]
fn a_filtered_resource_list_keeps_its_flat_shape_on_server_and_clients() {
    let (http, clients) = filtered_tag_list(false);

    let list = &http[http.find("async fn tag_list(").unwrap()..];
    let list = &list[..list.find("\n}\n").unwrap()];
    assert!(list.contains("title: Result<axum::extract::Query<String>, QueryRejection>,"), "{list}");
    assert!(list.contains("-> Result<Json<Vec<Tag>>, ErrorObject>"), "a bare array, no document:\n{list}");
    assert!(crate::servers::tests::compact(list).contains("tag::list(&ontogen_store,&title)"), "{list}");

    assert_eq!(
        ts_method(&clients.transport, "tagList"),
        "async tagList(title: string): Promise<Tag[]> {\n      return \
         httpGet(`/tags?title=${encodeURIComponent(title)}`);\n    },\n"
    );
    assert_eq!(
        ts_method(&clients.http, "tagList"),
        "async tagList(title: string): Promise<Tag[]> {\n    return \
         httpGet(`/tags?title=${encodeURIComponent(title)}`);\n  },\n"
    );
    assert_tag_crud_is_a_resource(&http, &clients);
}

#[test]
fn a_filtered_paginated_resource_list_keeps_its_flat_page_on_server_and_clients() {
    let (http, clients) = filtered_tag_list(true);

    let list = &http[http.find("async fn tag_list(").unwrap()..];
    let list = &list[..list.find("\n}\n").unwrap()];
    assert!(list.contains("ontogen_page: Result<axum::extract::Query<PaginationParams>, QueryRejection>,"), "{list}");
    assert!(list.contains("-> Result<Json<PaginatedResult<Tag>>, ErrorObject>"), "{list}");
    assert!(list.contains("items: ontogen_items,"), "{list}");
    assert!(http.contains("pub struct PaginatedResult<T: Serialize> {"), "{http}");

    let call = "httpGet(`/tags?title=${encodeURIComponent(title)}&${toQueryString({ limit, offset }).slice(1)}`);";
    assert_eq!(
        ts_method(&clients.transport, "tagList"),
        format!(
            "async tagList(title: string, limit?: number, offset?: number): Promise<PaginatedResult<Tag>> {{\n      \
             return {call}\n    }},\n"
        )
    );
    assert_eq!(
        ts_method(&clients.http, "tagList"),
        format!(
            "async tagList(title: string, limit?: number, offset?: number): Promise<PaginatedResult<Tag>> {{\n    \
             return {call}\n  }},\n"
        )
    );
    // The paginated `tasks` list beside it is served as a resource.
    assert!(ts_method(&clients.transport, "taskList").contains("httpGet<JsonApiPageDocument>"));
    assert_tag_crud_is_a_resource(&http, &clients);
}
