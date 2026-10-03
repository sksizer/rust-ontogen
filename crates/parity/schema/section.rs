use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A self-referential `has_many` over a non-`Option` foreign key: a child
/// cannot be dropped (`SectionParentRequired`). A root is its own parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
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
