use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// One thing a person did, at the finest grain they logged it: the spine.
/// What it measured lives in facet entities keyed by this id, and which
/// facets exist is the effort's shape.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "efforts", directory = "efforts")]
pub struct Effort {
    #[ontology(id)]
    pub id: String,

    #[ontology(relation(belongs_to, target = "Person"))]
    pub person_id: String,

    /// `None` is a loose effort nobody put in a session.
    #[ontology(relation(belongs_to, target = "Session"))]
    #[serde(default)]
    pub session_id: Option<String>,

    /// Order within the session: a backfilled 5x5 gives five efforts one
    /// timestamp.
    #[serde(default)]
    pub position: Option<i32>,

    #[ontology(relation(belongs_to, target = "ActivityKind"))]
    pub kind_id: String,

    /// ISO8601 UTC.
    pub started_at: String,

    #[serde(default)]
    pub duration_secs: Option<i32>,

    /// Body mass when logged, so a later weigh-in cannot change the work.
    #[serde(default)]
    pub bodyweight_grams: Option<i64>,

    /// Free words no rollup reads.
    #[serde(default)]
    pub labels: Vec<String>,

    #[serde(default)]
    pub notes: Option<String>,
}
