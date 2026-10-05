use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "values", table = "values")]
pub struct Value {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
