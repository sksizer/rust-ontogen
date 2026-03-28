use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "workouts")]
pub struct Workout {
    #[ontology(id)]
    pub id: String,

    #[serde(default)]
    pub name: Option<String>,

    /// ISO8601 date (e.g., "2026-03-28")
    pub date: String,

    #[serde(default)]
    pub duration_minutes: Option<i32>,

    #[serde(default)]
    pub notes: Option<String>,

    #[serde(default)]
    #[ontology(relation(many_to_many, target = "Tag"))]
    pub tags: Vec<String>,

    pub created_at: String,
}
