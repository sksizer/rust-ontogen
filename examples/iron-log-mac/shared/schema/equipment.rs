use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::EquipmentClass;

/// A piece of equipment. `Equipment` is a mass noun: its table name is given
/// rather than pluralised.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "equipment", directory = "equipment")]
pub struct Equipment {
    #[ontology(id)]
    pub id: String,

    pub name: String,

    #[ontology(enum_field)]
    pub class: EquipmentClass,
}
