use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::UnitPreference;

/// Someone whose training is logged. A household shares one log, so every
/// row of the log names its person.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "people", directory = "people")]
pub struct Person {
    #[ontology(id)]
    pub id: String,

    pub display_name: String,

    #[ontology(enum_field)]
    pub unit_preference: UnitPreference,

    /// ISO8601 UTC.
    pub created_at: String,
}
