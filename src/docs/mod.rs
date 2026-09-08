//! The docs stage: a data-model reference and JSON Schema for the parsed schema.
//!
//! Schemas written as annotated Rust structs already carry the meaning of every
//! field in its doc comment. This stage publishes that: one markdown reference
//! ([`markdown`]) and one JSON Schema per entity plus an export-bundle schema
//! ([`json_schema`]), so a consumer's data format has a spec other tools can
//! read.
//!
//! The stage depends only on `parse_schema` — it reads entities, their fields
//! and relations, and the string enums beside them, and writes nothing any
//! other stage consumes.

mod json_schema;
mod markdown;

use std::fs;
use std::path::PathBuf;

use ontogen_core::CodegenError;
use ontogen_core::ir::SchemaOutput;
use ontogen_core::model::{EntityDef, FieldDef, FieldType};
use ontogen_core::naming::to_snake_case;
use ontogen_core::utils::write_if_changed;
use sha2::{Digest, Sha256};

/// Configuration for [`gen_docs`](crate::gen_docs).
pub struct DocsConfig {
    /// File the markdown reference is written to (e.g. `docs/data-model.md`).
    pub markdown_output: PathBuf,
    /// Directory the JSON Schema files are written to. Receives one
    /// `<entity_snake>.schema.json` per entity plus `export.schema.json`.
    pub json_schema_dir: PathBuf,
    /// Title of the reference — the `# ` heading of the markdown, and the stem
    /// of the export bundle's title.
    pub title: String,
    /// The `manifest.format` constant in `export.schema.json`: the consumer's
    /// own name for its export bundle (e.g. `determined-fitness-log`).
    pub export_format: String,
}

/// Write the data-model reference and the JSON Schema files.
///
/// # Errors
///
/// Returns [`CodegenError::Docs`] if an output directory cannot be created, a
/// file cannot be written, or a schema fails to serialise.
pub fn generate(schema: &SchemaOutput, config: &DocsConfig) -> Result<(), CodegenError> {
    let revision = schema_revision(&schema.entities);

    if let Some(parent) = config.markdown_output.parent() {
        create_dir(parent)?;
    }
    create_dir(&config.json_schema_dir)?;

    write(&config.markdown_output, &markdown::render(schema, config, &revision))?;

    for entity in &schema.entities {
        let path = config.json_schema_dir.join(format!("{}.schema.json", to_snake_case(&entity.name)));
        write(&path, &json_schema::entity_schema(entity, &schema.enums, &revision)?)?;
    }
    write(&config.json_schema_dir.join("export.schema.json"), &json_schema::export_schema(schema, config, &revision)?)?;

    Ok(())
}

fn create_dir(dir: &std::path::Path) -> Result<(), CodegenError> {
    fs::create_dir_all(dir).map_err(|e| CodegenError::Docs(format!("Failed to create {}: {e}", dir.display())))
}

fn write(path: &std::path::Path, content: &str) -> Result<(), CodegenError> {
    write_if_changed(path, content).map_err(|e| CodegenError::Docs(format!("Failed to write {}: {e}", path.display())))
}

/// A short digest of the schema's shape: the first 16 hex characters of the
/// SHA-256 of the sorted `<entity_snake>.<field>:<rust_type>` lines.
///
/// It changes when a field is added, removed, renamed, or retyped, and not
/// when a doc comment is reworded — so a consumer can tell a spec edit from a
/// data-format change.
fn schema_revision(entities: &[EntityDef]) -> String {
    let mut lines = Vec::new();
    for entity in entities {
        let snake = to_snake_case(&entity.name);
        for field in &entity.fields {
            lines.push(format!("{snake}.{}:{}", field.name, rust_type(field)));
        }
    }
    lines.sort();

    let digest = Sha256::digest(lines.join("\n").as_bytes());
    let hex = format!("{digest:x}");
    hex[..16].to_string()
}

/// The field's Rust type as written in the schema, e.g. `Option<String>`.
fn rust_type(field: &FieldDef) -> String {
    match &field.field_type {
        FieldType::String => "String".to_string(),
        FieldType::OptionString => "Option<String>".to_string(),
        FieldType::OptionEnum(name) => format!("Option<{name}>"),
        FieldType::VecString => "Vec<String>".to_string(),
        FieldType::VecStruct(name) => format!("Vec<{name}>"),
        FieldType::I32 => "i32".to_string(),
        FieldType::OptionI32 => "Option<i32>".to_string(),
        FieldType::I64 => "i64".to_string(),
        FieldType::OptionI64 => "Option<i64>".to_string(),
        FieldType::F32 => "f32".to_string(),
        FieldType::OptionF32 => "Option<f32>".to_string(),
        FieldType::F64 => "f64".to_string(),
        FieldType::OptionF64 => "Option<f64>".to_string(),
        FieldType::Bool => "bool".to_string(),
        FieldType::OptionBool => "Option<bool>".to_string(),
        FieldType::Other(name) => name.clone(),
    }
}

/// A field is required unless its Rust type is an `Option<...>`.
fn is_required(field: &FieldDef) -> bool {
    !rust_type(field).starts_with("Option<")
}

/// The entities in reference order: sorted by name, so the document does not
/// move when a schema file is renamed.
fn sorted_entities(entities: &[EntityDef]) -> Vec<&EntityDef> {
    let mut sorted: Vec<&EntityDef> = entities.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use ontogen_core::model::FieldRole;

    fn field(name: &str, field_type: FieldType) -> FieldDef {
        FieldDef::new(name, field_type, FieldRole::Plain)
    }

    #[test]
    fn optional_fields_are_not_required() {
        assert!(is_required(&field("id", FieldType::String)));
        assert!(!is_required(&field("note", FieldType::OptionString)));
        assert!(is_required(&field("reps", FieldType::I32)));
        assert!(!is_required(&field("weight", FieldType::OptionF64)));
    }

    #[test]
    fn revision_tracks_types_not_docs() {
        let entity = |doc: &str| EntityDef {
            name: "Set".to_string(),
            doc: String::new(),
            directory: "sets".to_string(),
            table: "sets".to_string(),
            type_name: "set".to_string(),
            prefix: "set".to_string(),
            fields: vec![FieldDef { doc: doc.to_string(), ..field("reps", FieldType::I32) }],
        };

        let plain = schema_revision(&[entity("")]);
        assert_eq!(plain.len(), 16);
        assert_eq!(plain, schema_revision(&[entity("How many.")]));

        let retyped = EntityDef { fields: vec![field("reps", FieldType::OptionI32)], ..entity("") };
        assert_ne!(plain, schema_revision(&[retyped]));
    }
}
