use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "workout_sets")]
pub struct WorkoutSet {
    #[ontology(id)]
    pub id: String,

    #[ontology(relation(belongs_to, target = "Workout"))]
    pub workout_id: String,

    #[ontology(relation(belongs_to, target = "Exercise"))]
    pub exercise_id: String,

    pub set_number: i32,

    /// Weight in kilograms (stored as grams for integer precision)
    pub weight_grams: i32,

    pub reps: i32,

    /// Rate of perceived exertion (1-10), optional
    #[serde(default)]
    pub rpe: Option<i32>,

    #[serde(default)]
    pub notes: Option<String>,
}
