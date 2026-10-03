use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// The `many_to_many` target of `Item`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "tags", table = "tags")]
pub struct Tag {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
