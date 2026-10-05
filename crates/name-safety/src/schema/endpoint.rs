use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "endpoints", table = "endpoints")]
pub struct Endpoint {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
