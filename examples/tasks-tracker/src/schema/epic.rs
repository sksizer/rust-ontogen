use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A multi-task capability slice. Which tasks belong to it is answered by
/// walking `tasks/` and filtering on their `epic_id` — never stored here
/// (cross-entity `has_many` is app-level on both backends today).
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "epics", table = "epics")]
pub struct Epic {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    pub status: String,

    #[ontology(body)]
    pub body: String,
}
