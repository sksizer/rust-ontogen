//! JSON Schema (draft 2020-12) for the parsed schema: one document per entity,
//! plus `export.schema.json` describing an export bundle of every table.

use ontogen_core::CodegenError;
use ontogen_core::ir::SchemaOutput;
use ontogen_core::model::{EntityDef, EnumDef, FieldDef, FieldType};
use ontogen_core::naming::to_snake_case;
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use crate::docs::{DocsConfig, is_required, sorted_entities};

const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

/// The JSON Schema for one entity: its fields as `properties`, the
/// non-optional ones as `required`.
pub(super) fn entity_schema(entity: &EntityDef, enums: &[EnumDef], revision: &str) -> Result<String, CodegenError> {
    let doc = EntitySchemaDoc {
        schema: DRAFT,
        id: format!("{}.schema.json", to_snake_case(&entity.name)),
        title: entity.name.clone(),
        description: entity.doc.clone(),
        revision: revision.to_string(),
        ty: "object",
        properties: OrderedMap(entity.fields.iter().map(|f| (f.name.clone(), property(f, enums))).collect()),
        required: entity.fields.iter().filter(|f| is_required(f)).map(|f| f.name.clone()).collect(),
        additional_properties: false,
    };
    render(&doc)
}

/// The JSON Schema for an export bundle: a `manifest` object and one array of
/// records per table, each item `$ref`-ing that entity's schema.
pub(super) fn export_schema(
    schema: &SchemaOutput,
    config: &DocsConfig,
    revision: &str,
) -> Result<String, CodegenError> {
    let tables = sorted_entities(&schema.entities)
        .into_iter()
        .map(|entity| {
            let items = Schema {
                ty: None,
                reference: Some(format!("{}.schema.json", to_snake_case(&entity.name))),
                ..Schema::default()
            };
            (entity.table.clone(), Schema { ty: Some("array"), items: Some(Box::new(items)), ..Schema::default() })
        })
        .collect();

    let doc = ExportSchemaDoc {
        schema: DRAFT,
        id: "export.schema.json".to_string(),
        title: format!("{} export bundle", config.title),
        revision: revision.to_string(),
        ty: "object",
        properties: OrderedMap(vec![
            ("manifest".to_string(), manifest_schema(&config.export_format)),
            (
                "tables".to_string(),
                Schema { ty: Some("object"), properties: Some(OrderedMap(tables)), ..Schema::default() },
            ),
        ]),
        required: vec!["manifest".to_string(), "tables".to_string()],
    };
    render(&doc)
}

/// The bundle manifest: what was exported, when, and against which schema
/// revision. The field names are the export format's contract, so they are
/// spelled out here rather than derived.
fn manifest_schema(export_format: &str) -> Schema {
    let string = || Schema { ty: Some("string"), ..Schema::default() };
    let map_of = |ty: &'static str| Schema {
        ty: Some("object"),
        additional_properties: Some(Box::new(Schema { ty: Some(ty), ..Schema::default() })),
        ..Schema::default()
    };

    Schema {
        ty: Some("object"),
        properties: Some(OrderedMap(vec![
            (
                "format".to_string(),
                Schema { ty: None, const_value: Some(export_format.to_string()), ..Schema::default() },
            ),
            ("format_version".to_string(), string()),
            ("schema_revision".to_string(), string()),
            ("exported_at".to_string(), string()),
            ("scope".to_string(), Schema { ty: Some("object"), ..Schema::default() }),
            ("counts".to_string(), map_of("integer")),
            ("files".to_string(), map_of("string")),
        ])),
        required: Some(
            ["format", "format_version", "schema_revision", "exported_at", "scope", "counts", "files"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        ),
        ..Schema::default()
    }
}

/// One field's schema. An enum field carries its closed set; a `Vec` becomes an
/// array of its item type; an `Option<T>` is T, and drops out of `required`.
fn property(field: &FieldDef, enums: &[EnumDef]) -> Schema {
    let description = (!field.doc.is_empty()).then(|| field.doc.clone());

    if let Some(def) = field.enum_def(enums) {
        return Schema {
            ty: Some("string"),
            description,
            enum_values: Some(def.variants.iter().map(|v| v.value.clone()).collect()),
            ..Schema::default()
        };
    }

    let items = |ty: &'static str| Some(Box::new(Schema { ty: Some(ty), ..Schema::default() }));
    match &field.field_type {
        FieldType::VecString => Schema { ty: Some("array"), items: items("string"), description, ..Schema::default() },
        FieldType::VecStruct(_) => {
            Schema { ty: Some("array"), items: items("object"), description, ..Schema::default() }
        }
        other => {
            let ty = match other {
                FieldType::I32 | FieldType::OptionI32 | FieldType::I64 | FieldType::OptionI64 => "integer",
                FieldType::F32 | FieldType::OptionF32 | FieldType::F64 | FieldType::OptionF64 => "number",
                FieldType::Bool | FieldType::OptionBool => "boolean",
                // Everything else — including a named type with no enum
                // declaration — is a string on the wire.
                _ => "string",
            };
            Schema { ty: Some(ty), description, ..Schema::default() }
        }
    }
}

/// Pretty-print with a two-space indent and a trailing newline.
fn render(doc: &impl Serialize) -> Result<String, CodegenError> {
    let json = serde_json::to_string_pretty(doc)
        .map_err(|e| CodegenError::Docs(format!("Failed to serialise JSON Schema: {e}")))?;
    Ok(format!("{json}\n"))
}

// ── Document shapes ─────────────────────────────────────────────────
//
// Serde serialises a struct's fields in declaration order, so the field order
// below is the key order of the emitted JSON.

#[derive(Serialize)]
struct EntitySchemaDoc {
    #[serde(rename = "$schema")]
    schema: &'static str,
    #[serde(rename = "$id")]
    id: String,
    title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(rename = "x-schema-revision")]
    revision: String,
    #[serde(rename = "type")]
    ty: &'static str,
    properties: OrderedMap<Schema>,
    required: Vec<String>,
    #[serde(rename = "additionalProperties")]
    additional_properties: bool,
}

#[derive(Serialize)]
struct ExportSchemaDoc {
    #[serde(rename = "$schema")]
    schema: &'static str,
    #[serde(rename = "$id")]
    id: String,
    title: String,
    #[serde(rename = "x-schema-revision")]
    revision: String,
    #[serde(rename = "type")]
    ty: &'static str,
    properties: OrderedMap<Schema>,
    required: Vec<String>,
}

/// A nested schema node. Every key is optional, so one type covers a field
/// property, an array's items, and the manifest's sub-objects.
#[derive(Serialize, Default)]
struct Schema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    ty: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    items: Option<Box<Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    enum_values: Option<Vec<String>>,
    #[serde(rename = "const", skip_serializing_if = "Option::is_none")]
    const_value: Option<String>,
    #[serde(rename = "$ref", skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<OrderedMap<Schema>>,
    #[serde(rename = "additionalProperties", skip_serializing_if = "Option::is_none")]
    additional_properties: Option<Box<Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    required: Option<Vec<String>>,
}

/// A JSON object that keeps insertion order. `properties` follows field
/// declaration order, which a `BTreeMap` would sort away.
struct OrderedMap<V>(Vec<(String, V)>);

impl<V: Serialize> Serialize for OrderedMap<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}
