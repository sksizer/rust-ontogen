//! The training log's schema, shared by the SQLite and markdown crates: the
//! entities, their closed vocabularies, and the error the generated stores
//! construct. A subset of `determined-fitness`, chosen for the shapes it
//! stresses.

mod activity_kind;
mod distance_facet;
mod effort;
mod equipment;
mod exercise;
mod exercise_family;
mod exercise_muscle;
mod lifting_facet;
mod location;
mod muscle;
mod muscle_group;
mod note;
mod person;
mod reps_facet;
mod session;
mod set_group;
mod tag;

pub mod dto;

pub use activity_kind::ActivityKind;
pub use distance_facet::DistanceFacet;
pub use effort::Effort;
pub use equipment::Equipment;
pub use exercise::Exercise;
pub use exercise_family::ExerciseFamily;
pub use exercise_muscle::ExerciseMuscle;
pub use lifting_facet::LiftingFacet;
pub use location::Location;
pub use muscle::Muscle;
pub use muscle_group::MuscleGroup;
pub use note::Note;
pub use person::Person;
pub use reps_facet::RepsFacet;
pub use session::Session;
pub use set_group::SetGroup;
pub use tag::Tag;

pub use dto::activity_kind::{CreateActivityKindInput, UpdateActivityKindInput};
pub use dto::distance_facet::{CreateDistanceFacetInput, UpdateDistanceFacetInput};
pub use dto::effort::{CreateEffortInput, UpdateEffortInput};
pub use dto::equipment::{CreateEquipmentInput, UpdateEquipmentInput};
pub use dto::exercise::{CreateExerciseInput, UpdateExerciseInput};
pub use dto::exercise_family::{CreateExerciseFamilyInput, UpdateExerciseFamilyInput};
pub use dto::exercise_muscle::{CreateExerciseMuscleInput, UpdateExerciseMuscleInput};
pub use dto::lifting_facet::{CreateLiftingFacetInput, UpdateLiftingFacetInput};
pub use dto::location::{CreateLocationInput, UpdateLocationInput};
pub use dto::muscle::{CreateMuscleInput, UpdateMuscleInput};
pub use dto::muscle_group::{CreateMuscleGroupInput, UpdateMuscleGroupInput};
pub use dto::note::{CreateNoteInput, UpdateNoteInput};
pub use dto::person::{CreatePersonInput, UpdatePersonInput};
pub use dto::reps_facet::{CreateRepsFacetInput, UpdateRepsFacetInput};
pub use dto::session::{CreateSessionInput, UpdateSessionInput};
pub use dto::set_group::{CreateSetGroupInput, UpdateSetGroupInput};
pub use dto::tag::{CreateTagInput, UpdateTagInput};

pub use crate::domain::views::*;
/// One store change, as the `entity_changes` feed carries it.
pub use crate::store::EntityChange;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum UnitPreference {
    Kg,
    Lb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum Modality {
    Strength,
    Cardio,
    Mobility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum BodyRegion {
    Upper,
    Lower,
    Core,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum EquipmentClass {
    FreeWeight,
    Machine,
    Bodyweight,
    Cable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum Mechanic {
    Compound,
    Isolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum MuscleRole {
    Primary,
    Secondary,
    Stabilizer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum GroupKind {
    Superset,
    DropSet,
    Cluster,
}

/// What a set was for. A warmup is not volume and is not a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum Intent {
    Warmup,
    Working,
    Attempt,
}

/// Each kind of load progresses within itself and never across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum LoadKind {
    None,
    Mass,
    Level,
    Label,
}

/// What a note's polymorphic pointer points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum SubjectKind {
    Exercise,
    Muscle,
    Session,
    Effort,
}

/// A coaching cue, held in a list on its exercise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, specta::Type)]
pub struct Cue {
    pub text: String,
    pub phase: Option<String>,
}

/// The variants the generated stores construct, one set per entity. `DbError`
/// is what the SQLite store reports and `Md` what the markdown one does, so
/// each crate leaves the other's unused.
#[derive(Debug)]
pub enum AppError {
    PersonNotFound(String),
    PersonIdRequired(String),
    PersonAlreadyExists(String),
    ActivityKindNotFound(String),
    ActivityKindIdRequired(String),
    ActivityKindAlreadyExists(String),
    MuscleNotFound(String),
    MuscleIdRequired(String),
    MuscleAlreadyExists(String),
    MuscleGroupNotFound(String),
    MuscleGroupIdRequired(String),
    MuscleGroupAlreadyExists(String),
    EquipmentNotFound(String),
    EquipmentIdRequired(String),
    EquipmentAlreadyExists(String),
    ExerciseFamilyNotFound(String),
    ExerciseFamilyIdRequired(String),
    ExerciseFamilyAlreadyExists(String),
    ExerciseNotFound(String),
    ExerciseIdRequired(String),
    ExerciseAlreadyExists(String),
    ExerciseMuscleNotFound(String),
    ExerciseMuscleIdRequired(String),
    ExerciseMuscleAlreadyExists(String),
    LocationNotFound(String),
    LocationIdRequired(String),
    LocationAlreadyExists(String),
    TagNotFound(String),
    TagIdRequired(String),
    TagAlreadyExists(String),
    SessionNotFound(String),
    SessionIdRequired(String),
    SessionAlreadyExists(String),
    SetGroupNotFound(String),
    SetGroupIdRequired(String),
    SetGroupAlreadyExists(String),
    EffortNotFound(String),
    EffortIdRequired(String),
    EffortAlreadyExists(String),
    LiftingFacetNotFound(String),
    LiftingFacetIdRequired(String),
    LiftingFacetAlreadyExists(String),
    RepsFacetNotFound(String),
    RepsFacetIdRequired(String),
    RepsFacetAlreadyExists(String),
    DistanceFacetNotFound(String),
    DistanceFacetIdRequired(String),
    DistanceFacetAlreadyExists(String),
    NoteNotFound(String),
    NoteIdRequired(String),
    NoteAlreadyExists(String),
    /// A self-referential `has_many` names a child that has another required
    /// parent. Muscle's parent is optional, so this is never raised here.
    MuscleParentRequired(String),
    /// A rule the hand-written domain enforces.
    InvalidLog(String),
    DbError(String),
    Md(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AppError {}

// ── Event types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ChangeOp {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone)]
pub enum EntityKind {
    Person,
    ActivityKind,
    Muscle,
    MuscleGroup,
    Equipment,
    ExerciseFamily,
    Exercise,
    ExerciseMuscle,
    Location,
    Tag,
    Session,
    SetGroup,
    Effort,
    LiftingFacet,
    RepsFacet,
    DistanceFacet,
    Note,
}
