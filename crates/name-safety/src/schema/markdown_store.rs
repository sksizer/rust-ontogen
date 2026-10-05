use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Its module `markdown_store` shares the name of the store's runtime
/// crate, which the generated `open_vault` names beside it.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "markdown_stores", table = "markdown_stores")]
pub struct MarkdownStore {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
