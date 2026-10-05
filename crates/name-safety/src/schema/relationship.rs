use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "relationships", table = "relationships")]
pub struct Relationship {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
