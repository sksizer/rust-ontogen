use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// An outline section whose parent is required: every section has one, and
/// a root section is its own parent. Its `has_many` therefore runs over a
/// non-`Option` foreign key, so an update that drops a child is refused
/// (`SectionParentRequired`) instead of orphaning it.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "sections", table = "sections")]
pub struct Section {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    #[ontology(relation(belongs_to, target = "Section"))]
    pub parent_id: String,

    #[ontology(relation(has_many, target = "Section", foreign_key = "parent_id"))]
    pub children: Vec<String>,
}
