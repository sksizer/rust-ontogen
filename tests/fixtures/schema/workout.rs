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

    /// Self-referential parent (hidden from frontmatter rendering)
    #[serde(default)]
    #[ontology(relation(belongs_to, target = "Workout"))]
    pub parent_id: Option<String>,

    /// Multi-line many-to-many list, exercised by writer tests
    #[serde(default)]
    #[ontology(relation(many_to_many, target = "Tag"), multiline_list)]
    pub tags: Vec<String>,

    /// Body content for the markdown writer (#[ontology(body)])
    #[ontology(body)]
    pub notes: String,

    /// Field skipped from generated code but still rendered in writer
    #[serde(default)]
    #[ontology(skip)]
    pub local_only: Vec<String>,

    pub created_at: String,
}
