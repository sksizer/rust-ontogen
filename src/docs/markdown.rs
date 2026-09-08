//! The `data-model.md` reference: one section per entity, plus a Mermaid ER
//! diagram over the `belongs_to` relations.

use ontogen_core::ir::SchemaOutput;
use ontogen_core::model::{EntityDef, EnumDef, FieldDef, FieldType};

use crate::docs::{DocsConfig, is_required, sorted_entities};

/// Render the whole reference. Blocks are joined by a blank line and the
/// document ends with a newline.
pub(super) fn render(schema: &SchemaOutput, config: &DocsConfig, revision: &str) -> String {
    let entities = sorted_entities(&schema.entities);

    let mut blocks = vec![
        format!("# {}", config.title),
        format!("Generated from the schema. Do not edit. Schema revision `{revision}`."),
    ];

    for entity in &entities {
        blocks.push(format!("## {}", entity.name));
        if !entity.doc.is_empty() {
            blocks.push(entity.doc.clone());
        }
        blocks.push(format!("Table: `{}`", entity.table));
        blocks.push(field_table(entity, &schema.enums));
        if let Some(relations) = relations(entity, &entities) {
            blocks.push(relations);
        }
    }

    blocks.push("## Entity relationship diagram".to_string());
    blocks.push(er_diagram(&entities));

    format!("{}\n", blocks.join("\n\n"))
}

/// The `field | type | required | doc` table, in field declaration order.
fn field_table(entity: &EntityDef, enums: &[EnumDef]) -> String {
    let mut rows = vec!["| field | type | required | doc |".to_string(), "|---|---|---|---|".to_string()];
    for field in &entity.fields {
        let required = if is_required(field) { "yes" } else { "no" };
        let doc = escape_pipes(&field.doc.replace('\n', " "));
        rows.push(format!("| {} | {} | {required} | {doc} |", field.name, field_type(field, enums)));
    }
    rows.join("\n")
}

/// A `|` inside a cell ends the cell, so escape it. Field docs list closed
/// sets as `a | b | c` often enough that an unescaped pipe would split rows.
fn escape_pipes(text: &str) -> String {
    text.replace('|', "\\|")
}

/// The type column's vocabulary: a wire type, not the Rust type. An enum
/// prints its values, since the closed set is the contract.
fn field_type(field: &FieldDef, enums: &[EnumDef]) -> String {
    if let Some(def) = field.enum_def(enums) {
        let values: Vec<String> = def.variants.iter().map(|v| escape_pipes(&v.value)).collect();
        return format!("enum({})", values.join(", "));
    }
    match &field.field_type {
        FieldType::VecString => "string[]",
        FieldType::VecStruct(_) => "object[]",
        FieldType::I32 | FieldType::OptionI32 | FieldType::I64 | FieldType::OptionI64 => "integer",
        FieldType::F32 | FieldType::OptionF32 | FieldType::F64 | FieldType::OptionF64 => "number",
        FieldType::Bool | FieldType::OptionBool => "boolean",
        // Everything else — including a named type with no enum declaration —
        // is a string on the wire.
        _ => "string",
    }
    .to_string()
}

/// The relations block, or `None` when the entity has no relation in either
/// direction.
fn relations(entity: &EntityDef, entities: &[&EntityDef]) -> Option<String> {
    let mut lines = Vec::new();

    for (field, info) in entity.belongs_to_relations() {
        lines.push(format!("- belongs to {} via `{}`", info.target, field.name));
    }
    for other in entities {
        for (field, info) in other.belongs_to_relations() {
            if info.target == entity.name {
                lines.push(format!("- referenced by {}.`{}`", other.name, field.name));
            }
        }
    }

    if lines.is_empty() {
        return None;
    }
    Some(format!("Relations:\n{}", lines.join("\n")))
}

/// One `erDiagram` line per `belongs_to`, sorted by (target, entity, field).
/// An entity with no relation does not appear.
fn er_diagram(entities: &[&EntityDef]) -> String {
    let mut edges: Vec<(String, String, String)> = Vec::new();
    for entity in entities {
        for (field, info) in entity.belongs_to_relations() {
            edges.push((info.target.clone(), entity.name.clone(), field.name.clone()));
        }
    }
    edges.sort();

    let mut lines = vec!["```mermaid".to_string(), "erDiagram".to_string()];
    for (target, entity, field) in edges {
        lines.push(format!("  {target} ||--o{{ {entity} : \"{field}\""));
    }
    lines.push("```".to_string());
    lines.join("\n")
}
