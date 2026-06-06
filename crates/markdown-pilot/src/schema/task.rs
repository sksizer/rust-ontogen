use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Relation-complete entity: a `belongs_to` parent (wikilink FK in
/// frontmatter), a self-referential `has_many` (derived view, reverse walk),
/// an authoritative `many_to_many` wikilink list, and a markdown body.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "tasks", table = "tasks")]
pub struct Task {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    pub status: String,

    #[ontology(relation(belongs_to, target = "Task"))]
    pub parent_id: Option<String>,

    #[ontology(relation(has_many, target = "Task", foreign_key = "parent_id"))]
    pub subtasks: Vec<String>,

    #[ontology(relation(many_to_many, target = "Tag"))]
    pub tags: Vec<String>,

    #[ontology(body)]
    pub body: String,
}
