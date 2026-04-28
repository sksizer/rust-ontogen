use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "tags")]
pub struct Tag {
    #[ontology(id)]
    pub id: String,

    pub name: String,
}
