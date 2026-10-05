use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "events", table = "events")]
pub struct Event {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
