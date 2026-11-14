//! One training history, run against whichever backend includes this file.
//! Where the two differ on purpose, `ENFORCES_REFERENCES` says so, which makes
//! each divergence a tested fact and not a claim in a README.

use super::ENFORCES_REFERENCES;
use super::backend::AppState;
use super::backend::api::v1::training;
use super::backend::domain::catalog::{PERSON_ID, STRENGTH_KIND_ID};
use super::backend::schema::*;
use super::backend::store::Store;
use super::backend::store::effort::EffortSortField;
use super::backend::store::muscle::MuscleUpdate;
use super::backend::store::session::SessionUpdate;
use ontogen_core::order::OrderBy;

const NOW: &str = "2026-01-01T00:00:00Z";

fn scratch_dir() -> String {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("scenario-{}", uuid::Uuid::new_v4()));
    dir.to_string_lossy().into_owned()
}

async fn open() -> AppState {
    let state = AppState::open(&scratch_dir()).await.expect("open");
    training::seed_catalog(state.store().await.unwrap(), NOW.into()).await.expect("seed");
    state
}

fn set(
    id: &str,
    session: Option<&str>,
    exercise: &str,
    position: i32,
    intent: Intent,
    grams: i32,
    reps: i32,
) -> LogSetInput {
    LogSetInput {
        id: id.into(),
        person_id: PERSON_ID.into(),
        session_id: session.map(Into::into),
        exercise_id: exercise.into(),
        kind_id: STRENGTH_KIND_ID.into(),
        started_at: format!("2026-02-01T10:{position:02}:00Z"),
        position: Some(position),
        intent,
        load_grams: Some(grams),
        reps,
        rpe: Some(8.5),
    }
}

async fn session(store: &Store, id: &str, tags: &[&str]) -> Session {
    store
        .create_session(Session {
            id: id.into(),
            person_id: PERSON_ID.into(),
            activity_kind_id: STRENGTH_KIND_ID.into(),
            location_id: Some("home-gym".into()),
            title: Some("Push day".into()),
            started_at: "2026-02-01T10:00:00Z".into(),
            ended_at: None,
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
            notes: None,
        })
        .await
        .expect("create session")
}

#[tokio::test]
async fn a_training_day_reads_back_joined_and_rolled_up() {
    let state = open().await;
    let store = state.store().await.unwrap();
    session(store, "s1", &["heavy"]).await;

    training::log_set(store, set("e1", Some("s1"), "bench-press", 1, Intent::Warmup, 40_000, 10)).await.unwrap();
    training::log_set(store, set("e2", Some("s1"), "bench-press", 2, Intent::Working, 100_000, 5)).await.unwrap();
    training::log_set(store, set("e3", Some("s1"), "overhead-press", 3, Intent::Working, 60_000, 5)).await.unwrap();

    let detail = training::session_detail(store, "s1".into()).await.unwrap();
    assert_eq!(detail.location_name.as_deref(), Some("Home Gym"));
    assert_eq!(detail.tag_names, vec!["Heavy"]);
    assert_eq!(detail.sets.iter().map(|s| s.effort_id.as_str()).collect::<Vec<_>>(), ["e1", "e2", "e3"]);
    assert_eq!(detail.sets[1].exercise_name, "Bench Press");
    assert_eq!(detail.sets[1].rpe, Some(8.5));

    // Warmup excluded. Bench 500 kg: pecs 300, triceps 150, delts 50. OHP 300 kg: delts 180, triceps 120.
    let volumes = training::volume_by_muscle(store, PERSON_ID.into(), "2026-01-01".into()).await.unwrap();
    let of = |id: &str| volumes.iter().find(|v| v.muscle_id == id).map(|v| (v.volume_kg.round() as i64, v.sets));
    assert_eq!(of("pec-major"), Some((300, 1)));
    assert_eq!(of("triceps"), Some((270, 2)));
    assert_eq!(of("delts"), Some((230, 2)));
    assert_eq!(volumes[0].muscle_id, "pec-major");
    assert!(training::volume_by_muscle(store, PERSON_ID.into(), "2027".into()).await.unwrap().is_empty());

    let best = training::best_sets(store, PERSON_ID.into()).await.unwrap();
    assert_eq!(best[0].exercise_id, "bench-press");
    assert_eq!(best[0].load_grams, 100_000, "the warmup is not a record");
    assert_eq!(best[0].e1rm_grams, 116_667);

    training::delete_session_cascade(store, "s1".into()).await.unwrap();
    assert!(matches!(store.get_effort("e2").await, Err(AppError::EffortNotFound(_))));
    assert!(matches!(store.get_lifting_facet("e2").await, Err(AppError::LiftingFacetNotFound(_))));
    assert!(matches!(store.get_session("s1").await, Err(AppError::SessionNotFound(_))));
}

#[tokio::test]
async fn the_self_reference_reads_from_both_ends_and_reparents() {
    let state = open().await;
    let store = state.store().await.unwrap();

    let pec = store.get_muscle("pec-major").await.unwrap();
    assert_eq!(pec.heads, ["pec-clavicular", "pec-sternal"], "parent_id derives heads");
    assert_eq!(store.get_muscle("pec-sternal").await.unwrap().parent_id.as_deref(), Some("pec-major"));

    // Writing the has_many side moves the child.
    store
        .update_muscle("delts", MuscleUpdate { heads: Some(vec!["pec-sternal".into()]), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(store.get_muscle("pec-sternal").await.unwrap().parent_id.as_deref(), Some("delts"));
    assert_eq!(store.get_muscle("pec-major").await.unwrap().heads, ["pec-clavicular"]);

    // A cycle is nobody's job to refuse: the schema can't state it's a tree.
    store
        .update_muscle(
            "pec-major",
            MuscleUpdate { parent_id: Some(Some("pec-clavicular".into())), ..Default::default() },
        )
        .await
        .unwrap();
    assert_eq!(store.get_muscle("pec-clavicular").await.unwrap().parent_id.as_deref(), Some("pec-major"));
}

#[tokio::test]
async fn many_to_many_round_trips_and_clears() {
    let state = open().await;
    let store = state.store().await.unwrap();

    let mut groups = store.get_muscle("pec-major").await.unwrap().groups;
    groups.sort();
    assert_eq!(groups, ["chest", "push"]);

    session(store, "s1", &["heavy", "deload"]).await;
    let mut tags = store.get_session("s1").await.unwrap().tags;
    tags.sort();
    assert_eq!(tags, ["deload", "heavy"]);

    store.update_session("s1", SessionUpdate { tags: Some(vec![]), ..Default::default() }).await.unwrap();
    assert!(store.get_session("s1").await.unwrap().tags.is_empty());
}

#[tokio::test]
async fn every_field_shape_survives_the_round_trip() {
    let state = open().await;
    let store = state.store().await.unwrap();

    let cues = vec![
        Cue { text: "Brace: \"hard\"".into(), phase: Some("setup".into()) },
        Cue { text: "Drive\nup".into(), phase: None },
    ];
    let created = store
        .create_exercise(Exercise {
            id: "hip-thrust".into(),
            name: "Hip Thrust".into(),
            family_id: None,
            equipment_id: Some("barbell".into()),
            primary_group_id: "legs".into(),
            person_id: Some(PERSON_ID.into()),
            aka: vec!["bridge".into(), "thrust".into()],
            cues: cues.clone(),
            mechanic: None,
            bodyweight_factor: Some(0.65),
            unilateral: true,
            description: "# Setup\n\nBench at the **shoulder blades**.\n".into(),
        })
        .await
        .unwrap();
    assert_eq!(created.cues, cues);
    assert_eq!(created.aka, ["bridge", "thrust"]);
    assert_eq!(created.mechanic, None);
    assert_eq!(created.bodyweight_factor, Some(0.65));
    assert!(created.unilateral);
    assert_eq!(created.description, "# Setup\n\nBench at the **shoulder blades**.\n");

    let effort = store
        .create_effort(Effort {
            id: "e-types".into(),
            person_id: PERSON_ID.into(),
            session_id: None,
            position: None,
            kind_id: "cardio".into(),
            started_at: NOW.into(),
            duration_secs: Some(1800),
            bodyweight_grams: Some(5_000_000_000),
            labels: vec!["easy".into()],
            notes: Some("line one\nline two".into()),
        })
        .await
        .unwrap();
    assert_eq!(effort.bodyweight_grams, Some(5_000_000_000), "i64 beyond i32");
    assert_eq!(effort.notes.as_deref(), Some("line one\nline two"));

    let run = store
        .create_distance_facet(DistanceFacet { id: "e-types".into(), metres: 5000.125, location_id: None })
        .await
        .unwrap();
    assert_eq!(run.metres, 5000.125);

    // Clearing an optional relation reads back as None.
    session(store, "s1", &[]).await;
    let cleared =
        store.update_session("s1", SessionUpdate { location_id: Some(None), ..Default::default() }).await.unwrap();
    assert_eq!(cleared.location_id, None);
}

#[tokio::test]
async fn sort_order_and_paging_agree() {
    let state = open().await;
    let store = state.store().await.unwrap();
    for (id, pos) in [("a", 3), ("b", 1), ("c", 2)] {
        training::log_set(store, set(id, None, "bench-press", pos, Intent::Working, 50_000, 5)).await.unwrap();
    }
    let order = [OrderBy::desc(EffortSortField::Position)];
    let ids = |v: Vec<Effort>| v.into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids(store.list_efforts(&order, None, None).await.unwrap()), ["a", "c", "b"]);
    assert_eq!(ids(store.list_efforts(&order, Some(1), Some(1)).await.unwrap()), ["c"]);
    assert_eq!(store.count_efforts().await.unwrap(), 3);
}

#[tokio::test]
async fn the_schema_cannot_state_these_rules_so_neither_backend_keeps_them() {
    let state = open().await;
    let store = state.store().await.unwrap();

    // The pair is unique in meaning, and a second row with the same pair is accepted.
    store
        .create_exercise_muscle(ExerciseMuscle {
            id: "dup".into(),
            exercise_id: "bench-press".into(),
            muscle_id: "pec-major".into(),
            role: MuscleRole::Primary,
            // And a share above 1: no CHECK.
            contribution: 7.0,
        })
        .await
        .expect("no composite unique, no range check");

    // A note on nothing: a polymorphic pointer has no reference to enforce.
    store
        .create_note(Note {
            id: "n1".into(),
            subject_kind: SubjectKind::Exercise,
            subject_id: "no-such-exercise".into(),
            body: "hello".into(),
            created_at: NOW.into(),
        })
        .await
        .expect("polymorphic pointer is unchecked");

    // A facet with no effort: the shared primary key isn't declared.
    store.create_reps_facet(RepsFacet { id: "no-such-effort".into(), reps: 5 }).await.expect("shared PK is undeclared");
}

#[tokio::test]
async fn a_failed_set_leaves_nothing_behind() {
    let state = open().await;
    let store = state.store().await.unwrap();
    store.create_reps_facet(RepsFacet { id: "taken".into(), reps: 1 }).await.unwrap();

    let err =
        training::log_set(store, set("taken", None, "bench-press", 1, Intent::Working, 50_000, 5)).await.unwrap_err();
    assert!(matches!(err, AppError::RepsFacetAlreadyExists(_)), "{err:?}");
    assert!(matches!(store.get_effort("taken").await, Err(AppError::EffortNotFound(_))));
    assert!(matches!(store.get_lifting_facet("taken").await, Err(AppError::LiftingFacetNotFound(_))));
}

/// The divergence: the same call, a different outcome per backend.
#[tokio::test]
async fn references_are_enforced_only_where_there_are_foreign_keys() {
    let state = open().await;
    let store = state.store().await.unwrap();
    training::log_set(store, set("e1", None, "bench-press", 1, Intent::Working, 50_000, 5)).await.unwrap();
    session(store, "s1", &["heavy"]).await;

    let dangling = store
        .create_location(Location {
            id: "orphan".into(),
            person_id: "no-such-person".into(),
            name: "Nowhere".into(),
            latitude: None,
            longitude: None,
        })
        .await;
    let delete_exercise = store.delete_exercise("bench-press").await;
    let delete_parent = store.delete_muscle("pec-major").await;
    let delete_tag = store.delete_tag("heavy").await;

    println!("dangling create: {dangling:?}");
    println!("delete referenced exercise: {delete_exercise:?}");
    println!("delete parent muscle: {delete_parent:?}");
    println!("delete tag in use: {delete_tag:?}");

    if ENFORCES_REFERENCES {
        assert!(matches!(dangling, Err(AppError::DbError(_))), "{dangling:?}");
        assert!(matches!(delete_exercise, Err(AppError::DbError(_))), "{delete_exercise:?}");
        assert!(matches!(delete_parent, Err(AppError::DbError(_))), "{delete_parent:?}");
        assert!(matches!(delete_tag, Err(AppError::DbError(_))), "{delete_tag:?}");
    } else {
        assert!(dangling.is_ok() && delete_exercise.is_ok() && delete_parent.is_ok() && delete_tag.is_ok());
        // The references are still there, now pointing at nothing.
        assert_eq!(store.get_lifting_facet("e1").await.unwrap().exercise_id, "bench-press");
        assert_eq!(store.get_muscle("pec-sternal").await.unwrap().parent_id.as_deref(), Some("pec-major"));
        assert_eq!(store.get_session("s1").await.unwrap().tags, ["heavy"]);
        // So the read that follows them breaks, long after the delete that caused it.
        let read = training::session_detail(store, "s1".into()).await;
        assert!(matches!(read, Err(AppError::TagNotFound(_))), "{:?}", read.map(|d| d.tag_names));
    }
}

/// A generated delete doesn't clear the junction rows of the record it deletes.
#[tokio::test]
async fn deleting_a_record_with_many_to_many_rows_depends_on_the_backend() {
    let state = open().await;
    let store = state.store().await.unwrap();
    session(store, "s1", &["heavy"]).await;
    let deleted = store.delete_session("s1").await;
    println!("delete session with tags: {deleted:?}");
    if ENFORCES_REFERENCES {
        assert!(matches!(deleted, Err(AppError::DbError(_))), "{deleted:?}");
    } else {
        assert!(deleted.is_ok());
    }
}

/// The same 20k sets through the same code on each backend. Ignored by default.
/// Run: cargo test --release -p iron-log-<backend> --test scenario -- --ignored --nocapture perf
#[tokio::test(flavor = "multi_thread")]
#[ignore = "writes tens of thousands of records"]
async fn perf_twenty_thousand_sets() {
    use std::time::Instant;
    const SETS: usize = 20_000;
    let state = open().await;
    let store = state.store().await.unwrap();
    session(store, "s0", &["heavy"]).await;

    let started = Instant::now();
    for i in 0..SETS {
        let exercise = ["bench-press", "back-squat", "deadlift"][i % 3];
        let session = (i < 20).then_some("s0");
        let mut input = set(&format!("e{i:06}"), session, exercise, (i % 60) as i32, Intent::Working, 80_000, 5);
        input.started_at = format!("2026-{:02}-{:02}T10:00:00Z", i % 12 + 1, i % 28 + 1);
        training::log_set(store, input).await.unwrap();
    }
    println!("log_set x{SETS}: {:?} ({:?} each)", started.elapsed(), started.elapsed() / SETS as u32);

    let timed = |label: &'static str, started: Instant| println!("{label}: {:?}", started.elapsed());
    let t = Instant::now();
    store.get_effort("e010000").await.unwrap();
    timed("get_effort", t);
    let t = Instant::now();
    store.list_efforts(&[OrderBy::desc(EffortSortField::StartedAt)], Some(50), None).await.unwrap();
    timed("list_efforts newest 50", t);
    let t = Instant::now();
    store.count_efforts().await.unwrap();
    timed("count_efforts", t);
    let t = Instant::now();
    assert_eq!(training::session_detail(store, "s0".into()).await.unwrap().sets.len(), 20);
    timed("session_detail (20 of the sets)", t);
    let t = Instant::now();
    training::volume_by_muscle(store, PERSON_ID.into(), "2026-06".into()).await.unwrap();
    timed("volume_by_muscle", t);
    let t = Instant::now();
    training::best_sets(store, PERSON_ID.into()).await.unwrap();
    timed("best_sets", t);
    super::native_rollup(store).await;
}
