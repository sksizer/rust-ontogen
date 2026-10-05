use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Its API module `axum` shares the name of the HTTP server's crate.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "axums", table = "axums")]
pub struct Axum {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
