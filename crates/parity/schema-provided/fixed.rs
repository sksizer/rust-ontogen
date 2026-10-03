use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// The one entity of the `IdStrategy::Provided` stores: a create without an
/// id has nothing to derive one from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "fixed", table = "fixed")]
pub struct Fixed {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
