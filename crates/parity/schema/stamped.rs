use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// The entity that overrides the stores' `SlugFromField("title")` with
/// `IdStrategy::Uuid`: a create without an id gets a fresh UUID v4, though
/// it has a title. Both stores need their crate's `uuid` feature for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "stamped", table = "stamped", id = "uuid")]
pub struct Stamped {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
