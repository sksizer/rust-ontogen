use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

use super::SubjectKind;

/// A note on anything: a polymorphic pointer, which no relation attribute can
/// express and no foreign key can check.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, table = "notes", directory = "notes")]
pub struct Note {
    #[ontology(id)]
    pub id: String,

    #[ontology(enum_field)]
    pub subject_kind: SubjectKind,

    pub subject_id: String,

    #[ontology(body)]
    #[serde(default)]
    pub body: String,

    pub created_at: String,
}
