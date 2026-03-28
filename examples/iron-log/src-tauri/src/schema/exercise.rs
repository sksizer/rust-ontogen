use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "exercises")]
pub struct Exercise {
    #[ontology(id)]
    pub id: String,

    pub name: String,

    /// "chest" | "back" | "legs" | "shoulders" | "arms" | "core"
    pub muscle_group: String,

    /// "barbell" | "dumbbell" | "machine" | "bodyweight" | "cable"
    pub equipment: String,

    #[serde(default)]
    pub notes: Option<String>,
}
