use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "linkss", table = "linkss")]
pub struct Links {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
