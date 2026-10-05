use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "matches", table = "matches")]
pub struct Match {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
