use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Named like `std::path::Path`, which is not in the prelude: imported
/// from the schema like any other entity.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "paths", table = "paths")]
pub struct Path {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
