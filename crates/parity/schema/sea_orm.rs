use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Its module is `sea_orm`, the name of a crate the SeaORM store uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "sea_orms", table = "sea_orms")]
pub struct SeaOrm {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
