use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "methods", table = "methods")]
pub struct Method {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
