use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Its module is `sea_query`, the name of a crate the SeaORM store uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "sea_queries", table = "sea_queries")]
pub struct SeaQuery {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
