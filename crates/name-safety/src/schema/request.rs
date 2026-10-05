use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "requests", table = "requests")]
pub struct Request {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
