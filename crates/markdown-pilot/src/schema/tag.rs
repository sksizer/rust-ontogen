use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A tag. Its counters are `u64`, wider than the `i64` a store holds, so
/// the HTTP router tests show such a value refused before the store sees
/// it.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "tags", table = "tags")]
pub struct Tag {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    #[serde(default)]
    pub uses: u64,

    pub peak_uses: Option<u64>,
}
