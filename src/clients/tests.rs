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
";

/// The filtered lists beside [`BOARD_MODULE`]: `tag`'s, served as its
/// resource ([`filtered_tag_module`]), and two in modules with no entity,
/// served as ops: `digest`'s takes only the `DigestQuery` struct, `inbox`'s
/// only a bare `owner_id`.
fn filtered_list_modules(paginated: bool) -> [(&'static str, String); 3] {
    [
        ("tag.rs", filtered_tag_module(paginated)),
        ("digest.rs", filtered_op_list_module("DigestQuery, Task", "Task", "query: DigestQuery", paginated)),
        ("inbox.rs", filtered_op_list_module("Tag", "Tag", "owner_id: &str", paginated)),
    ]
}

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
    let (server, clients) = generate_jsonapi(paginated, extra, adjust, true);
    (server.unwrap(), clients)
}

/// [`jsonapi_stack`]'s clients alone, with no server generated beside them.
fn jsonapi_clients_with(
    paginated: bool,
    extra: &[(&str, &str)],
    adjust: impl FnOnce(&mut crate::ClientsConfig),
) -> JsonApiClients {
    generate_jsonapi(paginated, extra, adjust, false).1
}

/// [`jsonapi_stack`], generating the server only with `server`.
fn generate_jsonapi(
    paginated: bool,
    extra: &[(&str, &str)],
    adjust: impl FnOnce(&mut crate::ClientsConfig),
    server: bool,
) -> (Option<String>, JsonApiClients) {
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
    crate::gen_clients(&entities, Some(&api), &[], &config).unwrap();

    let read = |path: &std::path::Path| fs::read_to_string(path).unwrap();
    let clients = JsonApiClients {
        transport: read(&ts.join("transport.ts")),
        http: read(&ts.join("http.ts")),
        bindings: read(&bindings_path),
    };
    if !server {
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
    crate::gen_servers(&entities, Some(&api), &[], &servers).unwrap();
    (Some(read(&server_out)), clients)
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

/// The statement a method of the `HttpTs` client makes its call with.
fn http_call<'a>(http: &'a str, name: &str) -> &'a str {
    ts_method(http, name).lines().nth(1).unwrap().trim()
}

#[test]
fn a_scoped_junction_op_calls_its_action_route_given_the_prefix_argument() {
    let clients = jsonapi_clients(true, scope_under_projects);
    let parent = "${encodeURIComponent(boardId)}";
    let page = "${toQueryString({ opArg: { limit, offset } })}";
    for (name, signature, scoped, unscoped) in [
        (
            "boardListTags",
            "boardId: string, limit?: number, offset?: number, projectId?: string): Promise<PaginatedResult<Tag>>",
            format!(
                "return callOp<PaginatedResult<Tag>>('GET', scopedPath(projectId, `/boards/list-tags/{parent}{page}`));"
            ),
            format!("return callOp<PaginatedResult<Tag>>('GET', `/boards/{parent}/tags{page}`);"),
        ),
        (
            "boardAddTag",
            "boardId: string, tagId: string, projectId?: string): Promise<null>",
            "return callOp<null>('POST', scopedPath(projectId, '/boards/add-tag'), { board_id: boardId, tag_id: tagId });"
                .to_string(),
            format!("return callOp<null>('POST', `/boards/{parent}/tags`, {{ tag_id: tagId }});"),
        ),
        (
            "boardRemoveTag",
            "boardId: string, tagId: string, projectId?: string): Promise<null>",
            "return callOp<null>('POST', scopedPath(projectId, '/boards/remove-tag'), { board_id: boardId, tag_id: tagId \
             });"
                .to_string(),
            format!("return callOp<null>('DELETE', `/boards/{parent}/tags/${{encodeURIComponent(tagId)}}`);"),
        ),
    ] {
        assert_eq!(
            ts_method(&clients.transport, name),
            format!(
                "async {name}({signature} {{\n      if (projectId) {{\n        {scoped}\n      }}\n      {unscoped}\n    \
                 }},\n"
            )
        );
        // The HTTP-only client calls unscoped routes alone, as the transport
        // does without the prefix argument.
        assert_eq!(http_call(&clients.http, name), unscoped, "{}", clients.http);
    }
    // An op whose scoped route shares its unscoped shape needs no branch.
    assert_eq!(
        ts_method(&clients.transport, "boardArchive").lines().nth(1).unwrap().trim(),
        "return callOp<Task>('POST', scopedPath(projectId, '/boards/archive'), { task_id: taskId, reason });"
    );
}

/// An HTTP call: its method and its path, each path parameter written `{}`.
type Call = (String, String);

/// Every `(METHOD, path)` the generated Axum router `server` registers, each
/// path parameter written `{}`, with the handler serving it.
fn server_routes(server: &str) -> BTreeMap<Call, String> {
    let flat = crate::servers::tests::compact(server);
    let mut routes = BTreeMap::new();
    for route in flat.split(".route(\"").skip(1) {
        let (path, rest) = route.split_once("\",").unwrap();
        let handlers = &rest[..rest.find(".fallback(").unwrap_or_else(|| panic!("no fallback on {path}"))];
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
/// `filter[…]` members (named by its type's last segment), whether it pages
/// with the `page` family, its `opArg[…]` members and its `meta.args` keys.
#[derive(Debug, Default, PartialEq)]
struct ArgNames {
    filter: BTreeSet<String>,
    filter_struct: Option<String>,
    page: bool,
    op_args: BTreeSet<String>,
    meta_args: BTreeSet<String>,
}

/// What the generated handler `handler` in `server` reads outside its path:
/// the `filter`, `filter_fields`, `page` and `op_args` of its `RouteQuery`
/// spec, and the `meta.args` keys it checks (`request::check_op_arg_names`),
/// with those it requires.
fn server_args(server: &str, handler: &str) -> (ArgNames, BTreeSet<String>) {
    let flat = crate::servers::tests::compact(server);
    let code =
        &server[server.find(&format!("async fn {handler}(")).unwrap_or_else(|| panic!("no {handler} in:\n{server}"))..];
    let code = crate::servers::tests::compact(&code[..code.find("\n}\n").unwrap()]);
    let (signature, body) = code.split_once(")->").unwrap();
    let strings = |list: &str| -> BTreeSet<String> {
        list.split(',').filter(|s| !s.is_empty()).map(|s| s.trim_matches('"').to_string()).collect()
    };
    let mut names = ArgNames::default();
    // The JSON:API `Query<Spec>` it extracts, not Axum's own.
    for (at, _) in signature.match_indices("Query<").filter(|(at, _)| !signature[..*at].ends_with("::")) {
        let spec = &signature[at + "Query<".len()..];
        let spec = &spec[..spec.find('>').unwrap()];
        let Some(spec_impl) = flat.split_once(&format!("implRouteQueryfor{spec}{{")).map(|(_, rest)| rest) else {
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

/// `path` with every `{name}` or `${…}` parameter written `{}` and its query
/// string dropped.
fn route_template(path: &str) -> String {
    let path = path.find("${toQueryString(").or_else(|| path.find('?')).map_or(path, |end| &path[..end]);
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let open = if rest[..start].ends_with('$') { start - 1 } else { start };
        out.push_str(&rest[..open]);
        out.push_str("{}");
        rest = &rest[start + rest[start..].find('}').unwrap() + 1..];
    }
    out.push_str(rest);
    out
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

/// The `(METHOD, path under /api)` a generated HTTP method `method` (its
/// name, signature and body) calls, with its route-prefix argument
/// `projectId` given or not, and the argument names the call sends outside
/// its path.
fn client_call_and_args(method: &str, prefix_given: bool) -> Option<(Call, ArgNames)> {
    let signature = &method[..method.find("): ").unwrap()];
    let body = match method.split_once("if (projectId) {") {
        Some((_, branches)) => {
            let (scoped, unscoped) = branches.split_once("\n      }\n").unwrap();
            if prefix_given { scoped } else { unscoped }
        }
        None => method,
    };
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
/// members, and the struct whose fields the rest are), the `page` family
/// when the spec reads it, and its `opArg[…]` members, no more and no
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
        let handler = routes
            .get(call)
            .unwrap_or_else(|| panic!("{who} calls {call:?}, which the server does not serve:\n{routes:#?}"));
        let (declared, required) = server_args(server, handler);
        assert_eq!(
            (&sent.filter, &sent.filter_struct),
            (&declared.filter, &declared.filter_struct),
            "{who} sends these filter members, {handler} reads those"
        );
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

#[test]
fn every_junction_call_reaches_a_server_route() {
    let junction = |name: &str| name.ends_with("Tags") || name.ends_with("Tag");
    let (server, clients) = jsonapi_stack(true, &[], scope_under_projects);
    let calls = assert_calls_are_served(&server, &clients, true, &junction);
    let call = |method: &str, path: &str| Some((method.to_string(), path.to_string()));
    assert_eq!(calls["boardListTags"].0, call("GET", "/api/projects/{}/boards/list-tags/{}"));
    assert_eq!(calls["boardAddTag"].0, call("POST", "/api/projects/{}/boards/add-tag"));
    assert_eq!(calls["boardRemoveTag"].0, call("POST", "/api/projects/{}/boards/remove-tag"));

    // Unscoped, the same methods call the nested routes.
    let (server, clients) = jsonapi_stack(true, &[], |_| {});
    let calls = assert_calls_are_served(&server, &clients, false, &junction);
    assert_eq!(calls["boardListTags"].1, call("GET", "/api/boards/{}/tags"));
    assert_eq!(calls["boardAddTag"].1, call("POST", "/api/boards/{}/tags"));
    assert_eq!(calls["boardRemoveTag"].1, call("DELETE", "/api/boards/{}/tags/{}"));
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
        assert_eq!((inbox.filter, inbox.filter_struct, inbox.op_args), (set(&["owner_id"]), None, page));
        if scoped {
            assert_eq!(
                ts_method(&clients.transport, "statusGetVersion"),
                "async statusGetVersion(_projectId?: string): Promise<string> {\n      return callOp<string>('GET', \
                 '/statuses/version');\n    },\n"
            );
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
    // The unfiltered `tasks` list beside it pages as it always has.
    assert!(
        ts_method(&clients.transport, "taskList")
            .contains("httpGet<JsonApiPageDocument>(`/tasks${toQueryString({ page: { offset, limit } })}`);")
    );
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
