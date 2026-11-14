use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A container a person opens and closes. It measures nothing of its own:
/// everything that measured the work is on the efforts it holds.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "sessions", directory = "sessions")]
pub struct Session {
    #[ontology(id)]
    pub id: String,

    #[ontology(relation(belongs_to, target = "Person"))]
    pub person_id: String,

    #[ontology(relation(belongs_to, target = "ActivityKind"))]
    pub activity_kind_id: String,

    #[ontology(relation(belongs_to, target = "Location"))]
    #[serde(default)]
    pub location_id: Option<String>,

    #[serde(default)]
    pub title: Option<String>,

    /// ISO8601 UTC.
    pub started_at: String,

    /// `None` while the session is live.
    #[serde(default)]
    pub ended_at: Option<String>,

    #[ontology(relation(many_to_many, target = "Tag"))]
    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    pub notes: Option<String>,
}
