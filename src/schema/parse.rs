//! Parse `#[ontology(...)]` annotations from schema source files using `syn`.
//!
//! This module reads `.rs` files from the schema directory, finds structs with
//! `#[derive(OntologyEntity)]` and `#[ontology(entity, ...)]`, and extracts
//! [`EntityDef`] metadata from them.

use std::fs;
use std::path::Path;

use syn::{Attribute, Expr, Field, Fields, ItemStruct, Lit, Meta, Type};

use ontogen_core::naming::to_snake_case;

use crate::schema::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};

/// Parse all schema files in the given directory, returning entity definitions
/// for structs annotated with `#[ontology(entity, ...)]`.
pub fn parse_schema_dir(dir: &Path) -> Result<Vec<EntityDef>, String> {
    let mut entities = Vec::new();

    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read schema directory {}: {e}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

            let parsed = parse_schema_source(&content, &path)?;
            entities.extend(parsed);
        }
    }

    Ok(entities)
}

/// Parse a single source file, returning any entity definitions found.
pub fn parse_schema_source(source: &str, path: &Path) -> Result<Vec<EntityDef>, String> {
    let syntax = syn::parse_file(source).map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    let mut entities = Vec::new();

    for item in &syntax.items {
        if let syn::Item::Struct(item_struct) = item
            && has_ontology_entity_derive(&item_struct.attrs)
            && let Some(entity) = parse_entity_struct(item_struct, path)?
        {
            entities.push(entity);
        }
    }

    Ok(entities)
}

/// Check if a struct has `#[derive(OntologyEntity)]`.
fn has_ontology_entity_derive(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("derive") {
            return false;
        }
        let Ok(nested) =
            attr.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        else {
            return false;
        };
        nested.iter().any(|p| p.is_ident("OntologyEntity"))
    })
}

/// Parse a struct with `#[ontology(entity, ...)]` into an `EntityDef`.
fn parse_entity_struct(input: &ItemStruct, path: &Path) -> Result<Option<EntityDef>, String> {
    let name = input.ident.to_string();
    let struct_attrs = parse_struct_ontology_attrs(&name, &input.attrs);
    let Some(struct_attrs) = struct_attrs else {
        return Ok(None); // No #[ontology(entity, ...)] on this struct
    };

    let fields = match &input.fields {
        Fields::Named(named) => &named.named,
        _ => {
            return Err(format!("Struct {} in {} must have named fields", input.ident, path.display()));
        }
    };

    let mut field_defs = Vec::new();
    for field in fields {
        field_defs.push(parse_field(field)?);
    }

    let default_snake = to_snake_case(&name);
    let directory = struct_attrs.directory.unwrap_or_else(|| default_snake.clone());
    let table = struct_attrs.table.unwrap_or_else(|| default_snake.clone());
    let type_name = struct_attrs.type_name.unwrap_or_else(|| default_snake.clone());
    let prefix = struct_attrs.prefix.unwrap_or_else(|| default_snake.clone());

    validate_identifier("directory", &directory).map_err(|e| format!("entity `{name}`: {e}"))?;
    validate_identifier("table", &table).map_err(|e| format!("entity `{name}`: {e}"))?;
    validate_identifier("type_name", &type_name).map_err(|e| format!("entity `{name}`: {e}"))?;
    validate_identifier("prefix", &prefix).map_err(|e| format!("entity `{name}`: {e}"))?;

    Ok(Some(EntityDef { name, directory, table, type_name, prefix, fields: field_defs }))
}

/// Validate that a user-supplied identifier conforms to `[A-Za-z_][A-Za-z0-9_]*`.
///
/// Applied to `table`, `directory`, `type_name`, and `prefix` values that flow
/// into generated code (including SQL), to reject empty strings and inputs
/// containing characters outside the standard identifier alphabet.
fn validate_identifier(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("invalid {field}=``: must not be empty"));
    }
    let mut chars = value.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!(
            "invalid {field}=`{value}`: must match [A-Za-z_][A-Za-z0-9_]* (must start with letter or underscore)"
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "invalid {field}=`{value}`: must match [A-Za-z_][A-Za-z0-9_]* (only letters, digits, or underscore allowed)"
        ));
    }
    Ok(())
}

/// Parsed struct-level `#[ontology(...)]` attributes.
struct StructOntologyAttrs {
    directory: Option<String>,
    table: Option<String>,
    type_name: Option<String>,
    prefix: Option<String>,
}

/// Parse the `#[ontology(entity, ...)]` attribute.
fn parse_struct_ontology_attrs(struct_name: &str, attrs: &[Attribute]) -> Option<StructOntologyAttrs> {
    for attr in attrs {
        if !attr.path().is_ident("ontology") {
            continue;
        }

        let Ok(nested) = attr.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            println!(
                "cargo:warning=ontogen: malformed #[ontology(...)] on struct `{struct_name}` — attribute will be ignored"
            );
            continue;
        };

        let mut is_entity = false;
        let mut directory = None;
        let mut table = None;
        let mut type_name = None;
        let mut prefix = None;

        for meta in &nested {
            match meta {
                Meta::Path(p) if p.is_ident("entity") => {
                    is_entity = true;
                }
                Meta::NameValue(nv) if nv.path.is_ident("directory") => {
                    directory = expr_to_string(&nv.value);
                }
                Meta::NameValue(nv) if nv.path.is_ident("table") => {
                    table = expr_to_string(&nv.value);
                }
                Meta::NameValue(nv) if nv.path.is_ident("type_name") => {
                    type_name = expr_to_string(&nv.value);
                }
                Meta::NameValue(nv) if nv.path.is_ident("prefix") => {
                    prefix = expr_to_string(&nv.value);
                }
                _ => {}
            }
        }

        if is_entity {
            return Some(StructOntologyAttrs { directory, table, type_name, prefix });
        }
    }

    None
}

/// Parse a single struct field into a `FieldDef`.
fn parse_field(field: &Field) -> Result<FieldDef, String> {
    let name = field.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();

    let field_type = classify_type(&field.ty);
    let ontology_attrs = parse_field_ontology_attrs(&name, &field.attrs).map_err(|e| format!("field `{name}`: {e}"))?;
    let serde_default = has_serde_default(&field.attrs);

    Ok(FieldDef {
        name,
        field_type,
        role: ontology_attrs.role,
        serde_default,
        multiline_list: ontology_attrs.multiline_list,
        default_value: ontology_attrs.default_value,
    })
}

/// Parsed field-level `#[ontology(...)]` attributes.
struct FieldOntologyAttrs {
    role: FieldRole,
    multiline_list: bool,
    default_value: Option<String>,
}

/// Parse all `#[ontology(...)]` field-level attributes, collecting role and rendering hints.
fn parse_field_ontology_attrs(field_name: &str, attrs: &[Attribute]) -> Result<FieldOntologyAttrs, String> {
    let mut result = FieldOntologyAttrs { role: FieldRole::Plain, multiline_list: false, default_value: None };

    for attr in attrs {
        if !attr.path().is_ident("ontology") {
            continue;
        }

        let Ok(nested) = attr.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            println!(
                "cargo:warning=ontogen: malformed #[ontology(...)] on field `{field_name}` — attribute will be ignored"
            );
            continue;
        };

        for meta in &nested {
            match meta {
                Meta::Path(p) if p.is_ident("id") => result.role = FieldRole::Id,
                Meta::Path(p) if p.is_ident("body") => result.role = FieldRole::Body,
                Meta::Path(p) if p.is_ident("enum_field") => result.role = FieldRole::EnumField,
                Meta::Path(p) if p.is_ident("skip") => result.role = FieldRole::Skip,
                Meta::Path(p) if p.is_ident("multiline_list") => result.multiline_list = true,
                Meta::List(list) if list.path.is_ident("relation") => {
                    if let Some(info) = parse_relation_meta(list)? {
                        result.role = FieldRole::Relation(info);
                    }
                }
                Meta::NameValue(nv) if nv.path.is_ident("default_value") => {
                    result.default_value = expr_to_string(&nv.value);
                }
                _ => {}
            }
        }
    }

    Ok(result)
}

/// Parse `#[ontology(relation(kind, target = "...", ...))]` into a `RelationInfo`.
///
/// The relation kind must be explicitly specified:
/// - `belongs_to` — FK column on this table (many-to-one)
/// - `has_many` — reverse of a belongs_to on the target (requires `foreign_key`)
/// - `many_to_many` — junction table (optionally override with `junction`)
fn parse_relation_meta(list: &syn::MetaList) -> Result<Option<RelationInfo>, String> {
    let Ok(nested) = list.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated) else {
        return Ok(None);
    };

    let mut kind_name = None;
    let mut target = None;
    let mut junction = None;
    let mut foreign_key = None;

    for meta in &nested {
        match meta {
            Meta::Path(p) if kind_name.is_none() => {
                // First bare identifier is the relation kind
                if let Some(ident) = p.get_ident() {
                    kind_name = Some(ident.to_string());
                }
            }
            Meta::NameValue(nv) if nv.path.is_ident("target") => {
                target = expr_to_string(&nv.value);
            }
            Meta::NameValue(nv) if nv.path.is_ident("junction") => {
                junction = expr_to_string(&nv.value);
            }
            Meta::NameValue(nv) if nv.path.is_ident("foreign_key") => {
                foreign_key = expr_to_string(&nv.value);
            }
            _ => {}
        }
    }

    let Some(target) = target else {
        return Ok(None);
    };
    let Some(kind_str) = kind_name else {
        return Ok(None);
    };

    let kind = match kind_str.as_str() {
        "belongs_to" => RelationKind::BelongsTo,
        "has_many" => RelationKind::HasMany,
        "many_to_many" => RelationKind::ManyToMany,
        other => {
            return Err(format!("Unknown relation kind '{other}'. Expected belongs_to, has_many, or many_to_many"));
        }
    };

    Ok(Some(RelationInfo { kind, target, junction, foreign_key }))
}

/// Check if a field has `#[serde(default)]`.
fn has_serde_default(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("serde") {
            return false;
        }
        let Ok(nested) = attr.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            return false;
        };
        nested.iter().any(|m| matches!(m, Meta::Path(p) if p.is_ident("default")))
    })
}

/// Classify a Rust type into our simplified `FieldType`.
fn classify_type(ty: &Type) -> FieldType {
    match ty {
        Type::Path(type_path) => {
            let segments: Vec<_> = type_path.path.segments.iter().map(|s| s.ident.to_string()).collect();

            let last_segment = type_path.path.segments.last();

            match segments.last().map(String::as_str) {
                Some("String") if segments.len() == 1 => FieldType::String,
                Some("i32") if segments.len() == 1 => FieldType::I32,
                Some("i64" | "u64") if segments.len() == 1 => FieldType::I64,
                Some("bool") if segments.len() == 1 => FieldType::Bool,
                Some("Option") => {
                    let inner = extract_generic_arg(last_segment);
                    match inner.as_deref() {
                        Some("String") => FieldType::OptionString,
                        Some("i32") => FieldType::OptionI32,
                        Some("i64" | "u64") => FieldType::OptionI64,
                        Some("bool") => FieldType::OptionBool,
                        Some(other) => FieldType::OptionEnum(other.to_string()),
                        None => FieldType::Other(quote::quote!(#ty).to_string()),
                    }
                }
                Some("Vec") => {
                    let inner = extract_generic_arg(last_segment);
                    match inner.as_deref() {
                        Some("String") => FieldType::VecString,
                        Some(other) => FieldType::VecStruct(other.to_string()),
                        None => FieldType::Other(quote::quote!(#ty).to_string()),
                    }
                }
                _ => FieldType::Other(quote::quote!(#ty).to_string()),
            }
        }
        _ => FieldType::Other(quote::quote!(#ty).to_string()),
    }
}

/// Extract the single generic type argument from a path segment (e.g., `Option<String>` -> `"String"`).
fn extract_generic_arg(segment: Option<&syn::PathSegment>) -> Option<String> {
    let segment = segment?;
    match &segment.arguments {
        syn::PathArguments::AngleBracketed(args) => {
            let first = args.args.first()?;
            match first {
                syn::GenericArgument::Type(Type::Path(tp)) => Some(tp.path.segments.last()?.ident.to_string()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract a string literal from an expression (e.g., `"nodes"` -> `Some("nodes")`).
fn expr_to_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(s) => Some(s.value()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_node_schema() {
        let source = r#"
            use ontogen_macros::OntologyEntity;
            use serde::{Deserialize, Serialize};

            #[derive(Debug, Clone, Serialize, Deserialize, OntologyEntity)]
            #[ontology(entity, directory = "nodes", table = "nodes")]
            pub struct Node {
                #[ontology(id)]
                pub id: String,

                pub name: String,

                #[ontology(enum_field)]
                pub kind: Option<NodeKind>,

                #[serde(default)]
                #[ontology(relation(belongs_to, target = "Node"))]
                pub parent_id: Option<String>,

                #[serde(default)]
                #[ontology(relation(has_many, target = "Node", foreign_key = "parent_id"))]
                pub contains: Vec<String>,

                pub owner: Option<String>,

                #[serde(default)]
                pub tags: Vec<String>,

                #[serde(default)]
                #[ontology(body)]
                pub body: String,

                #[serde(default)]
                #[ontology(relation(many_to_many, target = "Requirement"))]
                pub fulfills: Vec<String>,
            }
        "#;

        let path = Path::new("test_node.rs");
        let entities = parse_schema_source(source, path).unwrap();
        assert_eq!(entities.len(), 1);

        let node = &entities[0];
        assert_eq!(node.name, "Node");
        assert_eq!(node.directory, "nodes");
        assert_eq!(node.table, "nodes");
        assert_eq!(node.type_name, "node");
        assert_eq!(node.prefix, "node");

        // id field
        let id = node.id_field().unwrap();
        assert_eq!(id.name, "id");
        assert_eq!(id.field_type, FieldType::String);
        assert_eq!(id.role, FieldRole::Id);

        // name field (plain)
        let name_field = &node.fields[1];
        assert_eq!(name_field.name, "name");
        assert_eq!(name_field.field_type, FieldType::String);
        assert_eq!(name_field.role, FieldRole::Plain);

        // kind field (enum)
        let kind = &node.fields[2];
        assert_eq!(kind.name, "kind");
        assert_eq!(kind.field_type, FieldType::OptionEnum("NodeKind".to_string()));
        assert_eq!(kind.role, FieldRole::EnumField);

        // parent_id (belongs_to)
        let parent = &node.fields[3];
        assert_eq!(parent.name, "parent_id");
        assert_eq!(parent.field_type, FieldType::OptionString);
        assert!(parent.serde_default);
        match &parent.role {
            FieldRole::Relation(info) => {
                assert_eq!(info.kind, RelationKind::BelongsTo);
                assert_eq!(info.target, "Node");
            }
            other => panic!("Expected Relation, got {other:?}"),
        }

        // contains (has_many — reverse of parent_id, no junction table)
        let contains = &node.fields[4];
        assert_eq!(contains.name, "contains");
        assert_eq!(contains.field_type, FieldType::VecString);
        match &contains.role {
            FieldRole::Relation(info) => {
                assert_eq!(info.kind, RelationKind::HasMany);
                assert_eq!(info.target, "Node");
                assert_eq!(info.foreign_key, Some("parent_id".to_string()));
            }
            other => panic!("Expected Relation, got {other:?}"),
        }

        // owner (plain optional)
        let owner = &node.fields[5];
        assert_eq!(owner.name, "owner");
        assert_eq!(owner.field_type, FieldType::OptionString);
        assert_eq!(owner.role, FieldRole::Plain);

        // tags (plain vec, not a relation)
        let tags = &node.fields[6];
        assert_eq!(tags.name, "tags");
        assert_eq!(tags.field_type, FieldType::VecString);
        assert_eq!(tags.role, FieldRole::Plain);
        assert!(tags.serde_default);

        // body
        let body = node.body_field().unwrap();
        assert_eq!(body.name, "body");
        assert_eq!(body.role, FieldRole::Body);

        // fulfills (many_to_many -> Requirement)
        let fulfills = &node.fields[8];
        assert_eq!(fulfills.name, "fulfills");
        match &fulfills.role {
            FieldRole::Relation(info) => {
                assert_eq!(info.kind, RelationKind::ManyToMany);
                assert_eq!(info.target, "Requirement");
            }
            other => panic!("Expected Relation, got {other:?}"),
        }

        // junction_relations — only fulfills (many_to_many)
        let junctions: Vec<_> = node.junction_relations().collect();
        assert_eq!(junctions.len(), 1);
        assert_eq!(junctions[0].0.name, "fulfills");

        // has_many_relations — only contains
        let has_many: Vec<_> = node.has_many_relations().collect();
        assert_eq!(has_many.len(), 1);
        assert_eq!(has_many[0].0.name, "contains");

        // belongs_to_relations
        let belongs_to: Vec<_> = node.belongs_to_relations().collect();
        assert_eq!(belongs_to.len(), 1); // parent_id
    }

    #[test]
    fn parse_unknown_relation_kind_returns_error() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Node {
                #[ontology(id)]
                pub id: String,

                #[ontology(relation(unknown_kind, target = "Node"))]
                pub parent_id: Option<String>,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let err = parse_schema_source(source, Path::new("test.rs")).unwrap_err();
        assert!(err.contains("Unknown relation kind"), "expected error to mention 'Unknown relation kind', got: {err}");
        assert!(err.contains("unknown_kind"), "expected error to mention the bad kind, got: {err}");
    }

    #[test]
    fn parse_inferred_defaults() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Agent {
                #[ontology(id)]
                pub id: String,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        assert_eq!(entities.len(), 1);
        let agent = &entities[0];
        assert_eq!(agent.directory, "agent");
        assert_eq!(agent.table, "agent");
        assert_eq!(agent.type_name, "agent");
        assert_eq!(agent.prefix, "agent");
    }

    #[test]
    fn parse_inferred_with_overrides() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity, table = "work_sessions", prefix = "session")]
            pub struct WorkSession {
                #[ontology(id)]
                pub id: String,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        let ws = &entities[0];
        assert_eq!(ws.directory, "work_session"); // inferred from name
        assert_eq!(ws.table, "work_sessions"); // overridden
        assert_eq!(ws.type_name, "work_session"); // inferred
        assert_eq!(ws.prefix, "session"); // overridden
    }

    #[test]
    fn parse_type_name_override() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity, directory = "sessions", table = "work_sessions", type_name = "work_session")]
            pub struct WorkSession {
                #[ontology(id)]
                pub id: String,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        assert_eq!(entities[0].type_name, "work_session");
    }

    #[test]
    fn parse_skip_field() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity, directory = "specs", table = "specifications")]
            pub struct Specification {
                #[ontology(id)]
                pub id: String,

                #[ontology(skip)]
                pub acceptance_criteria: Vec<AcceptanceCriterion>,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        let spec = &entities[0];
        let ac = &spec.fields[1];
        assert_eq!(ac.name, "acceptance_criteria");
        assert_eq!(ac.role, FieldRole::Skip);
    }

    #[test]
    fn struct_without_ontology_entity_is_skipped() {
        let source = r#"
            #[derive(Debug, Clone)]
            pub struct NotAnEntity {
                pub id: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn parse_junction_override() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity, directory = "nodes", table = "nodes")]
            pub struct Node {
                #[ontology(id)]
                pub id: String,

                #[ontology(relation(many_to_many, target = "Requirement", junction = "node_fulfills_req"))]
                pub fulfills: Vec<String>,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        let fulfills = &entities[0].fields[1];
        match &fulfills.role {
            FieldRole::Relation(info) => {
                assert_eq!(info.kind, RelationKind::ManyToMany);
                assert_eq!(info.junction, Some("node_fulfills_req".to_string()));
            }
            other => panic!("Expected Relation, got {other:?}"),
        }
    }

    #[test]
    fn parse_has_many_with_foreign_key() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Node {
                #[ontology(id)]
                pub id: String,

                #[ontology(relation(has_many, target = "Node", foreign_key = "parent_id"))]
                pub contains: Vec<String>,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        let contains = &entities[0].fields[1];
        match &contains.role {
            FieldRole::Relation(info) => {
                assert_eq!(info.kind, RelationKind::HasMany);
                assert_eq!(info.target, "Node");
                assert_eq!(info.foreign_key, Some("parent_id".to_string()));
            }
            other => panic!("Expected Relation, got {other:?}"),
        }
    }

    #[test]
    fn malformed_ontology_attr_on_field_is_skipped_cleanly() {
        // Malformed `#[ontology(...)]` at the field level should be skipped with a
        // `cargo:warning` diagnostic rather than panicking — the rest of the struct
        // should still parse normally.
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Node {
                #[ontology(id)]
                pub id: String,

                #[ontology(this is garbage syntax)]
                pub name: String,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        assert_eq!(entities.len(), 1);
        let node = &entities[0];

        // The malformed attr is ignored — `name` still shows up as a plain field.
        let name_field = node.fields.iter().find(|f| f.name == "name").expect("name field should exist");
        assert_eq!(name_field.role, FieldRole::Plain);

        // id and body still parse correctly.
        assert!(node.id_field().is_some());
        assert!(node.body_field().is_some());
    }

    #[test]
    fn malformed_relation_meta_is_skipped_cleanly() {
        // Malformed inner `relation(...)` should not panic; the outer `#[ontology(...)]`
        // parses, so the field falls back to Plain.
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Node {
                #[ontology(id)]
                pub id: String,

                #[ontology(relation(!! not valid !!))]
                pub parent_id: Option<String>,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        let node = &entities[0];
        let parent = node.fields.iter().find(|f| f.name == "parent_id").expect("parent_id should exist");
        // With a malformed relation the role stays Plain (the `relation` arm bails out).
        assert_eq!(parent.role, FieldRole::Plain);
    }

    #[test]
    fn malformed_ontology_attr_on_struct_is_skipped_cleanly() {
        // Malformed struct-level `#[ontology(...)]` short-circuits parse_struct_ontology_attrs
        // for that attribute; since no well-formed `#[ontology(entity, ...)]` is present,
        // the struct is skipped (no EntityDef produced), but parsing must not panic.
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(!! not valid !!)]
            pub struct Broken {
                pub id: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        // Malformed attr is skipped, no `entity` marker ever found -> no entities produced.
        assert!(entities.is_empty());
    }

    #[test]
    fn to_snake_case_works() {
        assert_eq!(to_snake_case("Node"), "node");
        assert_eq!(to_snake_case("WorkSession"), "work_session");
        assert_eq!(to_snake_case("Agent"), "agent");
        assert_eq!(to_snake_case("Evidence"), "evidence");
    }

    /// Parse the actual schema directory and verify all 10 entities are found
    /// with correct metadata.
    #[test]
    fn parse_all_real_schemas() {
        let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/schema");

        if !schema_dir.exists() {
            // Skip in CI or standalone builds where the full repo isn't available
            eprintln!("Skipping parse_all_real_schemas: schema dir not found at {}", schema_dir.display());
            return;
        }

        let entities = parse_schema_dir(&schema_dir).expect("failed to parse schema dir");
        let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();

        // All 17 entities must be present
        let expected = [
            "Product",
            "Problem",
            "Goal",
            "Capability",
            "Relation",
            "Contract",
            "Constraint",
            "Evidence",
            "Specification",
            "Requirement",
            "AcceptanceCriterion",
            "UnitOfWork",
            "WorkflowTemplate",
            "StepResult",
            "Agent",
            "Role",
            "WorkExecution",
        ];
        for name in &expected {
            assert!(names.contains(name), "Missing entity: {name}. Found: {names:?}");
        }
        assert_eq!(
            entities.len(),
            expected.len(),
            "Expected exactly {} entities, found {}: {:?}",
            expected.len(),
            entities.len(),
            names
        );

        // Spot-check key properties
        let find = |name: &str| entities.iter().find(|e| e.name == name).unwrap();

        // Capability (renamed from Node)
        let cap = find("Capability");
        assert_eq!(cap.directory, "capability");
        assert_eq!(cap.table, "capabilities");
        assert!(cap.id_field().is_some());
        assert!(cap.body_field().is_some());
        assert_eq!(cap.id_field().unwrap().field_type, FieldType::String);
        // has_many (contains) + belongs_to (parent_id) + many_to_many (goal_ids)
        assert_eq!(cap.belongs_to_relations().count(), 1, "Capability should have 1 belongs_to");
        assert_eq!(cap.has_many_relations().count(), 1, "Capability should have 1 has_many");
        assert_eq!(cap.junction_relations().count(), 1, "Capability should have 1 many_to_many");

        // Contract — 4 belongs_to (scope, from_id, to_id, spec) + 1 many_to_many (fulfills)
        let contract = find("Contract");
        assert_eq!(contract.directory, "contract");
        assert_eq!(contract.belongs_to_relations().count(), 4, "Contract should have 4 belongs_to");
        assert_eq!(contract.junction_relations().count(), 1, "Contract should have 1 many_to_many");

        // Evidence — no relations (polymorphic ref not annotated yet)
        let evidence = find("Evidence");
        assert_eq!(evidence.id_field().unwrap().field_type, FieldType::String);
        assert_eq!(evidence.belongs_to_relations().count(), 0);

        // Relation — 2 belongs_to (from_id, to_id)
        let rel = find("Relation");
        assert_eq!(rel.directory, "relation");
        assert_eq!(rel.table, "relations");
        assert_eq!(rel.belongs_to_relations().count(), 2, "Relation should have 2 belongs_to");

        // Requirement — 3 belongs_to (specification_id, parent_id, superseded_by) + 1 many_to_many (depends_on)
        let req = find("Requirement");
        assert_eq!(req.belongs_to_relations().count(), 3);
        assert_eq!(req.junction_relations().count(), 1);

        // Specification — 2 many_to_many (capability_ids, depends_on)
        let spec = find("Specification");
        assert_eq!(spec.junction_relations().count(), 2);

        // Constraint — 1 many_to_many (scope_ids)
        let cst = find("Constraint");
        assert_eq!(cst.directory, "constraint");
        assert_eq!(cst.table, "constraints");
        assert_eq!(cst.junction_relations().count(), 1);

        // AcceptanceCriterion — 1 belongs_to (requirement_id)
        let ac = find("AcceptanceCriterion");
        assert_eq!(ac.directory, "acceptance_criterion");
        assert_eq!(ac.table, "acceptance_criteria");
        assert_eq!(ac.belongs_to_relations().count(), 1);

        // Product — no relations
        let product = find("Product");
        assert_eq!(product.directory, "product");
        assert_eq!(product.relation_fields().count(), 0);

        // Problem — 1 belongs_to (product_id)
        let problem = find("Problem");
        assert_eq!(problem.belongs_to_relations().count(), 1);

        // Goal — 1 belongs_to (problem_id)
        let goal = find("Goal");
        assert_eq!(goal.belongs_to_relations().count(), 1);

        // WorkExecution — 2 belongs_to (unit_of_work_id, workflow_template_id)
        let we = find("WorkExecution");
        assert_eq!(we.table, "work_executions");
        assert_eq!(we.belongs_to_relations().count(), 2);

        // UnitOfWork — 2 belongs_to (workflow_template_id, parent_id) + 2 many_to_many (depends_on, constraints)
        let uow = find("UnitOfWork");
        assert_eq!(uow.directory, "unit_of_work");
        assert_eq!(uow.table, "units_of_work");
        assert_eq!(uow.belongs_to_relations().count(), 2);
        assert_eq!(uow.junction_relations().count(), 2);

        // StepResult — 2 belongs_to (work_execution_id, agent_id)
        let sr = find("StepResult");
        assert_eq!(sr.directory, "step_result");
        assert_eq!(sr.table, "step_results");
        assert_eq!(sr.belongs_to_relations().count(), 2);

        // Agent — no relations
        let agent = find("Agent");
        assert_eq!(agent.relation_fields().count(), 0);
    }

    #[test]
    fn rejects_sql_injection_in_table() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity, table = "users'; DROP TABLE users; --")]
            pub struct Node {
                #[ontology(id)]
                pub id: String,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let err = parse_schema_source(source, Path::new("test.rs")).expect_err("expected validation error");
        assert!(err.contains("invalid"), "error should mention `invalid`: {err}");
        assert!(err.contains("table"), "error should mention field name `table`: {err}");
        assert!(err.contains("Node"), "error should mention entity name `Node`: {err}");
    }

    #[test]
    fn rejects_invalid_directory() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity, directory = "1bad-dir")]
            pub struct Thing {
                #[ontology(id)]
                pub id: String,
            }
        "#;

        let err = parse_schema_source(source, Path::new("test.rs")).expect_err("expected validation error");
        assert!(err.contains("invalid"));
        assert!(err.contains("directory"));
    }

    #[test]
    fn rejects_empty_prefix() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity, prefix = "")]
            pub struct Thing {
                #[ontology(id)]
                pub id: String,
            }
        "#;

        let err = parse_schema_source(source, Path::new("test.rs")).expect_err("expected validation error");
        assert!(err.contains("prefix"));
        assert!(err.contains("empty"));
    }

    #[test]
    fn validate_identifier_accepts_good_values() {
        assert!(validate_identifier("table", "users").is_ok());
        assert!(validate_identifier("table", "work_sessions").is_ok());
        assert!(validate_identifier("prefix", "_leading_underscore").is_ok());
        assert!(validate_identifier("type_name", "Node42").is_ok());
    }

    #[test]
    fn validate_identifier_rejects_bad_values() {
        assert!(validate_identifier("table", "").is_err());
        assert!(validate_identifier("table", "1leading_digit").is_err());
        assert!(validate_identifier("table", "has-dash").is_err());
        assert!(validate_identifier("table", "has space").is_err());
        assert!(validate_identifier("table", "users'; DROP TABLE users; --").is_err());
    }
}
