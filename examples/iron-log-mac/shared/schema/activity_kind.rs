use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::Modality;

/// What kind of training a session or effort was: a catalog the person can
/// add to, with a closed modality the code branches on.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "activity_kinds", directory = "activity_kinds")]
pub struct ActivityKind {
    #[ontology(id)]
    pub id: String,

    pub name: String,

    #[ontology(enum_field)]
    pub modality: Modality,
}
