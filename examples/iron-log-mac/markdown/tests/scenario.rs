//! The shared scenario on a markdown vault, plus probes only this backend can run.

use iron_log_markdown as backend;

/// Files have no foreign keys, so a dangling reference is written as given.
const ENFORCES_REFERENCES: bool = false;

#[path = "../../shared/tests/scenario.rs"]
mod scenario;

use backend::AppState;
use backend::api::v1::training;
use backend::domain::catalog::PERSON_ID;
use backend::schema::AppError;

fn dir() -> String {
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("probe-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .into_owned()
}

async fn seeded(dir: &str) -> AppState {
    let state = AppState::open(dir).await.unwrap();
    training::seed_catalog(state.store().await.unwrap(), "2026-01-01".into()).await.unwrap();
    state
}

fn edit(dir: &str, file: &str, from: &str, to: &str) {
    let path = std::path::Path::new(dir).join("vault").join(file);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{text}");
    std::fs::write(path, text.replace(from, to)).unwrap();
}

#[tokio::test]
async fn the_log_survives_a_reopen() {
    let dir = dir();
    seeded(&dir).await;
    let state = AppState::open(&dir).await.unwrap();
    assert_eq!(state.store().await.unwrap().get_person(PERSON_ID).await.unwrap().display_name, "Me");
}

/// A hand edit the enum doesn't know surfaces as an error, on the read and on
/// every list that includes the file.
#[tokio::test]
async fn an_unknown_enum_value_fails_the_read_and_the_whole_list() {
    let dir = dir();
    let state = seeded(&dir).await;
    edit(&dir, "muscles/quads.md", "region: lower", "region: cosmic");
    let store = state.store().await.unwrap();
    let read = store.get_muscle("quads").await;
    println!("get: {read:?}");
    assert!(matches!(read, Err(AppError::Md(_))));
    let list = store.list_muscles(&[], None, None).await;
    println!("list: {:?}", list.as_ref().map(Vec::len));
    assert!(list.is_err(), "one bad file fails the list");
}

/// A hand edit shows on the next read: the store keeps no cache to invalidate.
#[tokio::test]
async fn a_hand_edit_shows_on_the_next_read_without_a_change_event() {
    let dir = dir();
    let state = seeded(&dir).await;
    let mut changes = state.changes();
    edit(&dir, "muscles/quads.md", "name: Quadriceps", "name: Quads");
    let store = state.store().await.unwrap();
    assert_eq!(store.get_muscle("quads").await.unwrap().name, "Quads");
    assert!(changes.try_recv().is_err(), "no event for an edit on disk");
}

/// There is no query language to compare the in-memory rollup against.
async fn native_rollup(_store: &backend::store::Store) {}
