//! End-to-end CRUD against a temp vault using an iron-log-shaped entity
//! (`Workout`): create with a derived id, get, update via read-modify-write,
//! list with pagination and the cap guard, delete.
//!
//! This is a preview of exactly what generated markdown store code does per
//! operation — the call pattern here is the contract the ontogen markdown
//! backend emits against.
//!
//! Run with: `cargo run -p markdown-store --example vault_crud`

use markdown_store::{Document, Error, IdStrategy, VaultHandle, VaultLayout};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Workout {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_minutes: Option<i64>,
}

const WORKOUT_FIELDS: &[&str] = &["name", "date", "duration_minutes"];

fn main() -> Result<(), Error> {
    let dir = tempfile::tempdir().expect("tempdir");
    let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir, IdStrategy::SlugFromField("date".into()))
        .with_list_cap(100);

    // ── create ──────────────────────────────────────────────────────────
    // Id derivation + slug dedup + the write happen atomically under the
    // vault's write lock — the create path generated store code uses.
    let workout = Workout { name: Some("Leg day".into()), date: "2026-06-01".into(), duration_minutes: Some(45) };
    let mut doc = Document::new();
    doc.merge_serialize(&workout, WORKOUT_FIELDS)?;
    doc.set_body("Felt strong on squats.\n");
    let id = vault.create_record_derived("workouts", None, Some(&workout.date), &doc)?;
    println!("created workouts/{id}.md");

    // Creating with the same explicit id fails loudly — never a silent overwrite.
    assert!(matches!(vault.create_record_derived("workouts", Some(&id), None, &doc), Err(Error::AlreadyExists { .. })));

    // A second workout on the same date gets a deduped slug.
    let mut doc2 = Document::new();
    doc2.merge_serialize(&Workout { name: None, date: workout.date.clone(), duration_minutes: None }, WORKOUT_FIELDS)?;
    let id2 = vault.create_record_derived("workouts", None, Some(&workout.date), &doc2)?;
    println!("created workouts/{id2}.md (slug deduped)");
    assert_eq!(id2, format!("{id}-2"));

    // ── get ─────────────────────────────────────────────────────────────
    let doc = vault.read_record("workouts", &id)?;
    let read: Workout = doc.deserialize()?;
    assert_eq!(read.name.as_deref(), Some("Leg day"));
    assert_eq!(doc.body(), "Felt strong on squats.\n");

    // ── update (read-modify-write) ──────────────────────────────────────
    vault.modify_record("workouts", &id, |doc| {
        let mut w: Workout = doc.deserialize()?;
        w.duration_minutes = Some(60);
        doc.merge_serialize(&w, WORKOUT_FIELDS)
    })?;
    let read: Workout = vault.read_record("workouts", &id)?.deserialize()?;
    assert_eq!(read.duration_minutes, Some(60));
    println!("updated workouts/{id}.md in place");

    // ── list + pagination ───────────────────────────────────────────────
    for i in 0..8 {
        let extra = Workout { name: None, date: format!("2026-06-{:02}", 10 + i), duration_minutes: None };
        let mut d = Document::new();
        d.merge_serialize(&extra, WORKOUT_FIELDS)?;
        vault.create_record_derived("workouts", None, Some(&extra.date), &d)?;
    }
    let all = vault.read_all("workouts")?;
    println!("listed {} workouts (lexicographic by id)", all.len());
    assert_eq!(all.len(), 10);
    // Stable order: ids come back sorted.
    let ids: Vec<&str> = all.iter().map(|(id, _)| id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);

    // Pagination is plain skip/take over the stable order — exactly what
    // generated `list_*(limit, offset)` does.
    let page: Vec<_> = all.iter().skip(2).take(3).map(|(id, _)| id.clone()).collect();
    println!("page (offset=2, limit=3): {page:?}");
    assert_eq!(page.len(), 3);
    assert_eq!(page, ids[2..5].to_vec());

    // The cap guard: a vault grown past its configured comfort zone errors
    // loudly instead of grinding.
    let tiny = vault.clone().with_list_cap(3);
    assert!(matches!(tiny.read_all("workouts"), Err(Error::ListCapExceeded { count: 10, cap: 3, .. })));
    println!("cap guard fires as expected");

    // ── delete ──────────────────────────────────────────────────────────
    vault.remove_record("workouts", &id2)?;
    assert!(matches!(vault.read_record("workouts", &id2), Err(Error::NotFound { .. })));
    assert_eq!(vault.read_all("workouts")?.len(), 9);
    println!("deleted workouts/{id2}.md");

    println!("vault crud ok");
    Ok(())
}
