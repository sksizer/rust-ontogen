//! Integration tests for the `ontogen::Pipeline` builder.
//!
//! These exercise the builder against the embedded schema fixtures under
//! `tests/fixtures/schema/`, generating into tempdirs.

use std::path::{Path, PathBuf};

use ontogen::Pipeline;

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
