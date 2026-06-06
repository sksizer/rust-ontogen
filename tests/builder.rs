//! Integration tests for the `ontogen::Pipeline` builder.
//!
//! These exercise the builder against the embedded schema fixtures under
//! `tests/fixtures/schema/`, generating into tempdirs.

use std::path::{Path, PathBuf};

use ontogen::{IdStrategy, MarkdownIoOptions, MarkdownLayout, Pipeline, StoreBackendChoice};

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
        // The fixture schema has no slug-able String field shared by every
        // entity (Workout.name is Option<String>, which SlugFromField
        // rejects at generation time), so the builder tests use caller-
        // supplied ids; slug derivation is covered by the store unit tests.
        id_strategy: IdStrategy::Provided,
        list_cap: 10_000,
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
    assert!(store_out.join("mod.rs").exists(), "missing store mod.rs");
    let store_code = std::fs::read_to_string(store_out.join("exercise.rs")).unwrap();
    assert!(store_code.contains("self.vault()"), "markdown store talks to the vault:\n{store_code}");
    assert!(!store_code.contains("sea_orm"), "no SeaORM in a markdown store:\n{store_code}");
    assert!(hooks.exists(), "hooks scaffolded");
    assert!(api_out.join("exercise.rs").exists(), "missing api module");
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
