//! Tests for the clients module: TypeScript bindings, HTTP/IPC transport,
//! HTTP-only client, and admin-registry generators.
//!
//! Mirrors the structure of [`crate::servers::tests`] for the server-side
//! generators. Client-side test cases were relocated here as part of the
//! servers→clients split.

use std::collections::{BTreeMap, BTreeSet, HashMap};
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
        required_query_structs: Default::default(),
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
        required_query_structs: Default::default(),
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

/// Junction ops appended to the generated resource modules, each a
/// relationship of its module's resource (§9.1) but `list_drafts`, which
/// has no add or remove beside it and is a custom GET. `task`'s `labels`
/// lists the `Tag` entities; `workout_set`'s `tags` lists ids, its target
/// named by the relationship.
const RELATIONSHIP_OPS: [(&str, &str); 2] = [
    (
        "task.rs",
        "
pub async fn list_labels(store: &Store, task_id: &str) -> Result<Vec<crate::schema::Tag>, anyhow::Error> { todo!() }
pub async fn add_label(store: &Store, task_id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }
pub async fn remove_label(store: &Store, task_id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }
pub async fn list_drafts(store: &Store, task_id: &str) -> Result<Vec<Task>, anyhow::Error> { todo!() }
",
    ),
    (
        "workout_set.rs",
        "
pub async fn list_tags(store: &Store, set_id: &str) -> Result<Vec<String>, anyhow::Error> { todo!() }
pub async fn add_tag(store: &Store, set_id: &str, tag_id: String) -> Result<(), anyhow::Error> { todo!() }
pub async fn remove_tag(store: &Store, set_id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }
",
    ),
];

/// `tag`, written by hand over the `tags` resource: CRUD whose `list` takes
/// the `ListTagsQuery` struct and a bare `title_prefix`, paged when
/// `paginated` beside a `count` taking the same filter (§7.3).
fn filtered_tag_module(paginated: bool) -> String {
    let (page, count) = if paginated {
        (
            ", limit: Option<u64>, offset: Option<u64>",
            "pub async fn count(store: &Store, query: ListTagsQuery, title_prefix: &str) -> Result<u64, anyhow::Error> \
             { todo!() }\n",
        )
    } else {
        ("", "")
    };
    format!(
        "use crate::schema::{{CreateTagInput, ListTagsQuery, Tag, UpdateTagInput}};
use crate::store::Store;

pub async fn list(store: &Store, query: ListTagsQuery, title_prefix: &str{page}) -> Result<Vec<Tag>, anyhow::Error> {{ todo!() }}
{count}pub async fn get_by_id(store: &Store, id: &str) -> Result<Tag, anyhow::Error> {{ todo!() }}
pub async fn create(store: &Store, input: CreateTagInput) -> Result<Tag, anyhow::Error> {{ todo!() }}
pub async fn update(store: &Store, id: &str, input: UpdateTagInput) -> Result<Tag, anyhow::Error> {{ todo!() }}
pub async fn delete(store: &Store, id: &str) -> Result<(), anyhow::Error> {{ todo!() }}
"
    )
}

/// A module with no entity whose `list` of `item`s takes the parameters
/// `filter` (§10.4), paged when `paginated` beside a `count` taking the
/// same filter. `imports` are the schema types it names.
fn filtered_op_list_module(imports: &str, item: &str, filter: &str, paginated: bool) -> String {
    let (page, count) = if paginated {
        (
            ", limit: Option<u64>, offset: Option<u64>".to_string(),
            format!("pub async fn count(store: &Store, {filter}) -> Result<u64, anyhow::Error> {{ todo!() }}\n"),
        )
    } else {
        (String::new(), String::new())
    };
    format!(
        "use crate::schema::{{{imports}}};
use crate::store::Store;

pub async fn list(store: &Store, {filter}{page}) -> Result<Vec<{item}>, anyhow::Error> {{ todo!() }}
{count}"
    )
}

/// The filter structs [`filtered_list_modules`] name, in a source root the
/// type pool reads.
const FILTER_STRUCTS: &str = "\
#[derive(serde::Deserialize)]
pub struct ListTagsQuery {
    pub title: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct DigestQuery {
    pub since: Option<String>,
    pub done: Option<bool>,
}

#[derive(serde::Deserialize)]
pub struct StrictQuery {
    pub owner: String,
    pub since: Option<String>,
}

#[derive(serde::Deserialize)]
pub enum QueryMode {
    Open,
    Closed,
}
";

/// The filtered lists beside [`BOARD_MODULE`]: `tag`'s, served as its
/// resource ([`filtered_tag_module`]), and three in modules with no entity,
/// served as ops: `digest`'s takes only the `DigestQuery` struct, `inbox`'s
/// only a bare `owner_id`, `queue`'s only a bare `mode` whose unit enum type
/// has `Query` in its name but is no filter struct.
fn filtered_list_modules(paginated: bool) -> [(&'static str, String); 4] {
    [
        ("tag.rs", filtered_tag_module(paginated)),
        ("digest.rs", filtered_op_list_module("DigestQuery, Task", "Task", "query: DigestQuery", paginated)),
        ("inbox.rs", filtered_op_list_module("Tag", "Tag", "owner_id: &str", paginated)),
        ("queue.rs", filtered_op_list_module("QueryMode, Tag", "Tag", "mode: QueryMode", paginated)),
    ]
}

/// What [`jsonapi_clients`] generated.
struct JsonApiClients {
    transport: String,
    http: String,
    bindings: String,
}

/// The `HttpTauriIpcSplit` and `HttpTs` output for [`JSONAPI_SCHEMA`],
/// [`RELATIONSHIP_OPS`] and [`BOARD_MODULE`], through
/// the public `gen_api` → `gen_clients` path. With `paginated`, every list
/// pages. `adjust` edits the clients config before generation.
fn jsonapi_clients(paginated: bool, adjust: impl FnOnce(&mut crate::ClientsConfig)) -> JsonApiClients {
    jsonapi_stack(paginated, &[], adjust).1
}

/// The Axum server [`jsonapi_clients`]' clients call, generated by
/// `gen_servers` from the same API, `extra` modules (file name, source) and
/// clients config, and the clients themselves.
fn jsonapi_stack(
    paginated: bool,
    extra: &[(&str, &str)],
    adjust: impl FnOnce(&mut crate::ClientsConfig),
) -> (String, JsonApiClients) {
    let (servers, clients) = generate_jsonapi(paginated, extra, adjust, true);
    (servers.unwrap().http, clients)
}

/// [`jsonapi_stack`] with the Tauri IPC commands, generated beside the Axum
/// server, in place of the server.
fn ipc_stack(
    paginated: bool,
    extra: &[(&str, &str)],
    adjust: impl FnOnce(&mut crate::ClientsConfig),
) -> (String, JsonApiClients) {
    let (servers, clients) = generate_jsonapi(paginated, extra, adjust, true);
    (servers.unwrap().ipc, clients)
}

/// The servers [`generate_jsonapi`] generates: Axum's and the Tauri IPC
/// commands.
struct Servers {
    http: String,
    ipc: String,
}

/// [`jsonapi_stack`]'s clients alone, with no server generated beside them.
fn jsonapi_clients_with(
    paginated: bool,
    extra: &[(&str, &str)],
    adjust: impl FnOnce(&mut crate::ClientsConfig),
) -> JsonApiClients {
    generate_jsonapi(paginated, extra, adjust, false).1
}

/// [`jsonapi_stack`], generating the servers only with `server`.
fn generate_jsonapi(
    paginated: bool,
    extra: &[(&str, &str)],
    adjust: impl FnOnce(&mut crate::ClientsConfig),
    server: bool,
) -> (Option<Servers>, JsonApiClients) {
    let (servers, clients) = try_generate_jsonapi(paginated, extra, adjust, server);
    (servers.map(Result::unwrap), clients.unwrap())
}

/// [`generate_jsonapi`] with each stage's error kept: `gen_servers` runs
/// (with `server`) whether or not `gen_clients` failed.
fn try_generate_jsonapi(
    paginated: bool,
    extra: &[(&str, &str)],
    adjust: impl FnOnce(&mut crate::ClientsConfig),
    server: bool,
) -> (Option<Result<Servers, String>>, Result<JsonApiClients, String>) {
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
    for (file, ops) in RELATIONSHIP_OPS {
        let generated = fs::read_to_string(api_dir.join(file)).unwrap();
        fs::write(api_dir.join(file), generated + ops).unwrap();
    }
    for (file, source) in extra {
        fs::write(api_dir.join(file), source).unwrap();
    }
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
        ..crate::ClientsConfig::new(api_dir.clone(), "AppState", "crate::api", "crate::schema", "crate::AppState")
    };
    let filters = tmp.path().join("filters").join("src");
    fs::create_dir_all(&filters).unwrap();
    fs::write(filters.join("lib.rs"), FILTER_STRUCTS).unwrap();
    config.pool_extra_roots.push(filters);
    adjust(&mut config);
    let read = |path: &std::path::Path| fs::read_to_string(path).unwrap();
    let clients = crate::gen_clients(&crate::schema::schema_of(&entities), Some(&api), &[], &config)
        .map_err(|e| e.to_string())
        .map(|()| JsonApiClients {
            transport: read(&ts.join("transport.ts")),
            http: read(&ts.join("http.ts")),
            bindings: read(&bindings_path),
        });
    if !server {
        return (None, clients);
    }

    let server_out = tmp.path().join("http.rs");
    let ipc_out = tmp.path().join("ipc.rs");
    let servers = crate::ServersConfig {
        api_dir,
        state_type: config.state_type.clone(),
        service_import_path: config.service_import_path.clone(),
        types_import_path: config.types_import_path.clone(),
        state_import: config.state_import.clone(),
        naming: config.naming.clone(),
        generators: vec![
            crate::servers::ServerGenerator::HttpAxum { output: server_out.clone() },
            crate::servers::ServerGenerator::TauriIpc { output: ipc_out.clone() },
        ],
        sse_route_overrides: config.sse_route_overrides.clone(),
        route_prefix: config.route_prefix.clone(),
        store_type: config.store_type.clone(),
        store_import: config.store_import.clone(),
        pagination: config.pagination.clone(),
        extra_surfaces: Vec::new(),
        error_source_dir: None,
    };
    let servers = crate::gen_servers(&crate::schema::schema_of(&entities), Some(&api), &[], &servers)
        .map_err(|e| e.to_string())
        .map(|_| Servers { http: read(&server_out), ipc: read(&ipc_out) });
    (Some(servers), clients)
}

/// A `route_prefix` scoping every store-scoped op under `projects/{project_id}`.
fn scope_under_projects(config: &mut crate::ClientsConfig) {
    config.route_prefix = Some(crate::servers::RoutePrefix {
        segments: "projects/:project_id".to_string(),
        state_accessor: "store_for".to_string(),
        params: vec![crate::servers::PrefixParam {
            name: "project_id".to_string(),
            rust_type: "String".to_string(),
            ts_type: "string".to_string(),
        }],
    });
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
        "async taskList(options?: ListOptions<TaskSortKey>): Promise<Task[]> {\n      const { data } = await \
         httpGet<JsonApiCollectionDocument>(`/tasks${toQueryString({ sort: options?.sort })}`);\n      return \
         data.map(flattenTask);\n    },\n"
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
    assert!(
        ts_method(ts, "workoutSetList")
            .contains("httpGet<JsonApiCollectionDocument>(`/workout-sets${toQueryString({ sort: options?.sort })}`)")
    );

    let http = &clients.http;
    assert!(
        ts_method(http, "workoutSetList")
            .contains("httpGet<JsonApiCollectionDocument>(`/workout-sets${toQueryString({ sort: options?.sort })}`)")
    );
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
        "async taskList(limit?: number, offset?: number, options?: ListOptions<TaskSortKey>): \
         Promise<PaginatedResult<Task>> {\n      const { data, meta } = await \
         httpGet<JsonApiPageDocument>(`/tasks${toQueryString({ sort: options?.sort, page: { offset, limit } \
         })}`);\n      return { items: data.map(flattenTask), total: meta.total, limit: meta.limit, offset: \
         meta.offset };\n    },\n"
    );
    // The family form brackets percent-encoded member names.
    let qs = ts_function(&clients.transport, "toQueryString");
    assert!(qs.contains("push(`${encodeURIComponent(key)}%5B${encodeURIComponent(member)}%5D`, v);"), "{qs}");
    // `HttpTs` pages the same way.
    assert_eq!(
        ts_method(&clients.http, "taskList"),
        "async taskList(limit?: number, offset?: number, options?: ListOptions<TaskSortKey>): \
         Promise<PaginatedResult<Task>> {\n    const { data, meta } = await \
         httpGet<JsonApiPageDocument>(`/tasks${toQueryString({ sort: options?.sort, page: { offset, limit } \
         })}`);\n    return { items: data.map(flattenTask), total: meta.total, limit: meta.limit, offset: \
         meta.offset };\n  },\n"
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
        "httpGet<JsonApiPageDocument>(scopedPath(projectId, `/tasks${toQueryString({ sort: options?.sort, page: { \
         offset, limit } })}`))"
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

/// The statement a method of the `HttpTs` client makes its call with.
fn http_call<'a>(http: &'a str, name: &str) -> &'a str {
    ts_method(http, name).lines().nth(1).unwrap().trim()
}

/// A junction op outside a resource module keeps its nested route under the
/// prefix: the transport calls it through `scopedPath` with no branch, and
/// the HTTP-only client calls the same path unscoped.
#[test]
fn a_scoped_junction_op_outside_a_resource_calls_its_nested_route_under_the_prefix() {
    let clients = jsonapi_clients(true, scope_under_projects);
    let parent = "`/boards/${encodeURIComponent(boardId)}/tags";
    let page = "${toQueryString({ opArg: { limit, offset } })}";
    for (name, signature, method, path, args) in [
        (
            "boardListTags",
            "boardId: string, limit?: number, offset?: number, projectId?: string): Promise<PaginatedResult<Tag>>",
            "callOp<PaginatedResult<Tag>>('GET'",
            format!("{parent}{page}`"),
            "",
        ),
        (
            "boardAddTag",
            "boardId: string, tagId: string, projectId?: string): Promise<null>",
            "callOp<null>('POST'",
            format!("{parent}`"),
            ", { tag_id: tagId }",
        ),
        (
            "boardRemoveTag",
            "boardId: string, tagId: string, projectId?: string): Promise<null>",
            "callOp<null>('DELETE'",
            format!("{parent}/${{encodeURIComponent(tagId)}}`"),
            "",
        ),
    ] {
        assert_eq!(
            ts_method(&clients.transport, name),
            format!(
                "async {name}({signature} {{\n      return {method}, scopedPath(projectId, {path}){args});\n    }},\n"
            )
        );
        assert_eq!(http_call(&clients.http, name), format!("return {method}, {path}{args});"), "{}", clients.http);
    }
    let http = &clients.transport[clients.transport.find("export function createHttpTransport").unwrap()..];
    assert!(!http.contains("if (projectId)"), "no call branches on the prefix argument:\n{http}");
}

/// The statements of `taskListLabels`, whose relationship lists entities,
/// as [`method_statements`] gives them.
fn labels_list(paginated: bool, scoped: bool) -> String {
    let path = "`/tasks/${encodeURIComponent(taskId)}/labels";
    let fetch = |path: String| if scoped { format!("scopedPath(projectId, {path})") } else { path };
    if paginated {
        format!(
            "const {{ data, meta }} = await httpGet<JsonApiPageDocument>({});\nreturn {{ items: data.map(flattenTag), \
             total: meta.total, limit: meta.limit, offset: meta.offset }};",
            fetch(format!("{path}${{toQueryString({{ page: {{ offset, limit }} }})}}`"))
        )
    } else {
        format!(
            "const {{ data }} = await httpGet<JsonApiCollectionDocument>({});\nreturn data.map(flattenTag);",
            fetch(format!("{path}`"))
        )
    }
}

/// The statements of the method `name` of the object literal `ts`, without
/// their indent.
fn method_statements(ts: &str, name: &str) -> String {
    let method = ts_method(ts, name);
    let body = &method[method.find("{\n").unwrap() + 2..method.rfind("\n").unwrap()];
    let body = &body[..body.rfind('\n').unwrap()];
    body.lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

/// A junction op of a resource module calls its relationship's routes
/// (§14.2): a list of entities reads the related resources and flattens the
/// target, a list of ids reads the linkage, each paged with the `page`
/// family when the module paginates; an add or remove sends one identifier
/// to the linkage and resolves `null`. The `Transport` signatures are the
/// junction ops' own.
#[test]
fn a_resource_junction_op_calls_its_relationship_route() {
    for paginated in [false, true] {
        let clients = jsonapi_clients(paginated, |_| {});
        let (page, labels, tags) = if paginated {
            (", limit?: number, offset?: number", "PaginatedResult<Tag>", "PaginatedResult<string>")
        } else {
            ("", "Tag[]", "string[]")
        };
        let ts = &clients.transport;
        for (signature, interface) in [
            (format!("taskListLabels(taskId: string{page})"), labels),
            (format!("workoutSetListTags(setId: string{page})"), tags),
            ("taskAddLabel(taskId: string, tagId: string)".to_string(), "null"),
            ("taskRemoveLabel(taskId: string, tagId: string)".to_string(), "null"),
            ("workoutSetAddTag(setId: string, tagId: string)".to_string(), "null"),
            ("workoutSetRemoveTag(setId: string, tagId: string)".to_string(), "null"),
        ] {
            assert!(ts.contains(&format!("  {signature}: Promise<{interface}>;\n")), "{signature}:\n{ts}");
        }

        let ids = if paginated {
            "const { data, meta } = await httpGet<JsonApiPageDocument<JsonApiResourceIdentifier>>(\
             `/workout-sets/${encodeURIComponent(setId)}/relationships/tags${toQueryString({ page: { offset, limit } \
             })}`);\nreturn { items: data.map((i) => i.id), total: meta.total, limit: meta.limit, offset: meta.offset };"
        } else {
            "const { data } = await httpGet<JsonApiCollectionDocument<JsonApiResourceIdentifier>>(\
             `/workout-sets/${encodeURIComponent(setId)}/relationships/tags`);\nreturn data.map((i) => i.id);"
        };
        let labels_linkage = "`/tasks/${encodeURIComponent(taskId)}/relationships/labels`";
        let tags_linkage = "`/workout-sets/${encodeURIComponent(setId)}/relationships/tags`";
        let label = "{ data: [{ type: 'tags', id: tagId }] }";
        for ts in [&clients.transport, &clients.http] {
            assert_eq!(method_statements(ts, "taskListLabels"), labels_list(paginated, false));
            assert_eq!(method_statements(ts, "workoutSetListTags"), ids);
            for (name, call) in [
                ("taskAddLabel", format!("httpPost({labels_linkage}, {label})")),
                ("taskRemoveLabel", format!("httpDelete({labels_linkage}, {label})")),
                ("workoutSetAddTag", format!("httpPost({tags_linkage}, {label})")),
                ("workoutSetRemoveTag", format!("httpDelete({tags_linkage}, {label})")),
            ] {
                assert_eq!(method_statements(ts, name), format!("await {call};\nreturn null;"));
            }
        }
    }
}

/// Scoped, a relationship's calls take the same paths through `scopedPath`.
#[test]
fn a_scoped_resource_junction_op_calls_its_relationship_route_under_the_prefix() {
    let clients = jsonapi_clients(true, scope_under_projects);
    let ts = &clients.transport;
    assert!(ts_method(ts, "taskListLabels").starts_with(
        "async taskListLabels(taskId: string, limit?: number, offset?: number, projectId?: string): \
         Promise<PaginatedResult<Tag>> {"
    ));
    assert_eq!(method_statements(ts, "taskListLabels"), labels_list(true, true));
    assert_eq!(
        method_statements(ts, "taskRemoveLabel"),
        "await httpDelete(scopedPath(projectId, `/tasks/${encodeURIComponent(taskId)}/relationships/labels`), { data: \
         [{ type: 'tags', id: tagId }] });\nreturn null;"
    );
    assert_eq!(method_statements(&clients.http, "taskListLabels"), labels_list(true, false));
}

/// `workout_set` whose junction ops take the state rather than a store,
/// beside a store-scoped `get_by_id`.
const STATE_JUNCTION_MODULE: &str = "\
use crate::schema::WorkoutSet;
use crate::store::Store;
use crate::AppState;

pub async fn get_by_id(store: &Store, id: &str) -> Result<WorkoutSet, anyhow::Error> { todo!() }
pub async fn list_tags(state: &AppState, set_id: &str) -> Result<Vec<String>, anyhow::Error> { todo!() }
pub async fn add_tag(state: &AppState, set_id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }
";

/// A relationship's routes are its resource's, served under the prefix when
/// its `get_by_id` is, whatever its junction ops take.
#[test]
fn a_relationship_is_scoped_as_its_resources_get_by_id() {
    let clients = jsonapi_clients_with(false, &[("workout_set.rs", STATE_JUNCTION_MODULE)], scope_under_projects);
    let ts = &clients.transport;
    assert_eq!(
        ts_method(ts, "workoutSetAddTag"),
        "async workoutSetAddTag(setId: string, tagId: string, projectId?: string): Promise<null> {\n      await \
         httpPost(scopedPath(projectId, `/workout-sets/${encodeURIComponent(setId)}/relationships/tags`), { data: [{ \
         type: 'tags', id: tagId }] });\n      return null;\n    },\n"
    );
    assert!(ts_method(ts, "workoutSetListTags").contains("(scopedPath(projectId, `/workout-sets/"), "{ts}");
}

const STORE: &str = "store: &Store";
const STATE: &str = "state: &AppState";

/// `tag`, its `list`, `get_by_id`, `create`, `update` and `delete` taking
/// [`STORE`] or [`STATE`] as given, in that order.
fn tag_module([list, get, create, update, delete]: [&str; 5]) -> String {
    format!(
        "use crate::schema::{{CreateTagInput, Tag, UpdateTagInput}};\nuse crate::store::Store;\nuse \
         crate::AppState;\n\npub async fn list({list}) -> Result<Vec<Tag>, anyhow::Error> {{ todo!() }}\npub async fn \
         get_by_id({get}, id: &str) -> Result<Tag, anyhow::Error> {{ todo!() }}\npub async fn create({create}, input: \
         CreateTagInput) -> Result<Tag, anyhow::Error> {{ todo!() }}\npub async fn update({update}, id: &str, input: \
         UpdateTagInput) -> Result<Tag, anyhow::Error> {{ todo!() }}\npub async fn delete({delete}, id: &str) -> \
         Result<(), anyhow::Error> {{ todo!() }}\n"
    )
}

/// `workout_set` with an unscoped `get_by_id` and `others`.
fn unscoped_workout_set(others: &str) -> String {
    format!(
        "use crate::schema::{{UpdateWorkoutSetInput, WorkoutSet}};\nuse crate::store::Store;\nuse \
         crate::AppState;\n\npub async fn get_by_id(state: &AppState, id: &str) -> Result<WorkoutSet, anyhow::Error> \
         {{ todo!() }}\n{others}"
    )
}

/// The errors `gen_servers` and `gen_clients` each fail with for `extra`
/// under the route prefix; with no prefix, both accept it.
fn scoping_errors(extra: &[(&str, &str)]) -> [String; 2] {
    let (servers, clients) = try_generate_jsonapi(false, extra, |_| {}, true);
    assert!(servers.unwrap().is_ok() && clients.is_ok(), "with no route prefix nothing is scoped");
    let (servers, clients) = try_generate_jsonapi(false, extra, scope_under_projects, true);
    [servers.unwrap().err().expect("gen_servers refuses it"), clients.err().expect("gen_clients refuses it")]
}

/// Under a route prefix, a resource's `list`, `create` and `update` are
/// served in its `get_by_id`'s scope: each resource they answer links to
/// itself in their own scope, where only that `get_by_id` could serve it.
/// `delete` answers no document, so its scope is free.
#[test]
fn under_a_route_prefix_a_resource_links_only_to_routes_its_scope_serves() {
    for (fns, culprit) in [
        ([STORE, STATE, STORE, STORE, STORE], "list"),
        ([STATE, STORE, STORE, STORE, STORE], "list"),
        ([STORE, STORE, STATE, STORE, STORE], "create"),
        ([STORE, STORE, STORE, STATE, STORE], "update"),
    ] {
        for err in scoping_errors(&[("tag.rs", &tag_module(fns))]) {
            assert!(err.contains(&format!("`tag::{culprit}` is served ")), "{culprit}: {err}");
            assert!(err.contains("take a store in both fns or in neither"), "{culprit}: {err}");
        }
    }
    let [err, _] = scoping_errors(&[("tag.rs", &tag_module([STATE, STORE, STORE, STORE, STORE]))]);
    assert!(
        err.ends_with(
            "ontogen: `tag::list` is served outside the route prefix `projects/:project_id` (it takes no store) and \
             `tag::get_by_id` only under it (it takes a store), but every resource `tag::list` answers links to \
             itself in `tag::list`'s scope, where no `get_by_id` route is served; take a store in both fns or in \
             neither"
        ),
        "{err}"
    );
    let (servers, clients) = try_generate_jsonapi(
        false,
        &[("tag.rs", &tag_module([STORE, STORE, STORE, STORE, STATE]))],
        scope_under_projects,
        true,
    );
    assert!(servers.unwrap().is_ok() && clients.is_ok(), "a scope apart for `delete` is fine");
}

/// Under a route prefix, a resource whose `get_by_id` takes no store serves
/// its relationship routes outside the prefix, so they may neither link to a
/// target served only under it nor call a junction op or `update` that takes
/// a store, which no unscoped handler can open.
#[test]
fn under_a_route_prefix_unscoped_relationship_routes_reach_nothing_scoped() {
    let [server, client] = scoping_errors(&[("workout_set.rs", &unscoped_workout_set(""))]);
    for err in [&server, &client] {
        assert!(
            err.ends_with(
                "ontogen: `workout_set::get_by_id` takes no store, so the relationship routes of the JSON:API \
                 resource `workout-sets` are served outside the route prefix `projects/:project_id`, but its \
                 relationship `tag` links `tags` resources, which `tag::get_by_id` serves only under the prefix (it \
                 takes a store); take a store in `workout_set::get_by_id`, or the state in `tag::get_by_id`"
            ),
            "{err}"
        );
    }

    let unscoped_tags = tag_module([STATE; 5]);
    for (ops, culprit) in [
        (
            "pub async fn list_tags(store: &Store, set_id: &str) -> Result<Vec<String>, anyhow::Error> { todo!() }\n\
             pub async fn add_tag(state: &AppState, set_id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }\n",
            "list_tags",
        ),
        (
            "pub async fn list_tags(state: &AppState, set_id: &str) -> Result<Vec<String>, anyhow::Error> { todo!() }\n\
             pub async fn remove_tag(store: &Store, set_id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }\n",
            "remove_tag",
        ),
    ] {
        let module = unscoped_workout_set(ops);
        for err in scoping_errors(&[("workout_set.rs", &module), ("tag.rs", &unscoped_tags)]) {
            assert!(
                err.ends_with(&format!(
                    "ontogen: `workout_set::{culprit}` takes a store, which only a handler under the route prefix \
                     `projects/:project_id` opens, but it serves the relationship `tags` of the JSON:API resource \
                     `workout-sets`, whose routes are served outside the prefix because `workout_set::get_by_id` \
                     takes no store; take a store in `workout_set::get_by_id`, or the state in \
                     `workout_set::{culprit}`"
                )),
                "{culprit}: {err}"
            );
        }
    }

    let update = unscoped_workout_set(
        "pub async fn update(store: &Store, id: &str, input: UpdateWorkoutSetInput) -> Result<WorkoutSet, \
         anyhow::Error> { todo!() }\n",
    );
    for err in scoping_errors(&[("workout_set.rs", &update), ("tag.rs", &unscoped_tags)]) {
        assert!(
            err.ends_with(
                "ontogen: `workout_set::update` is served under the route prefix `projects/:project_id` (it takes a \
                 store) and `workout_set::get_by_id` only outside it (it takes no store), but every resource \
                 `workout_set::update` answers links to itself in `workout_set::update`'s scope, where no \
                 `get_by_id` route is served; take a store in both fns or in neither"
            ),
            "{err}"
        );
    }
}

/// Scoped relationship routes may reach what is served outside the prefix:
/// a target whose `get_by_id` takes no store is linked at its unscoped
/// collection, and junction ops taking the state are called with it.
#[test]
fn scoped_relationship_routes_reach_unscoped_targets_and_junction_ops() {
    let unscoped_tags = tag_module([STATE; 5]);
    let (servers, clients) = try_generate_jsonapi(
        false,
        &[("workout_set.rs", STATE_JUNCTION_MODULE), ("tag.rs", &unscoped_tags)],
        scope_under_projects,
        true,
    );
    assert!(clients.is_ok(), "{:?}", clients.err());
    let http = servers.unwrap().unwrap().http;
    let related = &http[http.find("async fn ontogen_workout_set_related_get_scoped(").expect(&http)..];
    let related = &related[..related.find("\n}\n").unwrap()];
    assert!(related.contains("\"/api/tags\""), "the tags are linked where they are served:\n{related}");
    assert!(!related.contains("/tags\", encode_path_segment(&ontogen_scope"), "{related}");
    let post = &http[http.find("async fn ontogen_workout_set_relationship_post_scoped(").unwrap()..];
    let post = &post[..post.find("\n}\n").unwrap()];
    assert!(post.contains("add_tag(&ontogen_state, "), "the junction op takes the state:\n{post}");
}

/// `workout_set` with a `get_by_id`, a `create` and an `update` taking the
/// given first arguments, each left out when `None`.
fn workout_set_module(get: Option<&str>, create: Option<&str>, update: Option<&str>) -> String {
    let get = get
        .map(|a| format!("pub async fn get_by_id({a}, id: &str) -> Result<WorkoutSet, anyhow::Error> {{ todo!() }}\n"));
    let create = create.map(|a| {
        format!("pub async fn create({a}, input: CreateWorkoutSetInput) -> Result<WorkoutSet, anyhow::Error> {{ todo!() }}\n")
    });
    let update = update.map(|a| {
        format!(
            "pub async fn update({a}, id: &str, input: UpdateWorkoutSetInput) -> Result<WorkoutSet, anyhow::Error> \
             {{ todo!() }}\n"
        )
    });
    format!(
        "use crate::schema::{{CreateWorkoutSetInput, UpdateWorkoutSetInput, WorkoutSet}};\nuse \
         crate::store::Store;\nuse crate::AppState;\n\n{}{}{}",
        get.unwrap_or_default(),
        create.unwrap_or_default(),
        update.unwrap_or_default()
    )
}

/// Under a route prefix, a create or update that takes no store is served
/// outside the prefix, where it checks each id it links with its target's
/// `get_by_id`: a target whose `get_by_id` takes a store is out of its
/// reach, whether or not its own module serves `get_by_id`.
#[test]
fn under_a_route_prefix_an_unscoped_write_links_nothing_scoped() {
    for (module, culprit) in [
        (workout_set_module(None, Some(STATE), None), "create"),
        (workout_set_module(None, None, Some(STATE)), "update"),
        (workout_set_module(Some(STATE), Some(STATE), None), "create"),
    ] {
        for err in scoping_errors(&[("workout_set.rs", &module)]) {
            assert!(
                err.ends_with(&format!(
                    "ontogen: `workout_set::{culprit}` takes no store, so it is served outside the route prefix \
                     `projects/:project_id`, but it checks that the `tags` resources its relationship `tag` links \
                     exist with `tag::get_by_id`, which takes a store that only a handler under the prefix opens; \
                     take a store in `workout_set::{culprit}`, or the state in `tag::get_by_id`"
                )),
                "{culprit}: {err}"
            );
        }
    }

    let unscoped_tags = tag_module([STATE; 5]);
    let (servers, clients) = try_generate_jsonapi(
        false,
        &[("workout_set.rs", &workout_set_module(Some(STATE), Some(STATE), Some(STATE))), ("tag.rs", &unscoped_tags)],
        scope_under_projects,
        true,
    );
    assert!(clients.is_ok(), "{:?}", clients.err());
    let http = servers.unwrap().unwrap().http;
    assert!(http.contains("tag::get_by_id(state, &linked.id)"), "an unscoped target is read with the state:\n{http}");
    assert!(!http.contains("state.store()"), "no store is opened outside a scope:\n{http}");
}

/// The reverse is fine: a create or update under the prefix checks a target
/// whose `get_by_id` takes no store with the state, as an unscoped one
/// checks a target served outside the prefix.
#[test]
fn under_a_route_prefix_a_scoped_write_links_unscoped_targets() {
    let unscoped_tags = tag_module([STATE; 5]);
    let (servers, clients) = try_generate_jsonapi(
        false,
        &[("workout_set.rs", &workout_set_module(Some(STORE), Some(STORE), Some(STORE))), ("tag.rs", &unscoped_tags)],
        scope_under_projects,
        true,
    );
    assert!(clients.is_ok(), "{:?}", clients.err());
    let http = servers.unwrap().unwrap().http;
    let check = &http[http.find("async fn ontogen_workout_set_check_linked_scoped(").expect(&http)..];
    let check = &check[..check.find("\n}\n").unwrap()];
    assert!(check.contains("tag::get_by_id(state, &linked.id)"), "{check}");
    assert!(!check.contains("store_for"), "the state reaches an unscoped target:\n{check}");
    assert!(!http.contains("async fn ontogen_workout_set_check_linked("), "no write is unscoped:\n{http}");
}

/// A `list_X` with no add or remove beside it is a custom GET at its action
/// route, unpaged and returning its plain array, on every transport (§15).
#[test]
fn a_lone_list_x_is_a_custom_get_returning_its_plain_array() {
    let clients = jsonapi_clients(true, |_| {});
    let ts = &clients.transport;
    assert!(ts.contains("  taskListDrafts(taskId: string): Promise<Task[]>;\n"), "{ts}");
    for ts in [&clients.transport, &clients.http] {
        assert_eq!(
            method_statements(ts, "taskListDrafts"),
            "return callOp<Task[]>('GET', `/tasks/list-drafts/${encodeURIComponent(taskId)}`);"
        );
    }
    let ipc = &ts[ts.find("export function createIpcTransport").unwrap()..];
    assert_eq!(method_statements(ipc, "taskListDrafts"), "return invoke('task_list_drafts', { taskId });");
}

/// A junction list of entities flattens its target's resources, so the
/// target's flattener is emitted even when none of the target's own methods
/// is.
#[test]
fn a_junction_list_of_entities_emits_its_targets_flattener() {
    let skip_tags = |config: &mut crate::ClientsConfig| {
        config.ts_skip_commands =
            ["list", "get_by_id", "create", "update", "delete"].iter().map(|op| format!("tag_{op}")).collect();
    };
    let clients = jsonapi_clients_with(false, &[], skip_tags);
    for ts in [&clients.transport, &clients.http] {
        assert!(!ts.contains("async tagGetById("), "{ts}");
        assert!(ts.contains("function flattenTag(r: JsonApiResource): Tag {"), "{ts}");
        assert!(ts_method(ts, "taskListLabels").contains("data.map(flattenTag)"), "{ts}");
    }
    let skip_labels = |config: &mut crate::ClientsConfig| {
        skip_tags(config);
        config.ts_skip_commands.push("task_list_labels".to_string());
    };
    let clients = jsonapi_clients_with(false, &[], skip_labels);
    assert!(!clients.transport.contains("function flattenTag("), "{}", clients.transport);
}

/// An HTTP call: its method and its path, each path parameter written `{}`.
type Call = (String, String);

/// Every `(METHOD, path)` the generated Axum router `server` registers, each
/// path parameter written `{}` but a relationship name, `{rel}`, with the
/// handler serving it.
fn server_routes(server: &str) -> BTreeMap<Call, String> {
    let flat = crate::servers::tests::compact(server);
    let mut routes = BTreeMap::new();
    for route in flat.split(".route(\"").skip(1) {
        let (path, rest) = route.split_once("\",").unwrap();
        let handlers = &rest[..rest.find(".fallback(").unwrap_or_else(|| panic!("no fallback on {path}"))];
        let handlers = handlers.strip_prefix("axum::routing::").unwrap_or(handlers);
        for method in ["get", "post", "put", "patch", "delete"] {
            let at = if handlers.starts_with(&format!("{method}(")) {
                Some(0)
            } else {
                handlers.find(&format!(".{method}(")).map(|at| at + 1)
            };
            if let Some(at) = at {
                let handler = &handlers[at + method.len() + 1..];
                let handler = &handler[..handler.find(')').unwrap()];
                routes.insert((method.to_uppercase(), route_template(path)), handler.to_string());
            }
        }
    }
    routes
}

/// The argument names a request carries outside its path: its bare
/// `filter[…]` members, the `*Query` struct whose fields are its other
/// `filter[…]` members (named by its type's last segment), whether it sends
/// `sort`, whether it pages with the `page` family, its `opArg[…]` members
/// and its `meta.args` keys.
#[derive(Debug, Default, PartialEq)]
struct ArgNames {
    filter: BTreeSet<String>,
    filter_struct: Option<String>,
    sort: bool,
    page: bool,
    op_args: BTreeSet<String>,
    meta_args: BTreeSet<String>,
}

/// What the generated handler `handler` in `server` reads outside its path:
/// the `filter`, `filter_fields`, `page` and `op_args` of its `RouteQuery`
/// spec, whether it reads an order from `sort` (`sort_order`; a handler that
/// refuses `sort` reads none), and the `meta.args` keys it checks
/// (`request::check_op_arg_names`), with those it requires.
fn server_args(server: &str, handler: &str) -> (ArgNames, BTreeSet<String>) {
    let flat = crate::servers::tests::compact(server);
    let code =
        &server[server.find(&format!("async fn {handler}(")).unwrap_or_else(|| panic!("no {handler} in:\n{server}"))..];
    let code = crate::servers::tests::compact(&code[..code.find("\n}\n").unwrap()]);
    let (signature, body) = code.split_once(")->").unwrap();
    let strings = |list: &str| -> BTreeSet<String> {
        list.split(',').filter(|s| !s.is_empty()).map(|s| s.trim_matches('"').to_string()).collect()
    };
    let mut names = ArgNames { sort: body.contains(".sort_order"), ..ArgNames::default() };
    // A relationship GET parses its raw query per relationship; it reads the
    // `page` family when any relationship it serves pages.
    if body.contains("QueryParams::parse(") {
        names.page = body.contains("page:true");
    }
    // The JSON:API `Query<Spec>` it extracts, not Axum's own.
    for (at, _) in signature.match_indices("OntogenQuery<") {
        let spec = &signature[at + "OntogenQuery<".len()..];
        let spec = &spec[..spec.find('>').unwrap()];
        let Some(spec_impl) = flat.split_once(&format!("implOntogenRouteQueryfor{spec}{{")).map(|(_, rest)| rest)
        else {
            continue;
        };
        let spec_impl = &spec_impl[..spec_impl.find('}').unwrap()];
        if let Some((_, list)) = spec_impl.split_once("filter:&[") {
            names.filter = strings(&list[..list.find(']').unwrap()]);
        }
        if let Some((_, ty)) = spec_impl.split_once("filter_fields::<") {
            let ty = &ty[..ty.find('>').unwrap()];
            names.filter_struct = ty.rsplit("::").next().map(ToString::to_string);
        }
        names.page = spec_impl.contains("page:true");
        if let Some((_, list)) = spec_impl.split_once("op_args:&[") {
            names.op_args = strings(&list[..list.find(']').unwrap()]);
        }
    }
    let mut required = BTreeSet::new();
    if let Some((_, list)) = body.split_once("request::check_op_arg_names(&ontogen_args,&[") {
        names.meta_args = strings(&list[..list.find(']').unwrap()]);
        for read in body.split("request::op_arg::<").skip(1) {
            let (_, read) = read.split_once("(&ontogen_args,\"").unwrap();
            let (name, read) = read.split_once("\",").unwrap();
            if read.starts_with("true") {
                required.insert(name.to_string());
            }
        }
    }
    (names, required)
}

/// `path` with every `{name}` or `${…}` parameter written `{}`, but a
/// server route's `{rel}`, and its query string dropped.
fn route_template(path: &str) -> String {
    let path = path.find("${toQueryString(").or_else(|| path.find('?')).map_or(path, |end| &path[..end]);
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let open = if rest[..start].ends_with('$') { start - 1 } else { start };
        out.push_str(&rest[..open]);
        let end = start + rest[start..].find('}').unwrap() + 1;
        out.push_str(if &rest[open..end] == "{rel}" { "{rel}" } else { "{}" });
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The handler of `routes` that serves `call`, and the relationship name
/// the call puts where the route captures `{rel}`, if it does: a
/// relationship route serves every name of its type, so the call's literal
/// segment stands in for `{rel}`.
fn route_for<'r>(routes: &'r BTreeMap<Call, String>, (method, path): &Call) -> Option<(&'r str, Option<String>)> {
    if let Some(handler) = routes.get(&(method.clone(), path.clone())) {
        return Some((handler, None));
    }
    let segments: Vec<&str> = path.split('/').collect();
    routes.iter().filter(|((m, _), _)| m == method).find_map(|((_, route), handler)| {
        let route: Vec<&str> = route.split('/').collect();
        if route.len() != segments.len() {
            return None;
        }
        let mut rel = None;
        for (r, s) in route.iter().zip(&segments) {
            match *r {
                "{rel}" if *s != "{}" => rel = Some((*s).to_string()),
                r if r == *s => {}
                _ => return None,
            }
        }
        rel.map(|rel| (handler.as_str(), Some(rel)))
    })
}

/// The pair tests' reading of relationship routes: `{rel}` stands for the
/// name a call puts there, never for a path parameter, and a relationship
/// GET's raw query pages when it says so.
#[test]
fn the_route_parser_matches_a_relationship_name_against_rel() {
    let server = r#"
        .route(
            "/api/tasks/{id}/relationships/{rel}",
            get(ontogen_task_relationship_get).post(ontogen_task_relationship_post).fallback(allow([Method::GET])),
        )
        .route("/api/tasks/{id}/{rel}", get(ontogen_task_related_get).fallback(allow([Method::GET])))
        .route("/api/tasks/list-drafts/{task_id}", get(task_list_drafts).fallback(allow([Method::GET])))
async fn ontogen_task_related_get(path: Path<(LookupKey, LookupKey)>, raw: RawQuery) -> Response {
    let spec = match rel.as_str() {
        "labels" => QuerySpec { page: true, ..QuerySpec::NONE },
        _ => QuerySpec::NONE,
    };
    let query = QueryParams::parse(raw.as_deref(), &spec);
}
"#;
    let routes = server_routes(server);
    let call = |method: &str, path: &str| (method.to_string(), path.to_string());
    let found = |method: &str, path: &str| {
        route_for(&routes, &call(method, path)).map(|(handler, rel)| (handler.to_string(), rel))
    };
    assert_eq!(
        found("POST", "/api/tasks/{}/relationships/labels"),
        Some(("ontogen_task_relationship_post".to_string(), Some("labels".to_string())))
    );
    assert_eq!(
        found("GET", "/api/tasks/{}/labels"),
        Some(("ontogen_task_related_get".to_string(), Some("labels".to_string())))
    );
    assert_eq!(found("GET", "/api/tasks/list-drafts/{}"), Some(("task_list_drafts".to_string(), None)));
    assert_eq!(found("GET", "/api/tasks/{}/{}"), None, "a path parameter is no relationship name");
    assert_eq!(found("DELETE", "/api/tasks/{}/relationships/labels"), None);
    assert!(server_args(server, "ontogen_task_related_get").0.page);
}

/// The keys of the TS object literal `members` (`{ a, b: c }` without its
/// braces).
fn literal_keys(members: &str) -> BTreeSet<String> {
    members
        .split(',')
        .map(|member| member.split(':').next().unwrap().trim().to_string())
        .filter(|key| !key.is_empty())
        .collect()
}

/// The `(METHOD, path under /api)` a generated HTTP method `body` (its
/// name, signature and statements) calls, with its route-prefix argument
/// `projectId` given or not, and the argument names the call sends outside
/// its path.
fn client_call_and_args(body: &str, prefix_given: bool) -> Option<(Call, ArgNames)> {
    let signature = &body[..body.find("): ").unwrap()];
    let (method, args, op_call) = if let Some((_, call)) = body.split_once("callOp<") {
        let call = &call[call.find(">('").unwrap() + 3..];
        let (method, args) = call.split_once("', ").unwrap();
        (method.to_string(), args, true)
    } else {
        let (verb, call) = ["Get", "Post", "Patch", "Delete"]
            .iter()
            .find_map(|verb| body.split_once(&format!("http{verb}")).map(|(_, call)| (verb, call)))?;
        (verb.to_uppercase(), call[call.find('(').unwrap() + 1..].trim_start(), false)
    };
    let (scoped, literal) = match args.strip_prefix("scopedPath(projectId, ") {
        Some(literal) => (prefix_given, literal),
        None => (false, args),
    };
    let quote = literal.chars().next().unwrap();
    let path_end = literal[1..].find(quote).unwrap() + 1;
    let path = &literal[1..path_end];
    let prefix = if scoped { "/projects/{}" } else { "" };
    let mut names = ArgNames::default();
    // `filter: query`, `filter: { a, b: c }` or `filter: { ...query, a }`,
    // `query` being the `query?: X` parameter.
    if let Some((_, filter)) = path.split_once("toQueryString({ filter: ") {
        let members = match filter.strip_prefix('{') {
            Some(literal) => &literal[..literal.find('}').unwrap()],
            None => &filter[..filter.find([',', ' ']).unwrap()],
        };
        for member in members.split(',').map(str::trim).filter(|m| !m.is_empty()) {
            match member.trim_start_matches("...") {
                "query" => {
                    let (_, ty) = signature.split_once("query?: ").unwrap_or_else(|| panic!("no query in {signature}"));
                    names.filter_struct = Some(ty.split(',').next().unwrap().trim().to_string());
                }
                member => {
                    names.filter.insert(member.split(':').next().unwrap().trim().to_string());
                }
            }
        }
    }
    names.sort = path.contains("sort: options?.sort");
    names.page = path.contains("page: {");
    if let Some((_, members)) = path.split_once("opArg: {") {
        names.op_args = literal_keys(&members[..members.find('}').unwrap()]);
    }
    // `callOp`'s third argument is the request's `meta.args`.
    let after_path = literal[path_end + 1..].trim_start_matches(')');
    if op_call && let Some(members) = after_path.strip_prefix(", {") {
        names.meta_args = literal_keys(&members[..members.find('}').unwrap()]);
    }
    Some(((method, format!("/api{prefix}{}", route_template(path))), names))
}

/// `(name, body)` of every method of the object literal in `ts` that starts
/// at `head`, each method indented by `indent`.
fn object_methods<'a>(ts: &'a str, head: &str, indent: &str) -> Vec<(&'a str, &'a str)> {
    let object = &ts[ts.find(head).unwrap_or_else(|| panic!("no `{head}` in:\n{ts}"))..];
    let object = object.find("\n}\n").map_or(object, |end| &object[..end]);
    object
        .split(&format!("\n{indent}async "))
        .skip(1)
        .map(|method| (&method[..method.find('(').unwrap()], method))
        .collect()
}

/// Asserts that every call the `checked` methods of `clients` make reaches a
/// route `server` serves: the transport's, given the prefix argument when
/// `scoped` (under a `route_prefix` a store-scoped op is served scoped
/// only), and, unscoped, the HTTP-only client's. Each such call sends the
/// `filter[…]` members its handler's query spec declares (its bare
/// members, and the struct whose fields the rest are), `sort` when the
/// handler reads an order from it, the `page` family when the spec reads
/// it, and its `opArg[…]` members, no more and no
/// fewer, and `meta.args` keys the handler checks, each one it requires
/// among them. Returns the transport's calls by method name, with the
/// prefix argument given and not.
fn assert_calls_are_served(
    server: &str,
    clients: &JsonApiClients,
    scoped: bool,
    checked: &dyn Fn(&str) -> bool,
) -> BTreeMap<String, (Option<Call>, Option<Call>)> {
    let routes = server_routes(server);
    let served = |(call, sent): &(Call, ArgNames), who: &str| {
        if !checked(who) {
            return;
        }
        let (handler, rel) = route_for(&routes, call)
            .unwrap_or_else(|| panic!("{who} calls {call:?}, which the server does not serve:\n{routes:#?}"));
        if let Some(rel) = rel {
            assert!(server.contains(&format!("\"{rel}\"")), "{who} calls {call:?}; the server names no `{rel}`");
        }
        let (declared, required) = server_args(server, handler);
        assert_eq!(
            (&sent.filter, &sent.filter_struct),
            (&declared.filter, &declared.filter_struct),
            "{who} sends these filter members, {handler} reads those"
        );
        assert_eq!(sent.sort, declared.sort, "{who} and {handler} disagree on `sort`");
        assert_eq!(sent.page, declared.page, "{who} and {handler} disagree on the page family");
        assert_eq!(sent.op_args, declared.op_args, "{who} sends these opArg members, {handler} reads those");
        assert!(
            sent.meta_args.is_subset(&declared.meta_args),
            "{who} sends meta.args {:?}, {handler} takes {:?}",
            sent.meta_args,
            declared.meta_args
        );
        assert!(
            required.is_subset(&sent.meta_args),
            "{who} sends meta.args {:?}, {handler} requires {required:?}",
            sent.meta_args
        );
    };
    let mut calls = BTreeMap::new();
    for (name, body) in object_methods(&clients.transport, "export function createHttpTransport", "    ") {
        let (given, absent) = (client_call_and_args(body, true), client_call_and_args(body, false));
        if let Some(call) = if scoped { &given } else { &absent } {
            served(call, name);
        }
        calls.insert(name.to_string(), (given.map(|(call, _)| call), absent.map(|(call, _)| call)));
    }
    for (name, body) in object_methods(&clients.http, "export const httpCommands", "  ") {
        let call = client_call_and_args(body, false);
        // The HTTP-only client has no prefix argument: it calls what the
        // transport calls without one.
        assert_eq!(call.as_ref().map(|(call, _)| call), calls[name].1.as_ref(), "{name}");
        if !scoped && let Some(call) = &call {
            served(call, name);
        }
    }
    calls
}

/// Every junction call reaches a server route, paginated and not, scoped
/// and not, at the same shape either way: outside a resource module the
/// nested custom-op routes (§10.4), inside one the relationship routes, the
/// related link for a list of entities and the linkage for a list of ids
/// and for every write (§14.2). A lone `list_X` is a custom GET.
#[test]
fn every_junction_call_reaches_a_server_route() {
    let junction = |name: &str| ["Tags", "Tag", "Labels", "Label", "Drafts"].iter().any(|end| name.ends_with(end));
    let expected = [
        ("boardListTags", "GET", "/boards/{}/tags"),
        ("boardAddTag", "POST", "/boards/{}/tags"),
        ("boardRemoveTag", "DELETE", "/boards/{}/tags/{}"),
        ("taskListLabels", "GET", "/tasks/{}/labels"),
        ("taskAddLabel", "POST", "/tasks/{}/relationships/labels"),
        ("taskRemoveLabel", "DELETE", "/tasks/{}/relationships/labels"),
        ("taskListDrafts", "GET", "/tasks/list-drafts/{}"),
        ("workoutSetListTags", "GET", "/workout-sets/{}/relationships/tags"),
        ("workoutSetAddTag", "POST", "/workout-sets/{}/relationships/tags"),
        ("workoutSetRemoveTag", "DELETE", "/workout-sets/{}/relationships/tags"),
    ];
    for paginated in [false, true] {
        let (server, clients) = jsonapi_stack(paginated, &[], scope_under_projects);
        let calls = assert_calls_are_served(&server, &clients, true, &junction);
        for (name, method, path) in expected {
            let scoped = Some((method.to_string(), format!("/api/projects/{{}}{path}")));
            let unscoped = Some((method.to_string(), format!("/api{path}")));
            assert_eq!(calls[name], (scoped, unscoped), "{name}");
        }
        let (server, clients) = jsonapi_stack(paginated, &[], |_| {});
        assert_calls_are_served(&server, &clients, false, &junction);
    }
}

/// `status`: a custom GET taking the state rather than a store, and a
/// stateless one. Neither is store-scoped, so a `route_prefix` leaves both
/// unscoped.
const STATUS_MODULE: &str = "\
use crate::AppState;

pub async fn get_health(state: &AppState) -> Result<String, anyhow::Error> { todo!() }
#[ontogen::stateless]
pub fn get_version() -> Result<String, anyhow::Error> { todo!() }
";

/// Both clients, scoped and not, against the server generated beside them:
/// resource CRUD, CRUD with no entity, junction ops, custom GETs and POSTs,
/// ops that are not store-scoped, and filtered lists served as a resource
/// and as ops ([`filtered_list_modules`]).
#[test]
fn every_call_of_every_op_kind_reaches_a_server_route() {
    let configs = [(false, (|_| {}) as fn(&mut crate::ClientsConfig)), (true, scope_under_projects)];
    for (paginated, (scoped, adjust)) in [false, true].into_iter().flat_map(|p| configs.map(|c| (p, c))) {
        let filtered = filtered_list_modules(paginated);
        let mut extra = vec![("status.rs", STATUS_MODULE)];
        extra.extend(filtered.iter().map(|(file, source)| (*file, source.as_str())));
        let (server, clients) = jsonapi_stack(paginated, &extra, adjust);
        let calls = assert_calls_are_served(&server, &clients, scoped, &|_| true);
        for (name, (given, absent)) in &calls {
            assert_eq!(given.is_some() && absent.is_some(), !name.starts_with("subscribe"), "{name}");
        }
        // The server's argument names are read, not missed.
        let suffix = if scoped { "_scoped" } else { "" };
        let set = |names: &[&str]| names.iter().map(ToString::to_string).collect::<BTreeSet<_>>();
        let (summary, _) = server_args(&server, &format!("board_get_summary{suffix}"));
        assert_eq!(summary.op_args, set(&["include_done"]));
        let (archive, required) = server_args(&server, &format!("board_archive{suffix}"));
        assert_eq!((archive.meta_args, required), (set(&["reason", "task_id"]), set(&["task_id"])));
        let page = if paginated { set(&["limit", "offset"]) } else { set(&[]) };
        let (list, _) = server_args(&server, &format!("board_list{suffix}"));
        assert_eq!(list.op_args, page);
        let (tag, _) = server_args(&server, &format!("tag_list{suffix}"));
        assert_eq!(
            (tag.filter, tag.filter_struct, tag.page, tag.op_args),
            (set(&["title_prefix"]), Some("ListTagsQuery".to_string()), paginated, set(&[]))
        );
        let (digest, _) = server_args(&server, &format!("digest_list{suffix}"));
        assert_eq!(
            (digest.filter, digest.filter_struct, digest.page, digest.op_args),
            (set(&[]), Some("DigestQuery".to_string()), false, page.clone())
        );
        let (inbox, _) = server_args(&server, &format!("inbox_list{suffix}"));
        assert_eq!((inbox.filter, inbox.filter_struct, inbox.op_args), (set(&["owner_id"]), None, page.clone()));
        let (queue, _) = server_args(&server, &format!("queue_list{suffix}"));
        assert_eq!((queue.filter, queue.filter_struct, queue.op_args), (set(&["mode"]), None, page));
        if scoped {
            assert_eq!(
                ts_method(&clients.transport, "statusGetVersion"),
                "async statusGetVersion(_projectId?: string): Promise<string> {\n      return callOp<string>('GET', \
                 '/statuses/version');\n    },\n"
            );
        }
    }
}

/// `(command, argument keys)` of every `invoke` the IPC transport in `ts`
/// makes, by method name.
fn ipc_invokes(ts: &str) -> BTreeMap<String, (String, BTreeSet<String>)> {
    object_methods(ts, "export function createIpcTransport", "    ")
        .into_iter()
        .filter_map(|(name, body)| {
            let (_, call) = body.split_once("invoke('")?;
            let (command, rest) = call.split_once('\'').unwrap();
            let args = &rest[..rest.find(");").unwrap()];
            let keys = match args.strip_prefix(", {") {
                Some(members) => literal_keys(members.strip_suffix('}').unwrap()),
                None => BTreeSet::new(),
            };
            Some((name.to_string(), (command.to_string(), keys)))
        })
        .collect()
}

/// The arguments the generated Tauri command `command` in `ipc` takes from
/// its caller, as the caller names them: camelCased, Tauri's default for a
/// command's arguments. Its state and event channel come from Tauri.
fn ipc_command_args(ipc: &str, command: &str) -> BTreeSet<String> {
    let start = ipc
        .find(&format!("pub async fn {command}("))
        .or_else(|| ipc.find(&format!("pub fn {command}(")))
        .unwrap_or_else(|| panic!("no command {command} in:\n{ipc}"));
    let params = &ipc[start..];
    let params = &params[params.find('(').unwrap() + 1..params.find(") ->").unwrap()];
    // A parameter's type may hold commas (`State<'_, Arc<AppState>>`), so
    // each name is the one that follows a comma outside any `<…>`.
    let mut depth = 0usize;
    let mut names = vec![String::new()];
    for c in params.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => names.push(String::new()),
            _ => {}
        }
        names.last_mut().unwrap().push(c);
    }
    names
        .iter()
        .filter_map(|param| param.trim_start_matches(',').split_once(':').map(|(name, _)| name.trim()))
        .filter(|name| !name.is_empty() && !["ontogen_state", "channel"].contains(name))
        .map(crate::servers::types::snake_to_camel)
        .collect()
}

/// Every `invoke` of the IPC transport, paged and not, scoped and not, names
/// a generated Tauri command and sends exactly the arguments it takes: a
/// list's bare filters under their own names and its `*Query` struct as
/// `query`, whatever the filter's type is called.
#[test]
fn every_ipc_call_sends_its_commands_arguments() {
    let configs = [(false, (|_| {}) as fn(&mut crate::ClientsConfig)), (true, scope_under_projects)];
    for (paginated, (scoped, adjust)) in [false, true].into_iter().flat_map(|p| configs.map(|c| (p, c))) {
        let filtered = filtered_list_modules(paginated);
        let mut extra = vec![("status.rs", STATUS_MODULE)];
        extra.extend(filtered.iter().map(|(file, source)| (*file, source.as_str())));
        let (ipc, clients) = ipc_stack(paginated, &extra, adjust);
        let invokes = ipc_invokes(&clients.transport);
        for (name, (command, sent)) in &invokes {
            assert_eq!(sent, &ipc_command_args(&ipc, command), "{name} invokes {command}");
        }
        let page = if paginated { ", limit: limit ?? null, offset: offset ?? null" } else { "" };
        let prefix = if scoped { ", projectId: projectId ?? null" } else { "" };
        let ipc_methods: BTreeMap<_, _> =
            object_methods(&clients.transport, "export function createIpcTransport", "    ").into_iter().collect();
        for (name, command, args) in [
            ("queueList", "queue_list", format!("mode{page}")),
            ("inboxList", "inbox_list", format!("ownerId{page}")),
            ("digestList", "digest_list", format!("query: query ?? {{}}{page}")),
            ("tagList", "tag_list", format!("titlePrefix, query: query ?? {{}}{page}")),
        ] {
            let invoke = format!("invoke('{command}', {{ {args}{prefix} }});");
            assert!(ipc_methods[name].contains(&invoke), "{name} should {invoke}:\n{}", ipc_methods[name]);
        }
    }
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

/// `tag::list(store, {filter})`, with the page and a `count` taking the
/// same filter when `paginated`, in the resource fixture: the server's
/// handlers, and the `HttpTauriIpcSplit` and `HttpTs` output for the same
/// modules.
fn filtered_tag_list(filter: &str, paginated: bool) -> (String, JsonApiClients) {
    let tmp = tempfile::tempdir().unwrap();
    let mut server = crate::servers::tests::resource_fixture(tmp.path(), true);
    let page = if paginated { ", limit: Option<u64>, offset: Option<u64>" } else { "" };
    let mut tag = crate::servers::tests::app_error_crud_source("tag")
        .replace("store: &Store, limit: Option<u64>, offset: Option<u64>", &format!("store: &Store, {filter}{page}"))
        .replace("count(store: &Store)", &format!("count(store: &Store, {filter})"));
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
            ".route(\"/api/tags/{id}\", axum::routing::get(tag_get_by_id).patch(tag_update).delete(tag_delete)"
        )),
        "{http}"
    );
    assert!(http.contains("fn ontogen_tag_as_resource<'a>("), "{http}");
    for ts in [&clients.transport, &clients.http] {
        assert!(ts_method(ts, "tagGetById").contains("httpGet<JsonApiResourceDocument>(`/tags/"), "{ts}");
        assert!(ts_method(ts, "tagUpdate").contains("httpPatch<JsonApiResourceDocument>("), "{ts}");
        assert!(ts.contains("function flattenTag("), "{ts}");
    }
}

/// The filtered `tag_list` is served at the `tags` collection, reading its
/// filter from the `filter` family: no handler of `http` reads a flat query.
fn assert_tag_list_reads_the_filter_family(http: &str, clients: &JsonApiClients) -> ArgNames {
    assert_eq!(
        server_routes(http).get(&("GET".to_string(), "/api/tags".to_string())).map(String::as_str),
        Some("tag_list")
    );
    for flat in ["PaginationParams", "QueryRejection"] {
        assert!(!http.contains(flat), "no route reads a flat query, but `{flat}` is in:\n{http}");
    }
    assert_tag_crud_is_a_resource(http, clients);
    let (spec, _) = server_args(http, "tag_list");
    for ts in [&clients.transport, &clients.http] {
        let (call, sent) = client_call_and_args(ts_method(ts, "tagList"), false).unwrap();
        assert_eq!(call, ("GET".to_string(), "/api/tags".to_string()));
        assert_eq!(sent, spec, "the client sends what the handler reads");
    }
    spec
}

#[test]
fn a_filtered_resource_list_sends_its_filter_family_on_server_and_clients() {
    let (http, clients) = filtered_tag_list("title: &str", false);
    let spec = assert_tag_list_reads_the_filter_family(&http, &clients);
    assert_eq!((spec.filter, spec.filter_struct, spec.page), (BTreeSet::from(["title".to_string()]), None, false));

    let call = "const { data } = await httpGet<JsonApiCollectionDocument>(`/tags${toQueryString({ filter: { title } \
                })}`);";
    assert_eq!(
        ts_method(&clients.transport, "tagList"),
        format!(
            "async tagList(title: string): Promise<Tag[]> {{\n      {call}\n      return data.map(flattenTag);\n    \
             }},\n"
        )
    );
    assert_eq!(
        ts_method(&clients.http, "tagList"),
        format!(
            "async tagList(title: string): Promise<Tag[]> {{\n    {call}\n    return data.map(flattenTag);\n  }},\n"
        )
    );
}

#[test]
fn a_filtered_paginated_resource_list_sends_its_filter_and_page_families_on_server_and_clients() {
    let (http, clients) = filtered_tag_list("query: ListTagsQuery, title_prefix: &str", true);
    let spec = assert_tag_list_reads_the_filter_family(&http, &clients);
    assert_eq!(
        (spec.filter, spec.filter_struct, spec.page),
        (BTreeSet::from(["title_prefix".to_string()]), Some("ListTagsQuery".to_string()), true)
    );

    let signature = "async tagList(titlePrefix: string, query?: ListTagsQuery, limit?: number, offset?: number): \
                     Promise<PaginatedResult<Tag>> {";
    let call = "const { data, meta } = await httpGet<JsonApiPageDocument>(`/tags${toQueryString({ filter: { ...query, \
                title_prefix: titlePrefix }, page: { offset, limit } })}`);";
    let page = "return { items: data.map(flattenTag), total: meta.total, limit: meta.limit, offset: meta.offset };";
    assert_eq!(ts_method(&clients.transport, "tagList"), format!("{signature}\n      {call}\n      {page}\n    }},\n"));
    assert_eq!(ts_method(&clients.http, "tagList"), format!("{signature}\n    {call}\n    {page}\n  }},\n"));
    // The unfiltered `tasks` list beside it sends its sort before its page.
    assert!(ts_method(&clients.transport, "taskList").contains(
        "httpGet<JsonApiPageDocument>(`/tasks${toQueryString({ sort: options?.sort, page: { offset, limit } })}`);"
    ));
}

/// Both HTTP clients send a list's filter as the `filter` family (§14.2):
/// its `*Query` struct as `query`, its bare filters keyed by their Rust
/// names, or the struct spread under them. A list served as a resource
/// pages with the `page` family; one served as an op (§10.4) with `opArg`.
/// The IPC transport passes the same arguments flat.
#[test]
fn both_http_clients_send_a_list_filter_as_the_filter_family() {
    let tag_filter = "filter: { ...query, title_prefix: titlePrefix }";
    for paginated in [false, true] {
        let filtered = filtered_list_modules(paginated);
        let extra: Vec<(&str, &str)> = filtered.iter().map(|(file, source)| (*file, source.as_str())).collect();
        let (page, op_page, paged) = if paginated {
            (", page: { offset, limit }", ", opArg: { limit, offset }", ", limit?: number, offset?: number")
        } else {
            ("", "", "")
        };
        let (tag_ret, digest_ret, inbox_ret) = if paginated {
            ("PaginatedResult<Tag>", "PaginatedResult<Task>", "PaginatedResult<Tag>")
        } else {
            ("Tag[]", "Task[]", "Tag[]")
        };
        let queue_ret = inbox_ret;
        let document = if paginated {
            "{ data, meta } = await httpGet<JsonApiPageDocument>"
        } else {
            "{ data } = await httpGet<JsonApiCollectionDocument>"
        };
        let expected = [
            (
                "tagList",
                format!("titlePrefix: string, query?: ListTagsQuery{paged}): Promise<{tag_ret}>"),
                format!("const {document}(`/tags${{toQueryString({{ {tag_filter}{page} }})}}`);"),
                format!(
                    "const {document}(scopedPath(projectId, `/tags${{toQueryString({{ {tag_filter}{page} }})}}`));"
                ),
            ),
            (
                "digestList",
                format!("query?: DigestQuery{paged}): Promise<{digest_ret}>"),
                format!(
                    "return callOp<{digest_ret}>('GET', `/digests${{toQueryString({{ filter: query{op_page} }})}}`);"
                ),
                format!(
                    "return callOp<{digest_ret}>('GET', scopedPath(projectId, `/digests${{toQueryString({{ filter: \
                     query{op_page} }})}}`));"
                ),
            ),
            (
                "inboxList",
                format!("ownerId: string{paged}): Promise<{inbox_ret}>"),
                format!(
                    "return callOp<{inbox_ret}>('GET', `/inboxes${{toQueryString({{ filter: {{ owner_id: ownerId \
                     }}{op_page} }})}}`);"
                ),
                format!(
                    "return callOp<{inbox_ret}>('GET', scopedPath(projectId, `/inboxes${{toQueryString({{ filter: {{ \
                     owner_id: ownerId }}{op_page} }})}}`));"
                ),
            ),
            (
                "queueList",
                format!("mode: QueryMode{paged}): Promise<{queue_ret}>"),
                format!(
                    "return callOp<{queue_ret}>('GET', `/queues${{toQueryString({{ filter: {{ mode }}{op_page} }})}}`);"
                ),
                format!(
                    "return callOp<{queue_ret}>('GET', scopedPath(projectId, `/queues${{toQueryString({{ filter: {{ \
                     mode }}{op_page} }})}}`));"
                ),
            ),
        ];

        let unscoped = jsonapi_clients_with(paginated, &extra, |_| {});
        let scoped = jsonapi_clients_with(paginated, &extra, scope_under_projects);
        for (name, signature, call, scoped_call) in &expected {
            for ts in [&unscoped.transport, &unscoped.http] {
                let method = ts_method(ts, name);
                assert!(method.starts_with(&format!("async {name}({signature} {{")), "{method}");
                assert_eq!(method.lines().nth(1).unwrap().trim(), call, "{method}");
            }
            let method = ts_method(&scoped.transport, name);
            let signature = signature.replace("): ", ", projectId?: string): ");
            assert!(method.starts_with(&format!("async {name}({signature} {{")), "{method}");
            assert_eq!(method.lines().nth(1).unwrap().trim(), scoped_call, "{method}");
            // The HTTP-only client has no prefix argument.
            assert_eq!(http_call(&scoped.http, name), call);
        }
        for ts in [&unscoped.transport, &scoped.transport] {
            let ipc = &ts[ts.find("export function createIpcTransport").unwrap()..];
            assert!(!ipc.contains("filter:"), "IPC payloads stay flat:\n{ipc}");
        }
    }
}

/// A list's filter struct is only ever sent, and serde reads each of its
/// `Option` fields as `None` when absent, so the TS type lets a caller leave
/// any of them out (`digestList({ done: true })`).
#[test]
fn a_list_filter_structs_option_fields_are_optional_in_ts() {
    let filtered = filtered_list_modules(false);
    let extra: Vec<(&str, &str)> = filtered.iter().map(|(file, source)| (*file, source.as_str())).collect();
    let bindings = jsonapi_clients_with(false, &extra, |_| {}).bindings;
    for expected in [
        "export type DigestQuery = {\n  since?: string | null;\n  done?: boolean | null;\n};",
        "export type ListTagsQuery = {\n  title?: string | null;\n};",
    ] {
        assert!(bindings.contains(expected), "no `{expected}` in:\n{bindings}");
    }
}

/// A list whose filter struct has a required field (`owner: String`) takes
/// its `query` as a required parameter on both transports, the interface and
/// the HTTP-only client, and the IPC command always receives it. A list whose
/// filter is all `Option` keeps `query?` and the `?? {}` default.
#[test]
fn a_list_whose_filter_struct_has_a_required_field_requires_its_query() {
    for paginated in [false, true] {
        let filtered = filtered_list_modules(paginated);
        let mut extra: Vec<(&str, &str)> = filtered.iter().map(|(file, source)| (*file, source.as_str())).collect();
        let strict = filtered_op_list_module("StrictQuery, Task", "Task", "query: StrictQuery", paginated);
        extra.push(("strict.rs", strict.as_str()));
        let clients = jsonapi_clients_with(paginated, &extra, |_| {});
        let paged = if paginated { ", limit?: number, offset?: number" } else { "" };
        let required = format!("async strictList(query: StrictQuery{paged})");
        let interface = format!("  strictList(query: StrictQuery{paged}):");
        assert!(clients.transport.contains(&interface), "{}", clients.transport);
        for ts in [&clients.transport, &clients.http] {
            assert!(ts_method(ts, "strictList").starts_with(&required), "{}", ts_method(ts, "strictList"));
            // The all-`Option` filter is unchanged.
            assert!(ts_method(ts, "digestList").starts_with("async digestList(query?: DigestQuery"));
        }
        let ipc = &clients.transport[clients.transport.find("export function createIpcTransport").unwrap()..];
        let strict_ipc = ts_method(ipc, "strictList");
        assert!(strict_ipc.contains("invoke('strict_list', { query"), "{strict_ipc}");
        assert!(!strict_ipc.contains("query ??"), "{strict_ipc}");
        assert!(ts_method(ipc, "digestList").contains("query: query ?? {}"));
        assert!(
            clients.bindings.contains("export type StrictQuery = {\n  owner: string;\n  since?: string | null;\n};")
        );
    }
}

/// The `HttpTs` client imports the bindings file it was configured with, by
/// its path from the client.
#[test]
fn the_http_ts_client_imports_its_configured_bindings_file() {
    let clients = jsonapi_clients(false, |_| {});
    assert!(clients.http.contains("} from './jsonapi-bindings';"), "{}", clients.http);
    assert!(!clients.http.contains("from './bindings'"), "{}", clients.http);
}

const LABEL_MODULE: &str = "\
use crate::schema::Tag;
use crate::store::Store;

pub async fn list_tags(store: &Store, label_id: &str) -> Result<Vec<Tag>, anyhow::Error> { todo!() }
pub async fn add_tag(store: &Store, label_id: &str, tag_id: &str) -> Result<Tag, anyhow::Error> { todo!() }
pub async fn remove_tag(store: &Store, label_id: &str, tag_id: &str) -> Result<Tag, anyhow::Error> { todo!() }
";

/// A junction add or remove resolves to `null` on every transport, whatever
/// its fn returns: the interface declares `Promise<null>` and the HTTP route
/// answers `204`, so the IPC method must not promise the fn's value.
#[test]
fn a_junction_add_or_remove_resolves_null_on_every_transport() {
    let (_, clients) = jsonapi_stack(false, &[("label.rs", LABEL_MODULE)], |_| {});
    let ts = &clients.transport;
    let ipc = &ts[ts.find("export function createIpcTransport").unwrap()..];
    for name in ["labelAddTag", "labelRemoveTag"] {
        assert!(ts.contains(&format!("  {name}(labelId: string, tagId: string): Promise<null>;")), "{ts}");
        let method = ts_method(ipc, name);
        assert!(method.contains("): Promise<null> {"), "{method}");
        assert!(method.contains("return null;"), "{method}");
    }
}

/// `Ticket`, sortable by an enum (by its stored strings), an integer
/// primitive, an optional float and a bool; `Tag`, by an optional integer;
/// `Note`, whose list does not sort. The schema is also the type pool's
/// source of `Severity` and `ListTicketsQuery`.
const SORT_SCHEMA: &str = r#"
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Low,
    High,
    OnFire,
}

#[derive(serde::Deserialize)]
pub struct ListTicketsQuery {
    pub done: Option<bool>,
}

#[derive(OntologyEntity)]
#[ontology(entity)]
pub struct Ticket {
    #[ontology(id)]
    pub key: String,
    pub title: String,
    pub severity: Severity,
    pub rank: u16,
    pub score: Option<f64>,
    pub done: bool,
    #[ontology(relation(belongs_to, target = "Ticket"))]
    pub parent_id: Option<String>,
    pub labels: Vec<String>,
    #[ontology(body)]
    pub body: String,
}

#[derive(OntologyEntity)]
#[ontology(entity)]
pub struct Tag {
    #[ontology(id)]
    pub slug: String,
    pub uses: Option<i64>,
}

#[derive(OntologyEntity)]
#[ontology(entity)]
pub struct Note {
    #[ontology(id)]
    pub id: String,
    pub text: String,
}
"#;

/// `{entity}`'s get, create, update and delete over `crate::schema`.
fn sort_crud_rest(entity: &str) -> String {
    format!(
        "pub async fn get_by_id(store: &Store, id: &str) -> Result<{entity}, anyhow::Error> {{ todo!() }}
pub async fn create(store: &Store, input: Create{entity}Input) -> Result<{entity}, anyhow::Error> {{ todo!() }}
pub async fn update(store: &Store, id: &str, input: Update{entity}Input) -> Result<{entity}, anyhow::Error> {{ todo!() }}
pub async fn delete(store: &Store, id: &str) -> Result<(), anyhow::Error> {{ todo!() }}
"
    )
}

/// The API over [`SORT_SCHEMA`], paged when `paginated` and each `list`
/// then beside a `count` taking its filter: `ticket`'s list takes a `*Query` struct, a
/// bare filter and an order, beside a junction list that does not sort;
/// `tag`'s takes only an order, its type path-qualified; `note`'s takes
/// none; `digest`, a module with no entity, lists tickets unsorted.
fn sort_modules(paginated: bool) -> Vec<(&'static str, String)> {
    let page = if paginated { ", limit: Option<u64>, offset: Option<u64>" } else { "" };
    let count = |filter: &str| {
        if paginated {
            format!("pub async fn count(store: &Store{filter}) -> Result<u64, anyhow::Error> {{ todo!() }}\n")
        } else {
            String::new()
        }
    };
    vec![
        (
            "ticket.rs",
            format!(
                "use ontogen_core::order::OrderBy;
use crate::schema::{{CreateTicketInput, ListTicketsQuery, Tag, Ticket, UpdateTicketInput}};
use crate::store::Store;
use crate::store::ticket::TicketSortField;

pub async fn list(store: &Store, query: ListTicketsQuery, owner: &str, order: &[OrderBy<TicketSortField>]{page}) -> Result<Vec<Ticket>, anyhow::Error> {{ todo!() }}
{}{}pub async fn list_tags(store: &Store, ticket_id: &str) -> Result<Vec<Tag>, anyhow::Error> {{ todo!() }}
pub async fn add_tag(store: &Store, ticket_id: &str, tag_id: &str) -> Result<(), anyhow::Error> {{ todo!() }}
pub async fn remove_tag(store: &Store, ticket_id: &str, tag_id: &str) -> Result<(), anyhow::Error> {{ todo!() }}
",
                count(", query: ListTicketsQuery, owner: &str"),
                sort_crud_rest("Ticket")
            ),
        ),
        (
            "tag.rs",
            format!(
                "use crate::schema::{{CreateTagInput, Tag, UpdateTagInput}};
use crate::store::Store;

pub async fn list(store: &Store, sorted_by: &[ontogen_core::order::OrderBy<crate::store::tag::TagSortField>]{page}) -> Result<Vec<Tag>, anyhow::Error> {{ todo!() }}
{}{}",
                count(""),
                sort_crud_rest("Tag")
            ),
        ),
        (
            "note.rs",
            format!(
                "use crate::schema::{{CreateNoteInput, Note, UpdateNoteInput}};
use crate::store::Store;

pub async fn list(store: &Store{page}) -> Result<Vec<Note>, anyhow::Error> {{ todo!() }}
{}{}",
                count(""),
                sort_crud_rest("Note")
            ),
        ),
        (
            "digest.rs",
            format!(
                "use crate::schema::Ticket;
use crate::store::Store;

pub async fn list(store: &Store, owner_id: &str{page}) -> Result<Vec<Ticket>, anyhow::Error> {{ todo!() }}
{}",
                count(", owner_id: &str")
            ),
        ),
    ]
}

/// The `HttpTauriIpcSplit` and `HttpTs` output for [`SORT_SCHEMA`] (its
/// entities and its enums) and `modules`, through the public `gen_clients`.
/// `adjust` edits the clients config before generation. A client `adjust`
/// removes reads as empty.
fn try_sorted_clients(
    modules: &[(&str, String)],
    paginated: bool,
    adjust: impl FnOnce(&mut crate::ClientsConfig),
) -> Result<JsonApiClients, String> {
    try_sorted_stack(modules, paginated, adjust, false).1
}

/// [`try_sorted_clients`] beside the Axum server alone, generated by
/// `gen_servers` from the same API and config with no IPC or MCP
/// generator, as an app that serves its `HttpTauriIpcSplit` client over
/// HTTP only has it (iron-log-md). With `http_server` false no server is
/// generated.
fn try_sorted_stack(
    modules: &[(&str, String)],
    paginated: bool,
    adjust: impl FnOnce(&mut crate::ClientsConfig),
    http_server: bool,
) -> (Option<Result<String, String>>, Result<JsonApiClients, String>) {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    fs::create_dir_all(&api_dir).unwrap();
    for (file, source) in modules {
        fs::write(api_dir.join(file), source).unwrap();
    }
    let ts = tmp.path().join("ts");
    fs::create_dir_all(&ts).unwrap();
    let bindings_path = ts.join("bindings.ts");
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
        ..crate::ClientsConfig::new(api_dir.clone(), "AppState", "crate::api", "crate::schema", "crate::AppState")
    };
    let types = tmp.path().join("types").join("src");
    fs::create_dir_all(&types).unwrap();
    fs::write(types.join("lib.rs"), SORT_SCHEMA).unwrap();
    config.pool_extra_roots.push(types);
    adjust(&mut config);
    let path = std::path::Path::new("schema.rs");
    let schema = crate::ir::SchemaOutput {
        entities: crate::schema::parse::parse_schema_source(SORT_SCHEMA, path).unwrap(),
        enums: crate::schema::parse::parse_schema_enums_source(SORT_SCHEMA, path).unwrap(),
    };
    let read = |path: &std::path::Path| fs::read_to_string(path).unwrap_or_default();
    let clients = crate::gen_clients(&schema, None, &[], &config).map_err(|e| e.to_string()).map(|()| JsonApiClients {
        transport: read(&ts.join("transport.ts")),
        http: read(&ts.join("http.ts")),
        bindings: read(&bindings_path),
    });
    if !http_server {
        return (None, clients);
    }
    let server_out = tmp.path().join("http.rs");
    let servers = crate::ServersConfig {
        api_dir,
        state_type: config.state_type.clone(),
        service_import_path: config.service_import_path.clone(),
        types_import_path: config.types_import_path.clone(),
        state_import: config.state_import.clone(),
        naming: config.naming.clone(),
        generators: vec![crate::servers::ServerGenerator::HttpAxum { output: server_out.clone() }],
        sse_route_overrides: config.sse_route_overrides.clone(),
        route_prefix: config.route_prefix.clone(),
        store_type: config.store_type.clone(),
        store_import: config.store_import.clone(),
        pagination: config.pagination.clone(),
        extra_surfaces: Vec::new(),
        error_source_dir: None,
    };
    let server = crate::gen_servers(&schema, None, &[], &servers).map_err(|e| e.to_string()).map(|_| read(&server_out));
    (Some(server), clients)
}

/// [`try_sorted_clients`] over [`sort_modules`].
fn sorted_clients(paginated: bool, adjust: impl FnOnce(&mut crate::ClientsConfig)) -> JsonApiClients {
    try_sorted_clients(&sort_modules(paginated), paginated, adjust).unwrap()
}

/// The `Transport` interface's declaration of `name`.
fn interface_method<'a>(ts: &'a str, name: &str) -> &'a str {
    let interface = &ts[ts.find("export interface Transport {").unwrap()..];
    let start = interface.find(&format!("\n  {name}(")).unwrap_or_else(|| panic!("no `{name}` in:\n{ts}")) + 3;
    &interface[start..start + interface[start..].find('\n').unwrap()]
}

/// The signature of every client's method `name`: the interface's, the
/// HTTP and IPC transports' and the HTTP-only client's, each as
/// `name(params): Promise<R>`.
fn client_signatures(clients: &JsonApiClients, name: &str) -> [String; 4] {
    let ts = &clients.transport;
    let ipc = &ts[ts.find("export function createIpcTransport").unwrap()..];
    let head = |method: &str| method[method.find(name).unwrap()..method.find(" {").unwrap()].to_string();
    [
        interface_method(ts, name).trim_end_matches(';').to_string(),
        head(ts_method(ts, name)),
        head(ts_method(ipc, name)),
        head(ts_method(&clients.http, name)),
    ]
}

/// Each list whose fn takes an order gains a trailing `options?:
/// ListOptions<…SortKey>` on the interface and every client, after every
/// other parameter, the route-prefix one included; any other list, a
/// junction list among them, gains nothing.
#[test]
fn a_sorted_list_takes_trailing_list_options_on_every_client() {
    for (paginated, scoped) in [(false, false), (false, true), (true, false), (true, true)] {
        let clients = sorted_clients(paginated, |config| {
            if scoped {
                scope_under_projects(config);
            }
        });
        let page: &[&str] = if paginated { &["limit?: number", "offset?: number"] } else { &[] };
        let prefix: &[&str] = if scoped { &["projectId?: string"] } else { &[] };
        let result =
            |entity: &str| if paginated { format!("PaginatedResult<{entity}>") } else { format!("{entity}[]") };
        let signature = |name: &str, params: &[&[&str]], ret: String| {
            format!("{name}({}): Promise<{ret}>", params.concat().join(", "))
        };
        for (name, filter, entity) in
            [("ticketList", &["owner: string", "query?: ListTicketsQuery"][..], "Ticket"), ("tagList", &[][..], "Tag")]
        {
            let options = format!("options?: ListOptions<{entity}SortKey>");
            let options: &[&str] = &[&options];
            let [declared, http, ipc, http_client] = client_signatures(&clients, name);
            let expected = signature(name, &[filter, page, prefix, options], result(entity));
            assert_eq!(declared, expected, "{name}");
            assert_eq!(http, expected, "{name}");
            assert_eq!(ipc, expected, "{name}");
            // The HTTP-only client takes no route-prefix parameter.
            assert_eq!(http_client, signature(name, &[filter, page, options], result(entity)), "{name}");
        }
        for (name, expected) in [
            ("noteList", signature("noteList", &[page, prefix], result("Note"))),
            ("digestList", signature("digestList", &[&["ownerId: string"], page, prefix], result("Ticket"))),
            ("ticketListTags", signature("ticketListTags", &[&["ticketId: string"], page, prefix], result("Tag"))),
        ] {
            let [declared, http, ipc, _] = client_signatures(&clients, name);
            assert_eq!([&declared, &http, &ipc], [&expected; 3], "{name}");
        }
    }
}

/// One `{Entity}SortKey` union per entity a list sorts, enumerating its sort
/// keys ascending then descending: the id as `id` whatever the field is
/// called, an enum, an integer primitive, a float and a bool, but no
/// relationship, to-many or body field. `ListOptions` is declared once per
/// file, and the order's Rust types are never named in TS.
#[test]
fn each_sorted_entity_gets_a_sort_key_union_and_list_options_once() {
    let clients = sorted_clients(true, |_| {});
    for ts in [&clients.transport, &clients.http] {
        assert!(
            ts.contains(
                "export type TicketSortKey = 'id' | '-id' | 'title' | '-title' | 'severity' | '-severity' | 'rank' | \
                 '-rank' | 'score' | '-score' | 'done' | '-done';\n"
            ),
            "{ts}"
        );
        assert!(ts.contains("export type TagSortKey = 'id' | '-id' | 'uses' | '-uses';\n"), "{ts}");
        assert!(!ts.contains("NoteSortKey"), "{ts}");
        assert_eq!(
            ts.matches("export interface ListOptions<K extends string> {\n  sort?: K[];\n}\n").count(),
            1,
            "{ts}"
        );
        for rust in ["OrderBy", "SortField", "TODO: Type"] {
            assert!(!ts.contains(rust), "{rust} in:\n{ts}");
        }
    }
    assert!(!clients.bindings.contains("SortField"), "{}", clients.bindings);

    // With no list that sorts, neither is declared.
    let unsorted: Vec<_> =
        sort_modules(false).into_iter().filter(|(file, _)| !["ticket.rs", "tag.rs"].contains(file)).collect();
    let clients = try_sorted_clients(&unsorted, false, |_| {}).unwrap();
    for ts in [&clients.transport, &clients.http] {
        assert!(!ts.contains("ListOptions") && !ts.contains("SortKey"), "{ts}");
    }
}

/// Over HTTP a sorted list sends `options.sort` as `sort`, between its
/// `filter` and `page` families as the server's links order them, and
/// `toQueryString` writes an array as one parameter, its items joined by
/// `,`, or nothing when it is empty or absent.
#[test]
fn a_sorted_list_sends_sort_between_its_filter_and_its_page() {
    for paginated in [false, true] {
        let clients = sorted_clients(paginated, |_| {});
        let page = if paginated { ", page: { offset, limit }" } else { "" };
        for ts in [&clients.transport, &clients.http] {
            assert!(
                ts_method(ts, "ticketList").contains(&format!(
                    "`/tickets${{toQueryString({{ filter: {{ ...query, owner }}, sort: options?.sort{page} }})}}`"
                )),
                "{ts}"
            );
            assert!(
                ts_method(ts, "tagList")
                    .contains(&format!("`/tags${{toQueryString({{ sort: options?.sort{page} }})}}`")),
                "{ts}"
            );
            for name in ["noteList", "ticketListTags", "digestList"] {
                assert!(!ts_method(ts, name).contains("sort"), "{ts}");
            }
            let to_query_string = ts_function(ts, "toQueryString");
            assert!(
                to_query_string.contains(
                    "    if (value == null) return;\n    const values = Array.isArray(value) ? value : [value];\n    if \
                     (values.length > 0) parts.push(`${key}=${values.map((v) => \
                     encodeURIComponent(String(v))).join(',')}`);\n"
                ),
                "{to_query_string}"
            );
        }
    }
}

/// Over IPC a sorted list passes `options.sort` as the command's `sort`
/// argument, left out of the payload when absent; any other list sends no
/// `sort`.
#[test]
fn a_sorted_list_sends_sort_to_its_ipc_command() {
    let clients = sorted_clients(true, scope_under_projects);
    let ipc: BTreeMap<_, _> =
        object_methods(&clients.transport, "export function createIpcTransport", "    ").into_iter().collect();
    let page = "limit: limit ?? null, offset: offset ?? null, projectId: projectId ?? null";
    assert!(
        ipc["ticketList"].contains(&format!(
            "invoke('ticket_list', {{ owner, query: query ?? {{}}, sort: options?.sort, {page} }});"
        )),
        "{}",
        ipc["ticketList"]
    );
    assert!(
        ipc["tagList"].contains(&format!("invoke('tag_list', {{ sort: options?.sort, {page} }});")),
        "{}",
        ipc["tagList"]
    );
    for name in ["noteList", "ticketListTags", "digestList"] {
        assert!(!ipc[name].contains("sort"), "{}", ipc[name]);
    }
    let invokes = ipc_invokes(&clients.transport);
    assert!(invokes["ticketList"].1.contains("sort"), "{invokes:?}");
}

/// The clients stage refuses an order as the servers stage does: one on a
/// list with no resource behind it, one of another entity than its
/// module's resource, one on a fn that is not a `list`, one before the
/// filter, and a second one.
#[test]
fn an_order_no_client_can_send_is_an_error() {
    let order = |entity: &str| format!("order: &[ontogen_core::order::OrderBy<{entity}SortField>]");
    let modules = sort_modules(false);
    let replace = |file: &str, from: &str, to: &str| {
        let mut modules = modules.clone();
        let (_, source) = modules.iter_mut().find(|(f, _)| *f == file).unwrap();
        assert!(source.contains(from), "{source}");
        *source = source.replace(from, to);
        modules
    };
    for (bad, name) in [
        (replace("digest.rs", "owner_id: &str", &format!("owner_id: &str, {}", order("Ticket"))), "digest::list"),
        (replace("ticket.rs", "order: &[OrderBy<TicketSortField>]", &order("Tag")), "ticket::list"),
        (
            replace("note.rs", "id: &str) -> Result<Note", &format!("id: &str, {}) -> Result<Note", order("Note"))),
            "note::get_by_id",
        ),
        (
            replace(
                "ticket.rs",
                "query: ListTicketsQuery, owner: &str, order: &[OrderBy<TicketSortField>]",
                "order: &[OrderBy<TicketSortField>], query: ListTicketsQuery, owner: &str",
            ),
            "ticket::list",
        ),
        (
            replace(
                "ticket.rs",
                "order: &[OrderBy<TicketSortField>]",
                "again: &[OrderBy<TicketSortField>], order: &[OrderBy<TicketSortField>]",
            ),
            "ticket::list",
        ),
    ] {
        let err = try_sorted_clients(&bad, false, |_| {}).err().unwrap_or_else(|| panic!("{name} should fail"));
        let (module, f) = name.split_once("::").unwrap();
        assert!(err.contains(module) && err.contains(f), "{name}: {err}");
    }
}

/// [`sort_modules`], unpaged, with `file`'s `list` taking `filters` ahead
/// of its other parameters.
fn list_filtered(file: &str, filters: &str) -> Vec<(&'static str, String)> {
    let mut modules = sort_modules(false);
    let (_, source) = modules.iter_mut().find(|(f, _)| *f == file).unwrap();
    let head = "pub async fn list(store: &Store";
    assert!(source.contains(head), "{source}");
    *source = source.replacen(head, &format!("{head}, {filters}"), 1);
    modules
}

/// Keeps only the client generators `keep` matches.
fn clients_only(keep: fn(&ClientGenerator) -> bool) -> impl FnOnce(&mut crate::ClientsConfig) {
    move |config| config.generators.retain(keep)
}

fn is_transport(g: &ClientGenerator) -> bool {
    matches!(g, ClientGenerator::HttpTauriIpcSplit { .. })
}

fn is_http_ts(g: &ClientGenerator) -> bool {
    matches!(g, ClientGenerator::HttpTs { .. })
}

/// A `route_prefix` scoping every store-scoped op under `projects/{param}`.
fn scope_under(param: &'static str) -> impl FnOnce(&mut crate::ClientsConfig) {
    move |config| {
        config.route_prefix = Some(crate::servers::RoutePrefix {
            segments: format!("projects/:{param}"),
            state_accessor: "store_for".to_string(),
            params: vec![crate::servers::PrefixParam {
                name: param.to_string(),
                rust_type: "String".to_string(),
                ts_type: "string".to_string(),
            }],
        });
    }
}

/// Asserts `result` failed with an error containing every one of `parts`.
fn assert_refused<T>(result: Result<T, String>, parts: &[&str]) {
    let Err(err) = result else { panic!("should fail with {parts:?}") };
    for part in parts {
        assert!(err.contains(part), "missing `{part}` in: {err}");
    }
}

/// The IPC transport invokes a sorted list's command with its sort keys as
/// `sort`, so a bare filter named `sort` would be a second `sort` key in the
/// invoke payload. The clients stage refuses it whenever it emits that
/// transport: with no server generated, and beside an Axum server alone
/// (as iron-log-md has it), which accepts the filter, as `filter[sort]`
/// and `sort` do not collide over HTTP.
#[test]
fn a_sorted_list_filter_named_sort_is_refused_where_the_ipc_transport_is_emitted() {
    let modules = list_filtered("tag.rs", "sort: &str");
    let message = "ontogen: the IPC command `tag_list` cannot be generated: `tag::list` takes an argument named \
                   `sort`, which the command takes under the IPC wire key `sort`, the key it uses for the list's sort \
                   keys, so the two would collide. Rename the argument.";
    assert_refused(try_sorted_clients(&modules, false, |_| {}), &[message]);
    assert_refused(try_sorted_clients(&modules, false, clients_only(is_transport)), &[message]);
    let (server, clients) = try_sorted_stack(&modules, false, clients_only(is_transport), true);
    assert!(server.unwrap().is_ok(), "the Axum server takes `filter[sort]`");
    assert_refused(clients, &[message]);

    // The HTTP-only client sends it as `filter[sort]` beside `sort`.
    let (server, clients) = try_sorted_stack(&modules, false, clients_only(is_http_ts), true);
    assert!(server.unwrap().is_ok());
    let http = clients.unwrap().http;
    assert!(
        ts_method(&http, "tagList").contains("`/tags${toQueryString({ filter: { sort }, sort: options?.sort })}`"),
        "{http}"
    );

    // A command the transport skips is never invoked, so cannot collide.
    let skipped = try_sorted_clients(&modules, false, |config| config.ts_skip_commands.push("tag_list".into()));
    assert!(skipped.is_ok(), "{:?}", skipped.err());
}

/// The rest of the IPC wire-key rules hold on the clients stage as on the
/// servers': a bare filter named `query` beside a `*Query` struct, a
/// paginated junction list's argument named `limit`, an event argument
/// named `channel`, and an argument named like the route prefix parameter.
#[test]
fn the_ipc_transport_refuses_every_wire_key_its_commands_take() {
    let replace = |modules: Vec<(&'static str, String)>, file: &str, from: &str, to: &str| {
        let mut modules = modules;
        let (_, source) = modules.iter_mut().find(|(f, _)| *f == file).unwrap();
        assert!(source.contains(from), "{source}");
        *source = source.replace(from, to);
        modules
    };
    let query = replace(
        sort_modules(false),
        "ticket.rs",
        "query: ListTicketsQuery, owner: &str",
        "filter: ListTicketsQuery, query: &str",
    );
    assert_refused(
        try_sorted_clients(&query, false, clients_only(is_transport)),
        &["the IPC command `ticket_list`", "`ticket::list` takes an argument named `query`", "`*Query` filter struct"],
    );

    let limit = replace(
        sort_modules(true),
        "ticket.rs",
        "list_tags(store: &Store, ticket_id",
        "list_tags(store: &Store, limit",
    );
    assert_refused(
        try_sorted_clients(&limit, true, clients_only(is_transport)),
        &["the IPC command `ticket_list_tags`", "`ticket::list_tags` takes an argument named `limit`"],
    );

    let mut channel = sort_modules(false);
    channel.push((
        "feed.rs",
        "use crate::schema::Tag;
use crate::AppState;

pub async fn tag_changes(state: &AppState, channel: Option<String>) -> Result<tokio::sync::broadcast::Receiver<Tag>, anyhow::Error> { todo!() }
"
        .to_string(),
    ));
    assert_refused(
        try_sorted_clients(&channel, false, clients_only(is_transport)),
        &["the IPC command `tag_changes_subscribe`", "`feed::tag_changes` takes an argument named `channel`"],
    );

    let scoped = list_filtered("tag.rs", "project_id: &str");
    assert_refused(
        try_sorted_clients(&scoped, false, |config| {
            config.generators.retain(is_transport);
            scope_under("project_id")(config);
        }),
        &["the IPC command `tag_list`", "`tag::list` takes an argument named `project_id`", "route prefix parameter"],
    );
}

/// The IPC transport invokes each command with its arguments camelCased,
/// which is how Tauri reads them, so a bare filter `sort_` on a sorted list
/// is a second `sort` key in the invoke payload (`{ sort, sort: options?.sort
/// }`), which TypeScript rejects. iron-log-md's shape — a sorted list with a
/// filter `sort_`, its client beside an Axum server alone — fails the build
/// as a client codegen error, naming the argument and the key it travels
/// under.
#[test]
fn a_sorted_list_filter_named_sort_underscore_is_refused_where_the_ipc_transport_is_emitted() {
    let modules = list_filtered("tag.rs", "sort_: &str");
    let message = "client codegen error: ontogen: the IPC command `tag_list` cannot be generated: `tag::list` takes \
                   an argument named `sort_`, which the command takes under the IPC wire key `sort`, the key it uses \
                   for the list's sort keys, so the two would collide. Rename the argument.";
    let (server, clients) = try_sorted_stack(&modules, false, clients_only(is_transport), true);
    assert!(server.unwrap().is_ok(), "the Axum server takes `filter[sort_]`");
    assert_eq!(clients.err().as_deref(), Some(message));
    assert_refused(try_sorted_clients(&modules, false, |_| {}), &[message]);

    // The HTTP-only client sends it as `filter[sort_]` beside `sort`.
    let http = try_sorted_clients(&modules, false, clients_only(is_http_ts));
    assert!(http.is_ok(), "{:?}", http.err());
}

/// Every IPC wire-key rule compares the keys arguments travel under, the
/// camelCased names: `query_` beside a `*Query` struct, a paginated
/// junction list's `limit_`, an event's `channel_`, an argument
/// `project_id_` under the `project_id` route prefix, and a route prefix
/// parameter `sort_` on a sorted list are each refused on the clients
/// stage.
#[test]
fn the_ipc_transport_refuses_every_wire_key_its_commands_take_camelcased() {
    let replace = |modules: Vec<(&'static str, String)>, file: &str, from: &str, to: &str| {
        let mut modules = modules;
        let (_, source) = modules.iter_mut().find(|(f, _)| *f == file).unwrap();
        assert!(source.contains(from), "{source}");
        *source = source.replace(from, to);
        modules
    };
    let query = replace(
        sort_modules(false),
        "ticket.rs",
        "query: ListTicketsQuery, owner: &str",
        "filter: ListTicketsQuery, query_: &str",
    );
    assert_refused(
        try_sorted_clients(&query, false, clients_only(is_transport)),
        &["`ticket::list` takes an argument named `query_`", "IPC wire key `query`", "`*Query` filter struct"],
    );

    let limit = replace(
        sort_modules(true),
        "ticket.rs",
        "list_tags(store: &Store, ticket_id",
        "list_tags(store: &Store, limit_",
    );
    assert_refused(
        try_sorted_clients(&limit, true, clients_only(is_transport)),
        &["`ticket::list_tags` takes an argument named `limit_`", "IPC wire key `limit`"],
    );

    let mut channel = sort_modules(false);
    channel.push((
        "feed.rs",
        "use crate::schema::Tag;
use crate::AppState;

pub async fn tag_changes(state: &AppState, channel_: Option<String>) -> Result<tokio::sync::broadcast::Receiver<Tag>, anyhow::Error> { todo!() }
"
        .to_string(),
    ));
    assert_refused(
        try_sorted_clients(&channel, false, clients_only(is_transport)),
        &["`feed::tag_changes` takes an argument named `channel_`", "IPC wire key `channel`"],
    );

    let scoped = list_filtered("tag.rs", "project_id_: &str");
    assert_refused(
        try_sorted_clients(&scoped, false, |config| {
            config.generators.retain(is_transport);
            scope_under("project_id")(config);
        }),
        &["`tag::list` takes an argument named `project_id_`", "IPC wire key `projectId`", "route prefix parameter"],
    );

    assert_refused(
        try_sorted_clients(&sort_modules(false), false, |config| {
            config.generators.retain(is_transport);
            scope_under("sort_")(config);
        }),
        &[
            "is called with the route prefix parameter `sort_`",
            "IPC wire key `sort`",
            "the list's sort keys",
            "Rename the route prefix parameter.",
        ],
    );
}

/// Every TS client takes a sorted list's sort keys as `options`, so a bare
/// filter the method would name `options` — camelCased, as every bare
/// filter is — is refused on each one, with or without the IPC transport.
#[test]
fn a_sorted_list_filter_named_options_is_refused_on_every_ts_client() {
    for filter in ["options: &str", "options_: &str"] {
        let modules = list_filtered("tag.rs", filter);
        let arg = filter.split_once(':').unwrap().0;
        let message = format!(
            "ontogen: the TypeScript method `tagList` cannot be generated: `tag::list` takes an argument named \
             `{arg}`, which the method takes as `options`, the parameter name it uses for the list's sort keys, so \
             the two would collide. Rename the argument."
        );
        for keep in [is_transport as fn(&ClientGenerator) -> bool, is_http_ts] {
            assert_refused(try_sorted_clients(&modules, false, clients_only(keep)), &[&message]);
        }
        let (server, clients) = try_sorted_stack(&modules, false, clients_only(is_transport), true);
        assert!(server.unwrap().is_ok(), "{filter}");
        assert_refused(clients, &[&message]);
    }
}

/// The `Transport` methods take the route prefix parameter camelCased, so a
/// prefix parameter named `options` collides with a sorted list's sort
/// keys there; the HTTP-only client takes no prefix parameter and accepts
/// it.
#[test]
fn a_route_prefix_parameter_named_options_is_refused_on_the_transport_of_a_sorted_list() {
    let refused = try_sorted_clients(&sort_modules(false), false, scope_under("options"));
    assert_refused(
        refused,
        &["ontogen: the TypeScript method `tagList` cannot be generated: `tag::list` is called with the route \
             prefix parameter `options`, which the method takes as `options`, the parameter name it uses for the \
             list's sort keys, so the two would collide. Rename the route prefix parameter."],
    );
    let http_only = try_sorted_clients(&sort_modules(false), false, |config| {
        config.generators.retain(is_http_ts);
        scope_under("options")(config);
    });
    assert!(http_only.is_ok(), "{:?}", http_only.err());
}

/// The method of a list that takes a `*Query` struct takes it as `query`,
/// so no bare filter of it may be named `query` on the HTTP-only client
/// either.
#[test]
fn a_bare_filter_named_query_beside_a_query_struct_is_refused_on_the_http_client() {
    let mut modules = sort_modules(false);
    let (_, ticket) = modules.iter_mut().find(|(f, _)| *f == "ticket.rs").unwrap();
    *ticket = ticket.replace("query: ListTicketsQuery, owner: &str", "filter: ListTicketsQuery, query: &str");
    assert_refused(
        try_sorted_clients(&modules, false, clients_only(is_http_ts)),
        &["ontogen: the TypeScript method `ticketList` cannot be generated: `ticket::list` takes an argument named \
             `query`, which the method takes as `query`, the parameter name it uses for the list's `*Query` filter \
             struct, so the two would collide. Rename the argument."],
    );
}

/// A list that does not sort takes no `options` and sends no `sort`, so its
/// bare filters may be named either, on every client and server.
#[test]
fn an_unsorted_list_may_take_filters_named_sort_and_options() {
    let modules = list_filtered("note.rs", "sort: &str, options: &str");
    let (server, clients) = try_sorted_stack(&modules, false, |_| {}, true);
    assert!(server.unwrap().is_ok());
    let clients = clients.unwrap();
    let [declared, http, ipc, http_client] = client_signatures(&clients, "noteList");
    for signature in [declared, http, ipc, http_client] {
        assert_eq!(signature, "noteList(sort: string, options: string): Promise<Note[]>");
    }
    let ipc: BTreeMap<_, _> =
        object_methods(&clients.transport, "export function createIpcTransport", "    ").into_iter().collect();
    assert!(ipc["noteList"].contains("invoke('note_list', { sort, options });"), "{}", ipc["noteList"]);
    assert!(
        ts_method(&clients.http, "noteList").contains("`/notes${toQueryString({ filter: { sort, options } })}`"),
        "{}",
        clients.http
    );
}
