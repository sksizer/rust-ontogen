//! The ops CRUD can't express: a write spanning three entities, and reads that
//! join or aggregate them. They use only the generated store, so one source
//! serves both backends. That is also why every read here loads whole tables
//! and joins in memory: a generated `list_*` takes an order, a limit and an
//! offset, and no filter.

use std::collections::HashMap;

use ontogen_core::order::OrderBy;

use crate::domain::catalog;
use crate::schema::{
    AppError, BestSet, Effort, Intent, LiftingFacet, LoadKind, LogSetInput, MuscleVolume, RepsFacet, SessionDetail,
    SetLine,
};
use crate::store::Store;
use crate::store::effort::EffortSortField;
use crate::store::session::SessionUpdate;

/// Write the starting catalog into an empty log.
pub async fn seed_catalog(store: &Store, now: String) -> Result<(), AppError> {
    catalog::seed(store, &now).await
}

/// Log one set: an effort and its lifting and reps facets, sharing an id.
///
/// There is no transaction across entities, so a failure part-way undoes the
/// earlier writes by hand. A crash between writes still leaves them behind.
pub async fn log_set(store: &Store, input: LogSetInput) -> Result<SetLine, AppError> {
    if input.reps <= 0 {
        return Err(AppError::InvalidLog(format!("reps must be positive, got {}", input.reps)));
    }
    if let Some(rpe) = input.rpe
        && !(0.0..=10.0).contains(&rpe)
    {
        return Err(AppError::InvalidLog(format!("rpe must be 0 to 10, got {rpe}")));
    }
    // Only the SQLite backend has foreign keys, so this is checked here for both.
    let exercise = store.get_exercise(&input.exercise_id).await?;

    let id = input.id.clone();
    store
        .create_effort(Effort {
            id: id.clone(),
            person_id: input.person_id,
            session_id: input.session_id,
            position: input.position,
            kind_id: input.kind_id,
            started_at: input.started_at.clone(),
            duration_secs: None,
            bodyweight_grams: None,
            labels: vec![],
            notes: None,
        })
        .await?;
    let lifting = LiftingFacet {
        id: id.clone(),
        exercise_id: input.exercise_id.clone(),
        intent: input.intent,
        load_kind: if input.load_grams.is_some() { LoadKind::Mass } else { LoadKind::None },
        load_grams: input.load_grams,
        rpe: input.rpe,
        per_hand: false,
        set_group_id: None,
        group_kind: None,
    };
    if let Err(e) = store.create_lifting_facet(lifting).await {
        store.delete_effort(&id).await?;
        return Err(e);
    }
    if let Err(e) = store.create_reps_facet(RepsFacet { id: id.clone(), reps: input.reps }).await {
        store.delete_lifting_facet(&id).await?;
        store.delete_effort(&id).await?;
        return Err(e);
    }

    Ok(SetLine {
        effort_id: id,
        exercise_id: input.exercise_id,
        exercise_name: exercise.name,
        position: input.position,
        started_at: input.started_at,
        intent: input.intent,
        load_grams: input.load_grams,
        reps: input.reps,
        rpe: input.rpe,
        e1rm_grams: estimated_max(input.load_grams, input.reps),
    })
}

/// A session with its location, tags, notes and sets in logged order.
pub async fn session_detail(store: &Store, id: String) -> Result<SessionDetail, AppError> {
    let session = store.get_session(&id).await?;
    let location_name = match &session.location_id {
        Some(location_id) => Some(store.get_location(location_id).await?.name),
        None => None,
    };
    let mut tag_names = Vec::with_capacity(session.tags.len());
    for tag_id in &session.tags {
        tag_names.push(store.get_tag(tag_id).await?.name);
    }

    let order = [OrderBy::asc(EffortSortField::Position), OrderBy::asc(EffortSortField::StartedAt)];
    let efforts = store.list_efforts(&order, None, None).await?;
    let held: Vec<Effort> = efforts.into_iter().filter(|e| e.session_id.as_deref() == Some(id.as_str())).collect();
    let sets = set_lines(store, held).await?;

    let notes = store.list_notes(&[], None, None).await?;
    let notes = notes.into_iter().filter(|n| n.subject_id == id).collect();
    Ok(SessionDetail { session, location_name, tag_names, sets, notes })
}

/// A session and everything that hangs off it, children first.
pub async fn delete_session_cascade(store: &Store, id: String) -> Result<(), AppError> {
    store.get_session(&id).await?;
    let efforts = store.list_efforts(&[], None, None).await?;
    for effort in efforts.iter().filter(|e| e.session_id.as_deref() == Some(id.as_str())) {
        store.delete_lifting_facet(&effort.id).await?;
        store.delete_reps_facet(&effort.id).await?;
        store.delete_effort(&effort.id).await?;
    }
    for group in store.list_set_groups(&[], None, None).await? {
        if group.session_id.as_deref() == Some(id.as_str()) {
            store.delete_set_group(&group.id).await?;
        }
    }
    for note in store.list_notes(&[], None, None).await? {
        if note.subject_id == id {
            store.delete_note(&note.id).await?;
        }
    }
    // The generated delete leaves its own junction rows to the caller, and on
    // SQLite those rows then refuse the delete.
    store.update_session(&id, SessionUpdate { tags: Some(vec![]), ..Default::default() }).await?;
    store.delete_session(&id).await
}

/// Work per muscle since `since`, largest first. Warmups don't count.
pub async fn volume_by_muscle(store: &Store, person_id: String, since: String) -> Result<Vec<MuscleVolume>, AppError> {
    let efforts = store.list_efforts(&[], None, None).await?;
    let lifting: HashMap<String, LiftingFacet> =
        store.list_lifting_facets(&[], None, None).await?.into_iter().map(|f| (f.id.clone(), f)).collect();
    let reps: HashMap<String, i32> =
        store.list_reps_facets(&[], None, None).await?.into_iter().map(|f| (f.id, f.reps)).collect();
    let mut shares: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for row in store.list_exercise_muscles(&[], None, None).await? {
        shares.entry(row.exercise_id).or_default().push((row.muscle_id, row.contribution));
    }

    let mut totals: HashMap<String, (f64, i32)> = HashMap::new();
    for effort in efforts.iter().filter(|e| e.person_id == person_id && e.started_at >= since) {
        let Some(facet) = lifting.get(&effort.id) else { continue };
        let (Some(grams), Some(count)) = (facet.load_grams, reps.get(&effort.id)) else { continue };
        if facet.intent == Intent::Warmup {
            continue;
        }
        let kilos = f64::from(grams) / 1000.0 * f64::from(*count);
        for (muscle_id, share) in shares.get(&facet.exercise_id).into_iter().flatten() {
            let total = totals.entry(muscle_id.clone()).or_default();
            total.0 += kilos * share;
            total.1 += 1;
        }
    }

    let names: HashMap<String, String> =
        store.list_muscles(&[], None, None).await?.into_iter().map(|m| (m.id, m.name)).collect();
    let mut volumes: Vec<MuscleVolume> = totals
        .into_iter()
        .map(|(muscle_id, (volume_kg, sets))| MuscleVolume {
            muscle_name: names.get(&muscle_id).cloned().unwrap_or_else(|| muscle_id.clone()),
            muscle_id,
            volume_kg,
            sets,
        })
        .collect();
    volumes.sort_by(|a, b| b.volume_kg.total_cmp(&a.volume_kg).then_with(|| a.muscle_id.cmp(&b.muscle_id)));
    Ok(volumes)
}

/// Each exercise's strongest set for one person, strongest first. Warmups don't
/// count as records.
pub async fn best_sets(store: &Store, person_id: String) -> Result<Vec<BestSet>, AppError> {
    let efforts = store.list_efforts(&[], None, None).await?;
    let efforts: Vec<Effort> = efforts.into_iter().filter(|e| e.person_id == person_id).collect();
    let mut best: HashMap<String, BestSet> = HashMap::new();
    for line in set_lines(store, efforts).await? {
        let (Some(load_grams), Some(e1rm_grams)) = (line.load_grams, line.e1rm_grams) else { continue };
        if line.intent == Intent::Warmup {
            continue;
        }
        let candidate = BestSet {
            exercise_id: line.exercise_id.clone(),
            exercise_name: line.exercise_name,
            load_grams,
            reps: line.reps,
            e1rm_grams,
            achieved_at: line.started_at,
        };
        best.entry(line.exercise_id)
            .and_modify(|current| {
                if candidate.e1rm_grams > current.e1rm_grams {
                    *current = candidate.clone();
                }
            })
            .or_insert(candidate);
    }
    let mut sets: Vec<BestSet> = best.into_values().collect();
    sets.sort_by(|a, b| b.e1rm_grams.cmp(&a.e1rm_grams).then_with(|| a.exercise_id.cmp(&b.exercise_id)));
    Ok(sets)
}

/// Join efforts to their facets and exercises, keeping the efforts' order. An
/// effort without a lifting facet isn't a set and is left out.
async fn set_lines(store: &Store, efforts: Vec<Effort>) -> Result<Vec<SetLine>, AppError> {
    let lifting: HashMap<String, LiftingFacet> =
        store.list_lifting_facets(&[], None, None).await?.into_iter().map(|f| (f.id.clone(), f)).collect();
    let reps: HashMap<String, i32> =
        store.list_reps_facets(&[], None, None).await?.into_iter().map(|f| (f.id, f.reps)).collect();
    let names: HashMap<String, String> =
        store.list_exercises(&[], None, None).await?.into_iter().map(|e| (e.id, e.name)).collect();

    Ok(efforts
        .into_iter()
        .filter_map(|effort| {
            let facet = lifting.get(&effort.id)?;
            let count = reps.get(&effort.id).copied().unwrap_or(0);
            Some(SetLine {
                exercise_id: facet.exercise_id.clone(),
                exercise_name: names.get(&facet.exercise_id).cloned().unwrap_or_else(|| facet.exercise_id.clone()),
                position: effort.position,
                started_at: effort.started_at,
                intent: facet.intent,
                load_grams: facet.load_grams,
                reps: count,
                rpe: facet.rpe,
                e1rm_grams: estimated_max(facet.load_grams, count),
                effort_id: effort.id,
            })
        })
        .collect())
}

/// Epley: load × (1 + reps / 30). A single is its own max.
fn estimated_max(load_grams: Option<i32>, reps: i32) -> Option<i32> {
    let load = load_grams.filter(|g| *g > 0)?;
    if reps <= 0 {
        return None;
    }
    if reps == 1 {
        return Some(load);
    }
    Some((f64::from(load) * (1.0 + f64::from(reps) / 30.0)).round() as i32)
}
