use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::{Cue, Mechanic};

/// A movement in the catalog: optional and required foreign keys, a string
/// list, a list of structs, a float, a bool, an optional enum and a body.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "exercises", directory = "exercises")]
pub struct Exercise {
    #[ontology(id)]
    pub id: String,

    pub name: String,

    #[ontology(relation(belongs_to, target = "ExerciseFamily"))]
    #[serde(default)]
    pub family_id: Option<String>,

    #[ontology(relation(belongs_to, target = "Equipment"))]
    #[serde(default)]
    pub equipment_id: Option<String>,

    #[ontology(relation(belongs_to, target = "MuscleGroup"))]
    pub primary_group_id: String,

    /// Who added it; `None` is the shared catalog.
    #[ontology(relation(belongs_to, target = "Person"))]
    #[serde(default)]
    pub person_id: Option<String>,

    /// Other names a person types.
    #[serde(default)]
    pub aka: Vec<String>,

    /// Coaching cues, a list of structs held on the row.
    #[serde(default)]
    pub cues: Vec<Cue>,

    #[ontology(enum_field)]
    #[serde(default)]
    pub mechanic: Option<Mechanic>,

    /// Share of body mass the movement carries, 0..1.
    #[serde(default)]
    pub bodyweight_factor: Option<f64>,

    pub unilateral: bool,

    /// How to do it, in prose.
    #[ontology(body)]
    #[serde(default)]
    pub description: String,
}
