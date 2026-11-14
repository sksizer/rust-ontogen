use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::{GroupKind, Intent, LoadKind};

/// The lifting facet of an effort. Its id IS the effort's id: a shared
/// primary key the schema has no way to declare, so nothing relates the two
/// but the convention.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "lifting", directory = "lifting")]
pub struct LiftingFacet {
    /// The effort this describes.
    #[ontology(id)]
    pub id: String,

    #[ontology(relation(belongs_to, target = "Exercise"))]
    pub exercise_id: String,

    #[ontology(enum_field)]
    pub intent: Intent,

    #[ontology(enum_field)]
    pub load_kind: LoadKind,

    /// Grams for a mass, never a float: kilos would accumulate rounding
    /// error across a year of estimated-max maths.
    #[serde(default)]
    pub load_grams: Option<i32>,

    /// Half steps are exact in binary, so 8.5 is really 8.5.
    #[serde(default)]
    pub rpe: Option<f64>,

    pub per_hand: bool,

    #[ontology(relation(belongs_to, target = "SetGroup"))]
    #[serde(default)]
    pub set_group_id: Option<String>,

    /// Denormalised from the group, so volume scans never join it.
    #[ontology(enum_field)]
    #[serde(default)]
    pub group_kind: Option<GroupKind>,
}
