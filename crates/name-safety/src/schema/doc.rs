use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "docs", table = "docs")]
pub struct Doc {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
