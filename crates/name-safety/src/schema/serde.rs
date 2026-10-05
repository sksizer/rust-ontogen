use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Its API module `serde` shares the name of a crate the generated
/// servers name, which they therefore name by `::`-rooted paths.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "serdes", table = "serdes")]
pub struct Serde {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
