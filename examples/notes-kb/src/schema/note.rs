use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A note whose outbound `links` are wikilinks to other notes — the
/// self-referential many-to-many that makes a vault a graph. Obsidian
/// renders the same files as that graph natively; the example's web view
/// draws it from the generated API.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "notes", table = "notes")]
pub struct Note {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    #[ontology(relation(many_to_many, target = "Note"))]
    pub links: Vec<String>,

    #[ontology(body)]
    pub body: String,
}
