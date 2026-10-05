use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "maps", table = "maps")]
pub struct Map {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    /// A self-referential tree whose foreign key is a keyword: `r#in`.
    #[ontology(relation(belongs_to, target = "Map"))]
    pub r#in: Option<String>,

    #[ontology(relation(has_many, target = "Map", foreign_key = "r#in"))]
    pub children: Vec<String>,
}
