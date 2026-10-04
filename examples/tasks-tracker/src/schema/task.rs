use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A planning task, frontmatter-shaped like this repo's own
/// `docs/planning/tasks/*.md` corpus: status, created date, an epic
/// reference, tags, and a markdown body of sections.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "tasks", table = "tasks")]
pub struct Task {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    /// e.g. `open/ready`, `in-progress`, `closed/done` — compound statuses
    /// are plain strings, exactly as the corpus writes them.
    pub status: String,

    pub created: String,

    #[ontology(relation(belongs_to, target = "Epic"))]
    pub epic_id: Option<String>,

    #[ontology(relation(many_to_many, target = "Tag"))]
    pub tags: Vec<String>,

    /// The task this one is a subtask of.
    #[ontology(relation(belongs_to, target = "Task"))]
    pub parent_id: Option<String>,

    /// The tasks whose `parent_id` is this one, read from them.
    #[ontology(relation(has_many, target = "Task", foreign_key = "parent_id"))]
    pub subtasks: Vec<String>,

    #[ontology(body)]
    pub body: String,
}
