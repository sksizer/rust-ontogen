use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "notes", table = "notes")]
pub struct Note {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    #[ontology(body)]
    pub body: String,
}
