use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::BodyRegion;

/// A muscle, or one head of one: a self-referential tree.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "muscles", directory = "muscles")]
pub struct Muscle {
    #[ontology(id)]
    pub id: String,

    pub name: String,

    #[ontology(enum_field)]
    pub region: BodyRegion,

    /// The muscle this is a head of.
    #[ontology(relation(belongs_to, target = "Muscle"))]
    #[serde(default)]
    pub parent_id: Option<String>,

    /// The heads of this muscle: the other side of `parent_id`.
    #[ontology(relation(has_many, target = "Muscle", foreign_key = "parent_id"))]
    #[serde(default)]
    pub heads: Vec<String>,

    /// Pectoralis major is in Chest and in Push.
    #[ontology(relation(many_to_many, target = "MuscleGroup", junction = "muscle_group_members"))]
    #[serde(default)]
    pub groups: Vec<String>,
}
