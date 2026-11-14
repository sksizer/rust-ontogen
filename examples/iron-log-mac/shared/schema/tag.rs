use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A label on sessions, the target of their many-to-many.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "tags", directory = "tags")]
pub struct Tag {
    #[ontology(id)]
    pub id: String,

    pub name: String,
}
