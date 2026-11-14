use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::MuscleRole;

/// What an exercise loads and how much: a junction with attributes of its
/// own, so it is an entity rather than a `many_to_many`. The pair
/// (exercise, muscle) is unique in meaning, which the schema cannot state.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "exercise_muscles", directory = "exercise_muscles")]
pub struct ExerciseMuscle {
    #[ontology(id)]
    pub id: String,

    #[ontology(relation(belongs_to, target = "Exercise"))]
    pub exercise_id: String,

    #[ontology(relation(belongs_to, target = "Muscle"))]
    pub muscle_id: String,

    #[ontology(enum_field)]
    pub role: MuscleRole,

    /// 0..1, this muscle's share of the work: the weight in the volume sum.
    pub contribution: f64,
}
