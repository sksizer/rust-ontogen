//! Integration tests for the `ontogen::Pipeline` builder.
//!
//! These exercise the builder against the embedded schema fixtures under
//! `tests/fixtures/schema/`, generating into tempdirs.

use std::path::{Path, PathBuf};

use ontogen::{IdStrategy, MarkdownIoOptions, MarkdownLayout, OkfOptions, Pipeline, StoreBackendChoice};

/// Returns the path to the embedded schema fixture directory.
fn fixture_schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema")
}

#[test]
fn builder_minimal_seaorm_only() {
    // Smallest interesting pipeline: schema → seaorm.
    let tmp = tempfile::tempdir().expect("tempdir");
    let entities = tmp.path().join("entities");
    let conversions = tmp.path().join("conversions");

    Pipeline::new(fixture_schema_dir()).seaorm(&entities, &conversions).build().expect("minimal pipeline failed");

    // The fixture has 4 entities (Exercise, Tag, Workout, WorkoutSet).
    // gen_seaorm always emits a mod.rs alongside the per-entity files.
    let entity_mod = entities.join("mod.rs");
    assert!(entity_mod.exists(), "expected entity mod.rs at {}", entity_mod.display());

    // At least one per-entity file should land in the entity output dir.
    // File names use snake_case of the entity name (Exercise → exercise.rs).
    let exercise = entities.join("exercise.rs");
    assert!(exercise.exists(), "expected exercise.rs at {}", exercise.display());
}

#[test]
fn builder_realistic_schema_seaorm_store_api() {
    // Realistic shape consumers will use most: schema → seaorm → store → api.
    // No servers stage (that requires a complex ServersConfig).
    let tmp = tempfile::tempdir().expect("tempdir");
    let entities = tmp.path().join("entities");
    let conversions = tmp.path().join("conversions");
    let store_out = tmp.path().join("store");
    let hooks = tmp.path().join("hooks");
    let api_out = tmp.path().join("api");

    Pipeline::new(fixture_schema_dir())
        .seaorm(&entities, &conversions)
        .store(&store_out, Some(&hooks))
        .api(&api_out, "AppState")
        .build()
        .expect("realistic pipeline failed");

    // SeaORM stage produced entity output.
    assert!(entities.join("mod.rs").exists(), "missing entities/mod.rs");
    assert!(conversions.join("mod.rs").exists(), "missing conversions/mod.rs");

    // Store stage produced output.
    assert!(store_out.join("mod.rs").exists(), "missing store/mod.rs");
    // Hooks dir gets per-entity scaffold files.
    assert!(hooks.exists(), "expected hooks dir to be created");

    // API stage produced output.
    let api_mod = api_out.join("mod.rs");
    assert!(api_mod.exists(), "missing api/mod.rs");
    // CRUD module for one of the fixture entities.
    let exercise_api = api_out.join("exercise.rs");
    assert!(exercise_api.exists(), "missing api/exercise.rs at {}", exercise_api.display());
}

fn markdown_options() -> MarkdownIoOptions {
    MarkdownIoOptions {
        vault_root: "data/vault".into(),
        layout: MarkdownLayout::PerEntityDir,
        list_cap: 10_000,
        okf: OkfOptions::default(),
    }
}

#[test]
fn builder_markdown_pipeline_generates_store_and_api() {
    // Exactly one persistence stage configured ⇒ the store backend is
    // inferred: markdown_io output threads into a markdown-backed store, and
    // the API layer generates on top exactly as it does for SeaORM.
    let tmp = tempfile::tempdir().expect("tempdir");
    let md_out = tmp.path().join("markdown");
    let store_out = tmp.path().join("store");
    let hooks = tmp.path().join("hooks");
    let api_out = tmp.path().join("api");

    Pipeline::new(fixture_schema_dir())
        .markdown_io(&md_out, markdown_options())
        .store(&store_out, Some(&hooks))
        .api(&api_out, "AppState")
        .build()
        .expect("markdown pipeline failed");

    assert!(md_out.join("mod.rs").exists(), "missing markdown generated mod.rs");
    assert!(md_out.join("exercise.rs").exists(), "missing frontmatter module");
    let vault = std::fs::read_to_string(md_out.join("mod.rs")).expect("mod.rs");
    assert!(vault.contains("pub fn open_vault("), "{vault}");
    assert!(vault.contains(".with_list_cap(10000)\n}"), "both OKF options off: the default policy\n{vault}");
    assert!(store_out.join("mod.rs").exists(), "missing store mod.rs");
    let store_code = std::fs::read_to_string(store_out.join("exercise.rs")).unwrap();
    assert!(store_code.contains("self.vault()"), "markdown store talks to the vault:\n{store_code}");
    assert!(
        store_code.contains("&markdown_store::IdStrategy::Provided,"),
        "with no store_id_strategy the caller supplies every id:\n{store_code}"
    );
    assert!(!store_code.contains("sea_orm"), "no SeaORM in a markdown store:\n{store_code}");
    assert!(hooks.exists(), "hooks scaffolded");
    assert!(api_out.join("exercise.rs").exists(), "missing api module");
}

#[test]
fn builder_threads_okf_options_into_open_vault() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let md_out = tmp.path().join("markdown");
    let okf = OkfOptions { index: true, generated_by: Some("process:builder-test".into()) };
    Pipeline::new(fixture_schema_dir())
        .markdown_io(&md_out, MarkdownIoOptions { okf, ..markdown_options() })
        .build()
        .expect("markdown pipeline failed");

    let mod_rs = std::fs::read_to_string(md_out.join("mod.rs")).expect("mod.rs");
    assert!(mod_rs.contains("index: true,"), "{mod_rs}");
    assert!(mod_rs.contains("generated_by: Some(\"process:builder-test\".into()),"), "{mod_rs}");
}

#[test]
fn builder_with_both_persistence_stages_requires_explicit_backend() {
    let tmp = tempfile::tempdir().expect("tempdir");

    let err = Pipeline::new(fixture_schema_dir())
        .seaorm(tmp.path().join("entities"), tmp.path().join("conversions"))
        .markdown_io(tmp.path().join("markdown"), markdown_options())
        .store(tmp.path().join("store"), None::<PathBuf>)
        .build()
        .expect_err("ambiguous backend must be an error");
    assert!(format!("{err}").contains("store_backend"), "error should point at the disambiguator: {err}");

    // Explicitly choosing SeaORM resolves the ambiguity.
    let tmp2 = tempfile::tempdir().expect("tempdir");
    Pipeline::new(fixture_schema_dir())
        .seaorm(tmp2.path().join("entities"), tmp2.path().join("conversions"))
        .markdown_io(tmp2.path().join("markdown"), markdown_options())
        .store(tmp2.path().join("store"), None::<PathBuf>)
        .store_backend(StoreBackendChoice::Seaorm)
        .build()
        .expect("explicit seaorm choice should build");
    assert!(tmp2.path().join("store/mod.rs").exists());
}

#[test]
fn builder_store_without_persistence_stage_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = Pipeline::new(fixture_schema_dir())
        .store(tmp.path().join("store"), None::<PathBuf>)
        .build()
        .expect_err("store without a persistence backend must be an error");
    assert!(format!("{err}").contains("persistence backend"), "got: {err}");
}

#[test]
fn builder_store_id_strategy_reaches_either_backend() {
    let tmp = tempfile::tempdir().expect("tempdir");
    Pipeline::new(fixture_schema_dir())
        .seaorm(tmp.path().join("entities"), tmp.path().join("conversions"))
        .store(tmp.path().join("store"), None::<PathBuf>)
        .store_id_strategy(IdStrategy::Uuid)
        .build()
        .expect("seaorm pipeline");
    let seaorm = std::fs::read_to_string(tmp.path().join("store/tag.rs")).unwrap();
    assert!(seaorm.contains("ontogen_core::id::new_uuid()"), "{seaorm}");

    let tmp = tempfile::tempdir().expect("tempdir");
    Pipeline::new(fixture_schema_dir())
        .store_id_strategy(IdStrategy::Uuid)
        .markdown_io(tmp.path().join("markdown"), markdown_options())
        .store(tmp.path().join("store"), None::<PathBuf>)
        .build()
        .expect("markdown pipeline");
    let markdown = std::fs::read_to_string(tmp.path().join("store/tag.rs")).unwrap();
    assert!(markdown.contains("&markdown_store::IdStrategy::Uuid,"), "{markdown}");
}

#[test]
fn builder_slug_strategy_is_validated_on_the_seaorm_backend_too() {
    // Workout.name is Option<String>: no slug source on every entity.
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = Pipeline::new(fixture_schema_dir())
        .seaorm(tmp.path().join("entities"), tmp.path().join("conversions"))
        .store(tmp.path().join("store"), None::<PathBuf>)
        .store_id_strategy(IdStrategy::SlugFromField("name".into()))
        .build()
        .expect_err("an optional slug source must be refused");
    assert!(format!("{err}").contains("must be a plain String"), "got: {err}");
    assert!(!tmp.path().join("store").exists(), "nothing is written");
}

/// A servers stage over `schema_dir`, scanning an empty API directory.
fn servers_only(
    schema_dir: &Path,
    api_dir: &Path,
    error_source_dir: Option<PathBuf>,
) -> Result<(), ontogen::CodegenError> {
    std::fs::create_dir_all(api_dir).expect("api dir");
    Pipeline::new(schema_dir)
        .servers(ontogen::ServersConfig {
            api_dir: api_dir.to_path_buf(),
            state_type: "AppState".into(),
            service_import_path: "crate::api".into(),
            types_import_path: "crate::schema".into(),
            state_import: "crate::AppState".into(),
            naming: Default::default(),
            generators: vec![],
            rustfmt_edition: "2024".into(),
            sse_route_overrides: Default::default(),
            route_prefix: None,
            store_type: None,
            store_import: None,
            pagination: None,
            extra_surfaces: vec![],
            error_source_dir,
        })
        .build()
}

#[test]
fn builder_scans_the_schema_dir_for_app_error_unless_told_otherwise() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let schema = tmp.path().join("schema");
    std::fs::create_dir_all(&schema).unwrap();
    std::fs::write(
        schema.join("mod.rs"),
        "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Note {\n    #[ontology(id)]\n    pub id: String,\n}\n\n\
         pub enum AppError {\n    NoteNotFound(String),\n    InvalidDocument(String),\n}\n",
    )
    .unwrap();
    let api = tmp.path().join("api");

    // The clash is only reachable through the scan, so the error proves the
    // pipeline pointed it at the schema directory.
    let err = servers_only(&schema, &api, None).expect_err("a variant reusing an ontogen code must fail");
    assert!(format!("{err}").contains("AppError::InvalidDocument"), "got: {err}");

    let elsewhere = tmp.path().join("errors");
    std::fs::create_dir_all(&elsewhere).unwrap();
    servers_only(&schema, &api, Some(elsewhere)).expect("an explicit error_source_dir is kept");
}

#[test]
fn builder_servers_and_clients_refuse_an_entity_that_cannot_be_a_resource() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let schema = tmp.path().join("schema");
    std::fs::create_dir_all(&schema).unwrap();
    std::fs::write(
        schema.join("note.rs"),
        "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Note {\n    #[ontology(id)]\n    pub id: i64,\n}\n",
    )
    .unwrap();
    let api = tmp.path().join("api");

    let err = servers_only(&schema, &api, None).expect_err("an i64 id cannot be a resource id");
    assert!(matches!(err, ontogen::CodegenError::Server(ref e) if e.contains("Note.id")), "got: {err}");

    let err = Pipeline::new(&schema)
        .clients(ontogen::ClientsConfig::new(&api, "AppState", "crate::api", "crate::schema", "crate::AppState"))
        .build()
        .expect_err("an i64 id cannot be a resource id");
    assert!(matches!(err, ontogen::CodegenError::Client(ref e) if e.contains("Note.id")), "got: {err}");
}
