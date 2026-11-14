//! The starting catalog: what a fresh log knows before anyone trains. ontogen
//! has no seed stage, so this goes through the generated store like any write.

use crate::schema::{
    ActivityKind, AppError, BodyRegion, Cue, Equipment, EquipmentClass, Exercise, ExerciseFamily, ExerciseMuscle,
    Location, Mechanic, Modality, Muscle, MuscleGroup, MuscleRole, Person, Tag, UnitPreference,
};
use crate::store::Store;

pub const PERSON_ID: &str = "me";
pub const STRENGTH_KIND_ID: &str = "strength";

fn muscle(id: &str, name: &str, region: BodyRegion, parent: Option<&str>, groups: &[&str]) -> Muscle {
    Muscle {
        id: id.into(),
        name: name.into(),
        region,
        parent_id: parent.map(Into::into),
        heads: vec![],
        groups: groups.iter().map(|g| (*g).to_string()).collect(),
    }
}

struct Lift {
    id: &'static str,
    name: &'static str,
    family: &'static str,
    equipment: &'static str,
    group: &'static str,
    mechanic: Mechanic,
    cue: &'static str,
    /// Muscle, role, share of the work.
    muscles: &'static [(&'static str, MuscleRole, f64)],
}

const LIFTS: &[Lift] = &[
    Lift {
        id: "bench-press",
        name: "Bench Press",
        family: "press",
        equipment: "barbell",
        group: "chest",
        mechanic: Mechanic::Compound,
        cue: "Shoulder blades pinned",
        muscles: &[
            ("pec-major", MuscleRole::Primary, 0.6),
            ("triceps", MuscleRole::Secondary, 0.3),
            ("delts", MuscleRole::Secondary, 0.1),
        ],
    },
    Lift {
        id: "overhead-press",
        name: "Overhead Press",
        family: "press",
        equipment: "barbell",
        group: "shoulders",
        mechanic: Mechanic::Compound,
        cue: "Ribs down",
        muscles: &[("delts", MuscleRole::Primary, 0.6), ("triceps", MuscleRole::Secondary, 0.4)],
    },
    Lift {
        id: "back-squat",
        name: "Back Squat",
        family: "squat",
        equipment: "barbell",
        group: "legs",
        mechanic: Mechanic::Compound,
        cue: "Knees track the toes",
        muscles: &[("quads", MuscleRole::Primary, 0.6), ("glutes", MuscleRole::Secondary, 0.4)],
    },
    Lift {
        id: "deadlift",
        name: "Deadlift",
        family: "hinge",
        equipment: "barbell",
        group: "legs",
        mechanic: Mechanic::Compound,
        cue: "Bar over midfoot",
        muscles: &[
            ("hamstrings", MuscleRole::Primary, 0.4),
            ("glutes", MuscleRole::Primary, 0.4),
            ("lats", MuscleRole::Stabilizer, 0.2),
        ],
    },
    Lift {
        id: "pull-up",
        name: "Pull-up",
        family: "pull",
        equipment: "pull-up-bar",
        group: "back",
        mechanic: Mechanic::Compound,
        cue: "Chest to the bar",
        muscles: &[("lats", MuscleRole::Primary, 0.7), ("biceps", MuscleRole::Secondary, 0.3)],
    },
    Lift {
        id: "barbell-curl",
        name: "Barbell Curl",
        family: "curl",
        equipment: "barbell",
        group: "arms",
        mechanic: Mechanic::Isolation,
        cue: "Elbows still",
        muscles: &[("biceps", MuscleRole::Primary, 1.0)],
    },
];

/// Write the catalog into an empty log. A log that has its person is left as is.
pub async fn seed(store: &Store, now: &str) -> Result<(), AppError> {
    match store.get_person(PERSON_ID).await {
        Ok(_) => return Ok(()),
        Err(AppError::PersonNotFound(_)) => {}
        Err(e) => return Err(e),
    }

    for (id, name, modality) in
        [(STRENGTH_KIND_ID, "Strength", Modality::Strength), ("cardio", "Cardio", Modality::Cardio)]
    {
        store.create_activity_kind(ActivityKind { id: id.into(), name: name.into(), modality }).await?;
    }
    for (id, name) in [
        ("chest", "Chest"),
        ("shoulders", "Shoulders"),
        ("arms", "Arms"),
        ("back", "Back"),
        ("legs", "Legs"),
        ("push", "Push"),
        ("pull", "Pull"),
    ] {
        store.create_muscle_group(MuscleGroup { id: id.into(), name: name.into() }).await?;
    }
    for m in [
        muscle("pec-major", "Pectoralis Major", BodyRegion::Upper, None, &["chest", "push"]),
        muscle("pec-clavicular", "Clavicular Head", BodyRegion::Upper, Some("pec-major"), &["chest", "push"]),
        muscle("pec-sternal", "Sternal Head", BodyRegion::Upper, Some("pec-major"), &["chest", "push"]),
        muscle("delts", "Deltoids", BodyRegion::Upper, None, &["shoulders", "push"]),
        muscle("triceps", "Triceps", BodyRegion::Upper, None, &["arms", "push"]),
        muscle("biceps", "Biceps", BodyRegion::Upper, None, &["arms", "pull"]),
        muscle("lats", "Latissimus Dorsi", BodyRegion::Upper, None, &["back", "pull"]),
        muscle("quads", "Quadriceps", BodyRegion::Lower, None, &["legs"]),
        muscle("hamstrings", "Hamstrings", BodyRegion::Lower, None, &["legs"]),
        muscle("glutes", "Glutes", BodyRegion::Lower, None, &["legs"]),
    ] {
        store.create_muscle(m).await?;
    }
    for (id, name, class) in
        [("barbell", "Barbell", EquipmentClass::FreeWeight), ("pull-up-bar", "Pull-up Bar", EquipmentClass::Bodyweight)]
    {
        store.create_equipment(Equipment { id: id.into(), name: name.into(), class }).await?;
    }
    for (id, name) in [("press", "Press"), ("squat", "Squat"), ("hinge", "Hinge"), ("pull", "Pull"), ("curl", "Curl")] {
        store.create_exercise_family(ExerciseFamily { id: id.into(), name: name.into() }).await?;
    }
    store
        .create_person(Person {
            id: PERSON_ID.into(),
            display_name: "Me".into(),
            unit_preference: UnitPreference::Kg,
            created_at: now.into(),
        })
        .await?;
    for lift in LIFTS {
        store
            .create_exercise(Exercise {
                id: lift.id.into(),
                name: lift.name.into(),
                family_id: Some(lift.family.into()),
                equipment_id: Some(lift.equipment.into()),
                primary_group_id: lift.group.into(),
                person_id: None,
                aka: vec![],
                cues: vec![Cue { text: lift.cue.into(), phase: None }],
                mechanic: Some(lift.mechanic),
                bodyweight_factor: None,
                unilateral: false,
                description: String::new(),
            })
            .await?;
        for (muscle_id, role, contribution) in lift.muscles {
            store
                .create_exercise_muscle(ExerciseMuscle {
                    id: format!("{}--{muscle_id}", lift.id),
                    exercise_id: lift.id.into(),
                    muscle_id: (*muscle_id).into(),
                    role: *role,
                    contribution: *contribution,
                })
                .await?;
        }
    }
    store
        .create_location(Location {
            id: "home-gym".into(),
            person_id: PERSON_ID.into(),
            name: "Home Gym".into(),
            latitude: None,
            longitude: None,
        })
        .await?;
    for (id, name) in [("heavy", "Heavy"), ("deload", "Deload")] {
        store.create_tag(Tag { id: id.into(), name: name.into() }).await?;
    }
    Ok(())
}
