use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A named set of muscles, the target of the many-to-many on `Muscle`.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "muscle_groups", directory = "muscle_groups")]
pub struct MuscleGroup {
    #[ontology(id)]
    pub id: String,

    pub name: String,
}
