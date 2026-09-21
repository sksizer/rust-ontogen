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
        schema_entities: Vec::new(),
        pagination: Some(PaginationConfig { default_limit: 50, max_limit: 200 }),
        pool_extra_roots: Vec::new(),
        pool_exclude_paths: Vec::new(),
        extra_surfaces: Vec::new(),
    }
}

#[test]
fn paginated_list_with_a_query_struct_param_emits_list_has_query() {
    let module = admin_test_crud_module("timer_session", true);
    let dir = tempfile::tempdir().expect("tempdir");
    let output = dir.path().join("admin-registry.ts");

    admin::generate(&output, std::slice::from_ref(&module), &admin_test_config(), &[]);

    let written = fs::read_to_string(&output).expect("read admin registry");
    assert!(written.contains("paginated: true"), "registry was:\n{written}");
    assert!(written.contains("listHasQuery: true"), "registry was:\n{written}");
}

#[test]
fn paginated_list_without_a_query_struct_param_omits_list_has_query() {
    let module = admin_test_crud_module("timer_session", false);
    let dir = tempfile::tempdir().expect("tempdir");
    let output = dir.path().join("admin-registry.ts");

    admin::generate(&output, std::slice::from_ref(&module), &admin_test_config(), &[]);

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
    crate::clients::generators::admin::generate(&admin_out, &modules, &config, &config.schema_entities);

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

    let entities = crate::clients::surface_entities(&[surface(None), surface(Some(dir.path().to_path_buf()))]).unwrap();
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
    crate::clients::generators::admin::generate(&admin_out, &modules, &config, &config.schema_entities);

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
    config.schema_entities = crate::parse_schema(&crate::SchemaConfig { schema_dir: primary_schema }).unwrap().entities;
    config.generators = vec![ClientGenerator::AdminRegistry { output: tmp.path().join("admin-registry.ts") }];

    let err = crate::clients::generate_clients(&config).expect_err("an entity name shared by two surfaces must fail");
    assert!(err.contains("entity `Workout`"), "{err}");
    assert!(err.contains("more than one API surface"), "{err}");
}
