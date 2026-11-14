use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// The distance facet: a run, a row, a farmer's carry.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "distance", directory = "distance")]
pub struct DistanceFacet {
    /// The effort this describes.
    #[ontology(id)]
    pub id: String,

    pub metres: f64,

    #[ontology(relation(belongs_to, target = "Location"))]
    #[serde(default)]
    pub location_id: Option<String>,
}
