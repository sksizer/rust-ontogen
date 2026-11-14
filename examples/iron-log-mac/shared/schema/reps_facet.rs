use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// The reps facet: a hold has none, rather than a null count.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "reps", directory = "reps")]
pub struct RepsFacet {
    /// The effort this describes.
    #[ontology(id)]
    pub id: String,

    pub reps: i32,
}
