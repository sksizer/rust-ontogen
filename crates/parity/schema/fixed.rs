use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// The entity that overrides the stores' `SlugFromField("title")` with
/// `IdStrategy::Provided`: a create without an id has nothing to derive one
/// from, though it has a title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "fixed", table = "fixed", id = "provided")]
pub struct Fixed {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
