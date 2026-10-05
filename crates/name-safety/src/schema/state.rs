use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "states", table = "states")]
pub struct State {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
