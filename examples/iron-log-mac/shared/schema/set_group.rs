use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::GroupKind;

/// A drop set, superset or cluster: what its legs share.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "set_groups", directory = "set_groups")]
pub struct SetGroup {
    #[ontology(id)]
    pub id: String,

    #[ontology(relation(belongs_to, target = "Session"))]
    #[serde(default)]
    pub session_id: Option<String>,

    #[ontology(enum_field)]
    pub kind: GroupKind,

    #[serde(default)]
    pub rounds: Option<i32>,
}
