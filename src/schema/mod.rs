//! Schema parsing - extracts `EntityDef` metadata from `#[ontology(...)]` annotations.
//!
//! This is always the starting point of the pipeline.

pub mod model;
pub mod parse;
// The store, servers and clients stages read it once they emit sort code.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod sort;

// Re-export the key types at the schema module level
pub use model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};

/// A schema of `entities` with no enums, for tests that build entities by
/// hand.
#[cfg(test)]
pub(crate) fn schema_of(entities: &[EntityDef]) -> crate::ir::SchemaOutput {
    crate::ir::SchemaOutput { entities: entities.to_vec(), enums: Vec::new() }
}
