use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Snake-cased to the keyword `match`: every module and path derived from
/// it is `r#match`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "matches", table = "matches")]
pub struct Match {
    #[ontology(id)]
    pub id: String,

    pub title: String,
}
