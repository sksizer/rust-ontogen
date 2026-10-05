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
        .store_id_strategy(IdStrategy::Provided)
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
        .store_id_strategy(IdStrategy::Provided)
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
        "under IdStrategy::Provided the caller supplies every id:\n{store_code}"
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
        .store_id_strategy(IdStrategy::Provided)
        .build()
        .expect_err("ambiguous backend must be an error");
    assert!(format!("{err}").contains("store_backend"), "error should point at the disambiguator: {err}");

    // Explicitly choosing SeaORM resolves the ambiguity.
    let tmp2 = tempfile::tempdir().expect("tempdir");
    Pipeline::new(fixture_schema_dir())
        .seaorm(tmp2.path().join("entities"), tmp2.path().join("conversions"))
        .markdown_io(tmp2.path().join("markdown"), markdown_options())
        .store(tmp2.path().join("store"), None::<PathBuf>)
        .store_id_strategy(IdStrategy::Provided)
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
        .store_id_strategy(IdStrategy::Provided)
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

/// A servers stage over `schema_dir` and `api_dir` running `generators`.
fn servers_with(
    schema_dir: &Path,
    api_dir: &Path,
    error_source_dir: Option<PathBuf>,
    generators: Vec<ontogen::servers::ServerGenerator>,
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
            generators,
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
    let http = || vec![ontogen::servers::ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") }];
    let err = servers_with(&schema, &api, None, http()).expect_err("a variant reusing an ontogen code must fail");
    assert!(format!("{err}").contains("AppError::InvalidDocument"), "got: {err}");

    let elsewhere = tmp.path().join("errors");
    std::fs::create_dir_all(&elsewhere).unwrap();
    servers_with(&schema, &api, Some(elsewhere), http()).expect("an explicit error_source_dir is kept");
}

/// A schema directory declaring one entity and an `AppError` with `variants`.
fn schema_with_app_error(root: &Path, entity: &str, variants: &[&str]) -> PathBuf {
    let schema = root.join("schema");
    std::fs::create_dir_all(&schema).unwrap();
    std::fs::write(
        schema.join("mod.rs"),
        format!(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct {entity} {{\n    #[ontology(id)]\n    pub id: \
             String,\n}}\n\npub enum AppError {{\n    {}\n}}\n",
            variants.iter().map(|v| format!("{v}(String),")).collect::<Vec<_>>().join("\n    ")
        ),
    )
    .unwrap();
    schema
}

#[test]
fn builder_checks_app_error_codes_only_for_an_http_server() {
    use ontogen::servers::ServerGenerator;

    // The check exists because the JSON:API server's codes share the `code`
    // namespace; IPC and MCP carry no such codes.
    let tmp = tempfile::tempdir().expect("tempdir");
    let schema = schema_with_app_error(tmp.path(), "Note", &["NoteNotFound", "InvalidDocument"]);
    let api = tmp.path().join("api");
    let ipc = ServerGenerator::TauriIpc { output: tmp.path().join("ipc.rs") };
    let mcp = ServerGenerator::Mcp { output: tmp.path().join("mcp.rs") };
    let http = ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") };

    servers_with(&schema, &api, None, vec![]).expect("no transport, no clash");
    servers_with(&schema, &api, None, vec![ipc.clone(), mcp.clone()]).expect("IPC and MCP have no ontogen codes");
    let err = servers_with(&schema, &api, None, vec![ipc, mcp, http]).expect_err("the HTTP server's codes clash");
    assert!(format!("{err}").contains("AppError::InvalidDocument"), "got: {err}");
}

#[test]
fn builder_http_code_check_accepts_store_variants_of_relationship_named_entities() {
    for entity in ["Relationship", "RelatedResource"] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let variants = ["NotFound", "IdRequired", "AlreadyExists"].map(|suffix| format!("{entity}{suffix}"));
        let schema =
            schema_with_app_error(tmp.path(), entity, &variants.iter().map(String::as_str).collect::<Vec<_>>());
        let http = ontogen::servers::ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") };
        servers_with(&schema, &tmp.path().join("api"), None, vec![http])
            .unwrap_or_else(|e| panic!("`{entity}`'s store variants must not clash: {e}"));
    }
}

/// A `Note` with `note_fields` beside a `Tag`, under `root/schema`, and a
/// `note` API module serving `get_by_id`, under `root/api`.
fn note_project(root: &Path, note_fields: &str) -> (PathBuf, PathBuf) {
    let schema = root.join("schema");
    let api = root.join("api");
    std::fs::create_dir_all(&schema).unwrap();
    std::fs::create_dir_all(&api).unwrap();
    std::fs::write(
        schema.join("mod.rs"),
        format!(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Note {{\n{note_fields}\n}}\n\n\
             #[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Tag {{\n    #[ontology(id)]\n    pub id: String,\n}}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        api.join("note.rs"),
        "use crate::schema::Note;\n\n\
         pub async fn get_by_id(state: &AppState, id: &str) -> Result<Note, anyhow::Error> { todo!() }\n",
    )
    .unwrap();
    (schema, api)
}

/// Relations JSON:API refuses, with the error an HTTP build reports.
const REFUSED_RELATIONS: [(&str, &str); 2] = [
    (
        "    #[ontology(id)]\n    pub id: String,\n    #[ontology(relation(belongs_to, target = \"Tag\"))]\n    pub type_id: String,",
        "ontogen: relation `Note.type_id` would be a relationship named `type`, which is reserved; rename the field",
    ),
    (
        "    #[ontology(id)]\n    pub id: String,\n    #[ontology(relation(belongs_to, target = \"Tag\"))]\n    pub _tag_id: String,",
        "ontogen: `Note._tag_id` would be the relationship `_tag`, which is not a legal JSON:API member name (a \
         member name has only ASCII letters and digits, non-ASCII characters, `-`, `_` and spaces, and starts and \
         ends with an ASCII letter or digit or a non-ASCII character); rename the field",
    ),
];

#[test]
fn builder_ipc_and_mcp_servers_take_a_relation_json_api_refuses() {
    for (fields, refusal) in REFUSED_RELATIONS {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (schema, api) = note_project(tmp.path(), fields);
        let errors = tmp.path().join("errors");
        std::fs::create_dir_all(&errors).unwrap();
        let ipc = tmp.path().join("ipc.rs");
        let mcp = tmp.path().join("mcp.rs");

        servers_with(
            &schema,
            &api,
            Some(errors.clone()),
            vec![
                ontogen::servers::ServerGenerator::TauriIpc { output: ipc.clone() },
                ontogen::servers::ServerGenerator::Mcp { output: mcp.clone() },
            ],
        )
        .unwrap_or_else(|e| panic!("IPC and MCP payloads are not JSON:API: {e}"));
        assert!(ipc.exists() && mcp.exists(), "both transports are written");

        let http = vec![ontogen::servers::ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") }];
        let err = servers_with(&schema, &api, Some(errors), http).expect_err("the HTTP server serves JSON:API");
        assert!(matches!(err, ontogen::CodegenError::Server(ref e) if e == refusal), "got: {err}");
    }
}

#[test]
fn builder_admin_registry_takes_a_relation_json_api_refuses() {
    use ontogen::clients::ClientGenerator;

    for (fields, refusal) in REFUSED_RELATIONS {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (schema, api) = note_project(tmp.path(), fields);
        let clients = |generators: Vec<ClientGenerator>| {
            Pipeline::new(&schema)
                .clients(ontogen::ClientsConfig {
                    generators,
                    ..ontogen::ClientsConfig::new(&api, "AppState", "crate::api", "crate::schema", "crate::AppState")
                })
                .build()
        };

        let registry = tmp.path().join("admin-registry.ts");
        clients(vec![ClientGenerator::AdminRegistry { output: registry.clone() }])
            .unwrap_or_else(|e| panic!("the admin registry is not JSON:API: {e}"));
        assert!(registry.exists(), "the registry is written");

        let bindings_path = tmp.path().join("bindings.ts");
        for http in [
            ClientGenerator::HttpTs { output: tmp.path().join("http.ts"), bindings_path: bindings_path.clone() },
            ClientGenerator::HttpTauriIpcSplit {
                output: tmp.path().join("transport.ts"),
                bindings_path: bindings_path.clone(),
            },
        ] {
            let err = clients(vec![http]).expect_err("an HTTP client speaks JSON:API");
            assert!(matches!(err, ontogen::CodegenError::Client(ref e) if e == refusal), "got: {err}");
        }
    }
}

#[test]
fn builder_http_servers_and_clients_refuse_an_entity_that_cannot_be_a_resource() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (schema, api) = note_project(tmp.path(), "    #[ontology(id)]\n    pub id: i64,");
    let errors = tmp.path().join("errors");
    std::fs::create_dir_all(&errors).unwrap();

    let http = vec![ontogen::servers::ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") }];
    let err = servers_with(&schema, &api, Some(errors), http).expect_err("an i64 id cannot be a resource id");
    assert!(matches!(err, ontogen::CodegenError::Server(ref e) if e.contains("Note.id")), "got: {err}");

    let err = Pipeline::new(&schema)
        .clients(ontogen::ClientsConfig {
            generators: vec![ontogen::clients::ClientGenerator::HttpTs {
                output: tmp.path().join("http.ts"),
                bindings_path: tmp.path().join("bindings.ts"),
            }],
            ..ontogen::ClientsConfig::new(&api, "AppState", "crate::api", "crate::schema", "crate::AppState")
        })
        .build()
        .expect_err("an i64 id cannot be a resource id");
    assert!(matches!(err, ontogen::CodegenError::Client(ref e) if e.contains("Note.id")), "got: {err}");
}

#[test]
fn builder_store_without_id_strategy_errors_before_writing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = Pipeline::new(fixture_schema_dir())
        .seaorm(tmp.path().join("entities"), tmp.path().join("conversions"))
        .store(tmp.path().join("store"), None::<PathBuf>)
        .build()
        .expect_err("a store stage without a default id strategy must be an error");
    let msg = format!("{err}");
    assert!(msg.contains("call Pipeline::store_id_strategy("), "the error names the setter: {msg}");
    assert!(!tmp.path().join("entities").exists(), "no stage runs before the check");
    assert!(!tmp.path().join("store").exists(), "nothing is written");

    // Without a store stage there is nothing to default.
    Pipeline::new(fixture_schema_dir())
        .seaorm(tmp.path().join("entities"), tmp.path().join("conversions"))
        .build()
        .expect("no store, no id strategy needed");
}

/// A schema whose entities set their own id strategy, written to a temp dir.
fn override_schema(dir: &Path, task_id: &str) {
    std::fs::write(
        dir.join("note.rs"),
        r#"
            #[derive(OntologyEntity)]
            #[ontology(entity, directory = "notes", table = "notes")]
            pub struct Note {
                #[ontology(id)]
                pub id: String,
                pub title: String,
            }
        "#,
    )
    .unwrap();
    std::fs::write(
        dir.join("task.rs"),
        format!(
            r#"
            #[derive(OntologyEntity)]
            #[ontology(entity, directory = "tasks", table = "tasks", id = "{task_id}")]
            pub struct Task {{
                #[ontology(id)]
                pub id: String,
                pub summary: String,
                pub points: Option<i32>,
            }}
        "#
        ),
    )
    .unwrap();
}

#[test]
fn builder_entity_id_override_beats_the_store_default() {
    for (task_id, task_needle) in [
        ("uuid", "ontogen_core::id::new_uuid()"),
        ("slug(summary)", "ontogen_core::id::slugify(&record.summary)"),
        ("provided", "this store requires the caller to supply an id"),
    ] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let schema = tmp.path().join("schema");
        std::fs::create_dir_all(&schema).unwrap();
        // Task has no `title`: the default alone would be refused for it.
        override_schema(&schema, task_id);
        Pipeline::new(&schema)
            .seaorm(tmp.path().join("entities"), tmp.path().join("conversions"))
            .store(tmp.path().join("store"), None::<PathBuf>)
            .store_id_strategy(IdStrategy::SlugFromField("title".into()))
            .build()
            .unwrap_or_else(|e| panic!("id = {task_id:?}: {e}"));
        let task = std::fs::read_to_string(tmp.path().join("store/task.rs")).unwrap();
        let note = std::fs::read_to_string(tmp.path().join("store/note.rs")).unwrap();
        assert!(task.contains(task_needle), "id = {task_id:?}: the override wins:\n{task}");
        assert!(!task.contains("&record.title"), "id = {task_id:?}: not the default:\n{task}");
        assert!(note.contains("ontogen_core::id::slugify(&record.title)"), "the default applies to Note:\n{note}");
    }
}

#[test]
fn builder_entity_id_override_is_checked_at_build_time() {
    for (task_id, needle) in [
        ("slug(points)", "field `points` must be a plain String"),
        ("slug(missing)", "has no field `missing`"),
        ("random", "expected `id = \"provided\"`, `id = \"uuid\"` or `id = \"slug(<field>)\"`"),
    ] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let schema = tmp.path().join("schema");
        std::fs::create_dir_all(&schema).unwrap();
        override_schema(&schema, task_id);
        let err = Pipeline::new(&schema)
            .seaorm(tmp.path().join("entities"), tmp.path().join("conversions"))
            .store(tmp.path().join("store"), None::<PathBuf>)
            .store_id_strategy(IdStrategy::Provided)
            .build()
            .expect_err("a bad override must fail the build");
        let msg = format!("{err}");
        assert!(msg.contains("entity `Task`") && msg.contains(needle), "id = {task_id:?}: {msg}");
        assert!(!tmp.path().join("store").exists(), "nothing is written");
    }
}

#[test]
fn builder_requires_the_default_even_when_every_entity_overrides_it() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let schema = tmp.path().join("schema");
    std::fs::create_dir_all(&schema).unwrap();
    override_schema(&schema, "uuid");
    std::fs::remove_file(schema.join("note.rs")).unwrap();
    let err = Pipeline::new(&schema)
        .markdown_io(tmp.path().join("markdown"), markdown_options())
        .store(tmp.path().join("store"), None::<PathBuf>)
        .build()
        .expect_err("the default is required");
    assert!(format!("{err}").contains("call Pipeline::store_id_strategy("), "got: {err}");
}

#[test]
fn builder_api_scans_the_transports_api_dirs_unless_told_otherwise() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let api = tmp.path().join("api");
    std::fs::create_dir_all(&api).unwrap();
    std::fs::write(
        api.join("workout.rs"),
        "use crate::schema::{AppError, Workout};\nuse crate::store::Store;\n\n\
         pub async fn list(store: &Store, name: Option<String>) -> Result<Vec<Workout>, AppError> { todo!() }\n",
    )
    .unwrap();
    let generated = api.join("generated");

    Pipeline::new(fixture_schema_dir())
        .api(&generated, "AppState")
        .api_store_type(Some("Store".into()))
        .servers(ontogen::ServersConfig {
            api_dir: api.clone(),
            state_type: "AppState".into(),
            service_import_path: "crate::api".into(),
            types_import_path: "crate::schema".into(),
            state_import: "crate::AppState".into(),
            naming: Default::default(),
            generators: vec![],
            sse_route_overrides: Default::default(),
            route_prefix: None,
            store_type: Some("Store".into()),
            store_import: None,
            pagination: None,
            extra_surfaces: vec![],
            error_source_dir: Some(tmp.path().to_path_buf()),
        })
        .build()
        .expect("pipeline failed");

    let workout = std::fs::read_to_string(generated.join("workout.rs")).unwrap();
    assert!(!workout.contains("fn list("), "the hand-written list replaces it:\n{workout}");
    assert!(workout.contains("fn get_by_id("), "{workout}");
    let tag = std::fs::read_to_string(generated.join("tag.rs")).unwrap();
    assert!(tag.contains("fn list("), "other entities keep their list:\n{tag}");
}

const HAND_WRITTEN_LIST: &str = "use crate::schema::{AppError, Workout};\nuse crate::store::Store;\n\n\
     pub async fn list(store: &Store, name: Option<String>) -> Result<Vec<Workout>, AppError> { todo!() }\n";

fn servers_config(api: &Path, error_source_dir: &Path) -> ontogen::ServersConfig {
    ontogen::ServersConfig {
        api_dir: api.to_path_buf(),
        state_type: "AppState".into(),
        service_import_path: "crate::api".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: Default::default(),
        generators: vec![],
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: None,
        pagination: None,
        extra_surfaces: vec![],
        error_source_dir: Some(error_source_dir.to_path_buf()),
    }
}

#[test]
fn builder_explicit_api_scan_dirs_add_to_the_transports_dirs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let api = tmp.path().join("api");
    let other = tmp.path().join("other");
    std::fs::create_dir_all(&api).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(api.join("workout.rs"), HAND_WRITTEN_LIST).unwrap();
    let generated = api.join("generated");

    Pipeline::new(fixture_schema_dir())
        .api(&generated, "AppState")
        .api_store_type(Some("Store".into()))
        .api_scan_dirs(vec![other])
        .servers(servers_config(&api, tmp.path()))
        .build()
        .expect("pipeline failed");

    let workout = std::fs::read_to_string(generated.join("workout.rs")).unwrap();
    assert!(!workout.contains("fn list("), "the servers' api_dir is still scanned:\n{workout}");
}

#[test]
fn builder_direct_gen_api_without_the_servers_dir_gets_the_same_surface_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let api = tmp.path().join("api");
    std::fs::create_dir_all(&api).unwrap();
    std::fs::write(api.join("workout.rs"), HAND_WRITTEN_LIST).unwrap();
    let generated = api.join("generated");

    let schema = ontogen::parse_schema(&ontogen::SchemaConfig { schema_dir: fixture_schema_dir() }).expect("schema");
    let api_out = ontogen::gen_api(
        &schema.entities,
        &ontogen::ApiConfig {
            output_dir: generated,
            exclude: vec![],
            scan_dirs: vec![],
            state_type: "AppState".into(),
            store_type: Some("Store".into()),
            schema_module_path: ontogen::DEFAULT_SCHEMA_MODULE_PATH.into(),
            paginated: vec![],
        },
    )
    .expect("gen_api");
    let Err(err) = ontogen::gen_servers(&schema, Some(&api_out), &[], &servers_config(&api, tmp.path())) else {
        panic!("the generated and the hand-written list collide");
    };
    let msg = err.to_string();
    assert!(msg.contains("fn `workout::list` is defined twice in API directory"), "{msg}");
    assert!(msg.contains("ApiConfig::scan_dirs"), "{msg}");
}

#[test]
fn builder_a_stateless_hand_written_list_does_not_replace_the_generated_one() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let api = tmp.path().join("api");
    std::fs::create_dir_all(&api).unwrap();
    std::fs::write(
        api.join("workout.rs"),
        "use crate::schema::{AppError, Workout};\n\n#[ontogen::stateless]\n\
         pub async fn list() -> Result<Vec<Workout>, AppError> { todo!() }\n",
    )
    .unwrap();
    let generated = api.join("generated");

    let err = Pipeline::new(fixture_schema_dir())
        .api(&generated, "AppState")
        .api_store_type(Some("Store".into()))
        .servers(servers_config(&api, tmp.path()))
        .build()
        .expect_err("the generated list stays, so the names collide");
    let msg = err.to_string();
    assert!(msg.contains("fn `workout::list` is defined twice in API directory"), "{msg}");
    assert!(msg.contains("never replaces it"), "{msg}");
}

#[test]
fn builder_a_missing_explicit_api_scan_dir_is_an_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = Pipeline::new(fixture_schema_dir())
        .api(tmp.path().join("generated"), "AppState")
        .api_scan_dirs(vec![tmp.path().join("nope")])
        .build()
        .expect_err("a missing explicit dir is refused");
    assert!(err.to_string().contains("API scan directory does not exist"), "{err}");
}

#[test]
fn builder_a_missing_transport_api_dir_is_skipped_by_the_api_stage() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let generated = tmp.path().join("elsewhere/generated");
    let err = Pipeline::new(fixture_schema_dir())
        .api(&generated, "AppState")
        .servers(servers_config(&tmp.path().join("missing"), tmp.path()))
        .build()
        .expect_err("the servers stage reports its own missing directory");
    let msg = err.to_string();
    assert!(msg.contains("API directory does not exist"), "{msg}");
    assert!(generated.join("workout.rs").exists(), "the api stage ran");
}

/// A schema directory under `root` with one entity named `entity`.
fn schema_with_entity(root: &Path, entity: &str) -> PathBuf {
    let schema = root.join("schema");
    std::fs::create_dir_all(&schema).unwrap();
    std::fs::write(
        schema.join("mod.rs"),
        format!(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct {entity} {{\n    #[ontology(id)]\n    pub id: \
             String,\n}}\n"
        ),
    )
    .unwrap();
    schema
}

#[test]
fn builder_refuses_an_entity_named_like_what_the_output_names_bare() {
    // Refused at schema parse, whatever the pipeline generates.
    for (entity, why) in [
        ("Result", "the generated code names the prelude's `Result` bare"),
        ("Option", "the generated code names the prelude's `Option` bare"),
        ("OntogenListParams", "the `Ontogen` prefix is reserved"),
        ("Std", "its module `std` would shadow the crate `std`"),
    ] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = Pipeline::new(schema_with_entity(tmp.path(), entity))
            .dtos(tmp.path().join("dto"))
            .build()
            .expect_err(entity);
        assert!(matches!(err, ontogen::CodegenError::Schema(ref e) if e.contains(why)), "{entity}: {err}");
        let rename = entity.strip_prefix("Ontogen").map_or(format!("{entity}Item"), str::to_string);
        assert!(format!("{err}").contains(&format!("rename the entity (e.g. `{rename}`)")), "{err}");
        assert!(!tmp.path().join("dto").exists(), "nothing is written");
    }

    // Refused when the store is generated, before any stage writes.
    for (entity, why) in [
        ("Store", "import the consumer contract's `Store` bare"),
        ("OrderBy", "import the consumer contract's `OrderBy` bare"),
    ] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let schema = schema_with_entity(tmp.path(), entity);
        let err = Pipeline::new(&schema)
            .markdown_io(tmp.path().join("md"), markdown_options())
            .store(tmp.path().join("store"), None::<PathBuf>)
            .store_id_strategy(IdStrategy::Provided)
            .build()
            .expect_err(entity);
        assert!(matches!(err, ontogen::CodegenError::Store(ref e) if e.contains(why)), "{entity}: {err}");
        assert!(!tmp.path().join("md").exists(), "{entity}: refused before the markdown stage writes");

        // Without a store, the name is free.
        Pipeline::new(&schema).markdown_io(tmp.path().join("md"), markdown_options()).build().expect(entity);
    }

    // The store's modules are the API forwarders' to refuse.
    let tmp = tempfile::tempdir().expect("tempdir");
    let schema = schema_with_entity(tmp.path(), "Hooks");
    let err = Pipeline::new(&schema)
        .markdown_io(tmp.path().join("md"), markdown_options())
        .store(tmp.path().join("store"), None::<PathBuf>)
        .store_id_strategy(IdStrategy::Provided)
        .api(tmp.path().join("api"), "AppState")
        .build()
        .expect_err("Hooks");
    assert!(
        matches!(err, ontogen::CodegenError::Api(ref e) if e.contains("they reach its store module as `crate::store::hooks`")),
        "{err}"
    );
    assert!(!tmp.path().join("md").exists(), "refused before the markdown stage writes");

    // The configured state and store types are refused before any stage
    // writes, so no hooks scaffold outlives the refusal.
    for (entity, role, store_type) in [("AppState", "state", "Store"), ("Vault", "store", "Vault")] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let schema = schema_with_entity(tmp.path(), entity);
        let api = tmp.path().join("api");
        std::fs::create_dir_all(&api).unwrap();
        let mcp = ontogen::servers::ServerGenerator::Mcp { output: tmp.path().join("mcp.rs") };
        let err = Pipeline::new(&schema)
            .markdown_io(tmp.path().join("md"), markdown_options())
            .dtos(tmp.path().join("dto"))
            .store(tmp.path().join("store"), Some(tmp.path().join("hooks")))
            .store_id_strategy(IdStrategy::Provided)
            .api(tmp.path().join("api_out"), "AppState")
            .servers(ontogen::ServersConfig {
                api_dir: api.clone(),
                state_type: "crate::AppState".into(),
                service_import_path: "crate::api".into(),
                types_import_path: "crate::schema".into(),
                state_import: "crate::AppState".into(),
                naming: Default::default(),
                generators: vec![mcp],
                sse_route_overrides: Default::default(),
                route_prefix: None,
                store_type: Some(store_type.into()),
                store_import: Some(format!("crate::store::{store_type}")),
                pagination: None,
                extra_surfaces: vec![],
                error_source_dir: None,
            })
            .build()
            .expect_err(entity);
        let configured = format!("configured {role} type");
        assert!(
            matches!(err, ontogen::CodegenError::Schema(ref e) if e.contains(&configured) && e.contains(&format!("`{entity}Item`"))),
            "{entity}: {err}"
        );
        for written in ["md", "dto", "store", "hooks", "api_out"] {
            assert!(!tmp.path().join(written).exists(), "{entity}: nothing is written ({written})");
        }
    }
}

#[test]
fn builder_refuses_an_api_argument_no_transport_can_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let schema = schema_with_entity(tmp.path(), "Note");
    let api = tmp.path().join("api");
    std::fs::create_dir_all(&api).unwrap();
    std::fs::write(
        api.join("note.rs"),
        "pub async fn find(state: &AppState, ontogen_state: String) -> Result<Vec<Note>, anyhow::Error> { todo!() }\n",
    )
    .unwrap();
    let mcp = ontogen::servers::ServerGenerator::Mcp { output: tmp.path().join("mcp.rs") };
    let err = servers_with(&schema, &api, None, vec![mcp]).expect_err("refused");
    assert!(
        matches!(err, ontogen::CodegenError::Server(ref e) if e.contains("API fn `note::find` takes an argument named `ontogen_state`: the `ontogen_` prefix is reserved")),
        "{err}"
    );
    assert!(!tmp.path().join("mcp.rs").exists(), "nothing is written");

    // A TypeScript-only build scans with the same rules.
    let err = Pipeline::new(&schema)
        .clients(ontogen::ClientsConfig {
            generators: vec![ontogen::clients::ClientGenerator::HttpTauriIpcSplit {
                output: tmp.path().join("transport.ts"),
                bindings_path: tmp.path().join("types.ts"),
            }],
            ..ontogen::ClientsConfig::new(&api, "AppState", "crate::api", "crate::schema", "crate::AppState")
        })
        .build()
        .expect_err("refused");
    assert!(
        matches!(err, ontogen::CodegenError::Client(ref e) if e.contains("`note::find` takes an argument named `ontogen_state`")),
        "{err}"
    );
}
