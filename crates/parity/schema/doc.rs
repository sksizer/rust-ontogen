use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Named like the markdown store's own `doc` binding, with a
/// self-referential tree whose foreign key is a keyword: `r#in` is the
/// column `in`, which SQL does not take bare either.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "docs", table = "docs")]
pub struct Doc {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    #[ontology(relation(belongs_to, target = "Doc"))]
    pub r#in: Option<String>,

    #[ontology(relation(has_many, target = "Doc", foreign_key = "r#in"))]
    pub children: Vec<String>,

    #[ontology(body)]
    pub body: String,
}
