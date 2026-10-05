use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Named like SeaORM's `sea_query::Order`, with a `many_to_many` whose
/// field is a keyword (`r#loop`) to an entity whose module is one
/// (`r#match`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "orders", table = "orders")]
pub struct Order {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    #[ontology(relation(many_to_many, target = "Match"))]
    pub r#loop: Vec<String>,
}
