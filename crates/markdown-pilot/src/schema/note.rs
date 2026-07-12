use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// The golden spec's minimal entity: id (filename stem), one plain field,
/// and a markdown body.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "notes", table = "notes")]
pub struct Note {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    #[ontology(body)]
    pub body: String,
}
