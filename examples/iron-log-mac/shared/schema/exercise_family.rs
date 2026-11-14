use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Variations of one movement: every squat.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "exercise_families", directory = "exercise_families")]
pub struct ExerciseFamily {
    #[ontology(id)]
    pub id: String,

    pub name: String,
}
