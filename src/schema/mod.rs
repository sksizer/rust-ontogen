//! Schema parsing — extracts `EntityDef` metadata from `#[ontology(...)]` annotations.
//!
//! This is always the starting point of the pipeline.

pub mod model;
pub mod parse;

// Re-export the key types at the schema module level
pub use model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};
