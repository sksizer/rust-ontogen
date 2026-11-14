use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Somewhere training happens.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "locations", directory = "locations")]
pub struct Location {
    #[ontology(id)]
    pub id: String,

    #[ontology(relation(belongs_to, target = "Person"))]
    pub person_id: String,

    pub name: String,

    #[serde(default)]
    pub latitude: Option<f64>,

    #[serde(default)]
    pub longitude: Option<f64>,
}
