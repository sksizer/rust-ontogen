use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "responses", table = "responses")]
pub struct Response {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
