//! Schema parsing - extracts `EntityDef` metadata from `#[ontology(...)]` annotations.
//!
//! This is always the starting point of the pipeline.

pub mod model;
pub mod parse;
pub(crate) mod sort;

// Re-export the key types at the schema module level
pub use model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};

/// A schema of `entities` with no enums, for tests that build entities by
/// hand.
#[cfg(test)]
pub(crate) fn schema_of(entities: &[EntityDef]) -> crate::ir::SchemaOutput {
    crate::ir::SchemaOutput { entities: entities.to_vec(), enums: Vec::new() }
}

/// Entities named like what the generated code binds and imports: `Doc`
/// (the markdown store's vault document), `Order` (SeaORM's
/// `sea_query::Order`), `Match` (snake-cased to a keyword), with keyword
/// relationships `r#in` (a self-referential `has_many` foreign key) and
/// `r#loop` (a `many_to_many`).
#[cfg(test)]
pub(crate) fn hostile_entities() -> Vec<EntityDef> {
    const SOURCE: &str = r#"
        #[derive(OntologyEntity)]
        #[ontology(entity, directory = "docs", table = "docs")]
        pub struct Doc {
            #[ontology(id)]
            pub id: String,
            pub title: String,
            #[ontology(relation(belongs_to, target = "Doc"))]
            pub r#in: Option<String>,
            #[ontology(relation(has_many, target = "Doc", foreign_key = "in"))]
            pub children: Vec<String>,
            #[ontology(body)]
            pub body: String,
        }

        #[derive(OntologyEntity)]
        #[ontology(entity, directory = "orders", table = "orders")]
        pub struct Order {
            #[ontology(id)]
            pub id: String,
            pub title: String,
            pub weight: f64,
            #[ontology(relation(many_to_many, target = "Match"))]
            pub r#loop: Vec<String>,
        }

        #[derive(OntologyEntity)]
        #[ontology(entity, directory = "matches", table = "matches")]
        pub struct Match {
            #[ontology(id)]
            pub id: String,
            pub title: String,
        }
    "#;
    parse::parse_schema_source(SOURCE, std::path::Path::new("hostile.rs")).expect("hostile schema parses")
}
