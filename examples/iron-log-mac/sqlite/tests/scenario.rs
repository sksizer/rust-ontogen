//! The shared scenario on SQLite, plus probes only this backend can run.

use iron_log_sqlite as backend;

/// Foreign keys are enforced, so a dangling reference is refused.
const ENFORCES_REFERENCES: bool = true;

#[path = "../../shared/tests/scenario.rs"]
mod scenario;

use backend::AppState;
use backend::api::v1::training;
use backend::domain::catalog::PERSON_ID;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

fn dir() -> String {
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("probe-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .into_owned()
}

async fn raw(state: &AppState, sql: &str) {
    let store = state.store().await.unwrap();
    store.db().execute(Statement::from_string(DatabaseBackend::Sqlite, sql)).await.unwrap();
}

/// Reopening keeps the data, and `create_tables` is a no-op on existing tables.
#[tokio::test]
async fn the_log_survives_a_reopen() {
    let dir = dir();
    {
        let state = AppState::open(&dir).await.unwrap();
        training::seed_catalog(state.store().await.unwrap(), "2026-01-01".into()).await.unwrap();
    }
    let state = AppState::open(&dir).await.unwrap();
    assert_eq!(state.store().await.unwrap().get_person(PERSON_ID).await.unwrap().display_name, "Me");
}

/// A column value the enum doesn't know, as an old build or a manual edit
/// leaves behind. The generated conversion has no way to report it.
#[tokio::test]
#[should_panic(expected = "enum should have a fallback variant")]
async fn an_unknown_enum_value_panics_the_read() {
    let state = AppState::open(&dir()).await.unwrap();
    training::seed_catalog(state.store().await.unwrap(), "2026-01-01".into()).await.unwrap();
    raw(&state, "UPDATE muscles SET region = 'cosmic' WHERE id = 'quads'").await;
    let _ = state.store().await.unwrap().get_muscle("quads").await;
}

/// A corrupt JSON column reads as an empty list, with no error, and the next
/// save makes the loss permanent by writing that empty list back.
#[tokio::test]
async fn a_corrupt_json_column_reads_as_empty() {
    let state = AppState::open(&dir()).await.unwrap();
    training::seed_catalog(state.store().await.unwrap(), "2026-01-01".into()).await.unwrap();
    raw(&state, "UPDATE exercises SET cues = 'not json' WHERE id = 'deadlift'").await;
    let exercise = state.store().await.unwrap().get_exercise("deadlift").await.unwrap();
    assert!(exercise.cues.is_empty());

    let store = state.store().await.unwrap();
    let rename = backend::store::exercise::ExerciseUpdate { name: Some("Pull".into()), ..Default::default() };
    store.update_exercise("deadlift", rename).await.unwrap();
    let row = store
        .db()
        .query_one(Statement::from_string(DatabaseBackend::Sqlite, "SELECT cues FROM exercises WHERE id = 'deadlift'"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get_by_index::<String>(0).unwrap(), "[]");
}

/// The rollup as the query it would be, if a generated op could say it.
async fn native_rollup(store: &backend::store::Store) {
    let sql = "SELECT em.muscle_id, SUM(l.load_grams / 1000.0 * r.reps * em.contribution), COUNT(*)
        FROM efforts e JOIN lifting l ON l.id = e.id JOIN reps r ON r.id = e.id
        JOIN exercise_muscles em ON em.exercise_id = l.exercise_id
        WHERE e.person_id = 'me' AND e.started_at >= '2026-06' AND l.intent != 'warmup' AND l.load_grams IS NOT NULL
        GROUP BY em.muscle_id ORDER BY 2 DESC";
    let started = std::time::Instant::now();
    let rows = store.db().query_all(Statement::from_string(DatabaseBackend::Sqlite, sql)).await.unwrap();
    println!("volume_by_muscle as raw SQL ({} rows): {:?}", rows.len(), started.elapsed());
}
