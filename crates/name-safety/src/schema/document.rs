use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// Relationships named after Rust keywords: `match_id` and `ref_id` become
/// the `match` and `ref` relationships, and `r#loop` keeps its raw ident.
/// `crate_id`, `self_id` and `super_id` become relationships named after
/// keywords that have no raw spelling.
#[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "documents", table = "documents")]
pub struct Document {
    #[ontology(id)]
    pub id: String,

    pub title: String,

    #[ontology(relation(belongs_to, target = "Doc"))]
    pub match_id: Option<String>,

    #[ontology(relation(belongs_to, target = "Map"))]
    pub ref_id: Option<String>,

    #[ontology(relation(belongs_to, target = "Doc"))]
    pub crate_id: Option<String>,

    #[ontology(relation(belongs_to, target = "Document"))]
    pub self_id: Option<String>,

    #[ontology(relation(belongs_to, target = "Map"))]
    pub super_id: Option<String>,

    #[ontology(relation(many_to_many, target = "Response"))]
    pub r#loop: Vec<String>,

    #[ontology(body)]
    pub body: String,
}
