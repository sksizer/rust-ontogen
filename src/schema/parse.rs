//! Parse `#[ontology(...)]` annotations from schema source files using `syn`.
//!
//! This module reads `.rs` files from the schema directory, finds structs with
//! `#[derive(OntologyEntity)]` and `#[ontology(entity, ...)]`, and extracts
//! [`EntityDef`] metadata from them. The string enums declared beside them
//! become [`EnumDef`]s.

use std::fs;
use std::path::{Path, PathBuf};

use syn::{Attribute, Expr, Field, Fields, ItemStruct, Lit, Meta, Type};

use ontogen_core::naming::to_snake_case;

use crate::ir::IdStrategy;
use crate::schema::model::{
    EntityDef, EnumDef, EnumVariant, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind,
};

/// Parse all schema files in the given directory, returning entity definitions
/// for structs annotated with `#[ontology(entity, ...)]`.
///
/// Files are visited in sorted path order: `read_dir` order is
/// platform-dependent (inode/hash order on Linux, insertion order on APFS),
/// and entity order flows into every generated artifact - unsorted, the same
/// schema emits differently ordered code on different machines, which reads
/// as codegen drift to consumers that commit generated output.
pub fn parse_schema_dir(dir: &Path) -> Result<Vec<EntityDef>, String> {
    let mut entities = Vec::new();
    for (path, content) in schema_sources(dir)? {
        entities.extend(parse_schema_source(&content, &path)?);
    }
    Ok(entities)
}

/// Parse the string enums declared in the schema directory, in the same
/// file order as [`parse_schema_dir`].
pub fn parse_schema_enums_dir(dir: &Path) -> Result<Vec<EnumDef>, String> {
    let mut enums = Vec::new();
    for (path, content) in schema_sources(dir)? {
        enums.extend(parse_schema_enums_source(&content, &path)?);
    }
    Ok(enums)
}

/// The `.rs` files of the schema directory with their contents, in sorted path order.
fn schema_sources(dir: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read schema directory {}: {e}", dir.display()))?;

    let mut paths: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            Ok((path, content))
        })
        .collect()
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

/// Parse the string enums in a single source file: every enum whose variants
/// are all unit variants, with the serde wire name of each. An enum with a
/// payload variant, or a serde shape ontogen-ts rejects, is not a string enum
/// and is left out.
pub fn parse_schema_enums_source(source: &str, path: &Path) -> Result<Vec<EnumDef>, String> {
    let syntax = syn::parse_file(source).map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    Ok(syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Enum(item_enum) => parse_string_enum(item_enum),
            _ => None,
        })
        .collect())
}

fn parse_string_enum(item: &syn::ItemEnum) -> Option<EnumDef> {
    let variants = ontogen_ts::unit_variant_wire_names(item).ok().flatten()?;
    Some(EnumDef {
        name: item.ident.to_string(),
        variants: variants.into_iter().map(|(name, value)| EnumVariant { name, value }).collect(),
    })
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

    reject_serde_shape_attrs(&input.attrs).map_err(|e| format!("entity `{name}` in {}: {e}", path.display()))?;

    let mut field_defs = Vec::new();
    for field in fields {
        field_defs.push(parse_field(field).map_err(|e| format!("entity `{name}` in {}: {e}", path.display()))?);
    }

    let default_snake = to_snake_case(&name);
    // Every generated layer names a module after the entity, and these
    // keywords have no raw form (`r#crate` is not an identifier).
    if crate::ident::UNRAWABLE.contains(&default_snake.as_str()) {
        return Err(format!(
            "entity `{name}` in {}: its snake_case name `{default_snake}` is a Rust keyword that cannot name a module, \
             not even as a raw identifier; rename the entity (e.g. `{name}Item`)",
            path.display()
        ));
    }
    let directory = struct_attrs.directory.unwrap_or_else(|| default_snake.clone());
    let table = struct_attrs.table.unwrap_or_else(|| default_snake.clone());
    let type_name = struct_attrs.type_name.unwrap_or_else(|| name.clone());
    let prefix = struct_attrs.prefix.unwrap_or_else(|| default_snake.clone());

    validate_identifier("directory", &directory).map_err(|e| format!("entity `{name}`: {e}"))?;
    validate_identifier("table", &table).map_err(|e| format!("entity `{name}`: {e}"))?;
    validate_identifier("type_name", &type_name).map_err(|e| format!("entity `{name}`: {e}"))?;
    validate_identifier("prefix", &prefix).map_err(|e| format!("entity `{name}`: {e}"))?;

    let id_strategy = struct_attrs
        .id
        .map(|expr| parse_id_strategy(&expr, &field_defs))
        .transpose()
        .map_err(|e| format!("entity `{name}` in {}: {e}", path.display()))?;

    let doc = doc_comment(&input.attrs);

    Ok(Some(EntityDef { name, doc, directory, table, type_name, prefix, id_strategy, fields: field_defs }))
}

/// The accepted spellings of `#[ontology(entity, id = "...")]`, for errors.
const ID_FORMS: &str = r#"expected `id = "provided"`, `id = "uuid"` or `id = "slug(<field>)"`"#;

/// Parse the value of `#[ontology(entity, id = "...")]`: `"provided"`,
/// `"uuid"`, or `"slug(<field>)"` naming a plain `String` field of the entity
/// (the generated create reads it on either backend).
fn parse_id_strategy(expr: &Expr, fields: &[FieldDef]) -> Result<IdStrategy, String> {
    let Some(value) = expr_to_string(expr) else {
        return Err(format!("`id` must be a string literal; {ID_FORMS}"));
    };
    match value.as_str() {
        "provided" => return Ok(IdStrategy::Provided),
        "uuid" => return Ok(IdStrategy::Uuid),
        _ => {}
    }
    let Some(field) = value.strip_prefix("slug(").and_then(|rest| rest.strip_suffix(')')) else {
        return Err(format!("invalid `id = {value:?}`: {ID_FORMS}"));
    };
    if validate_identifier("slug field", field).is_err() {
        return Err(format!("invalid `id = {value:?}`: the slug field must be a field name; {ID_FORMS}"));
    }
    match fields.iter().find(|f| f.name == field) {
        Some(f) if f.field_type == FieldType::String => Ok(IdStrategy::SlugFromField(field.to_string())),
        Some(f) => Err(format!(
            "`id = {value:?}`: field `{field}` must be a plain String to derive ids from, found {:?}",
            f.field_type
        )),
        None => Err(format!("`id = {value:?}`: the entity has no field `{field}` to derive ids from")),
    }
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
    /// The raw `id = ...` value, checked against the fields once they are parsed.
    id: Option<Expr>,
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
                "cargo:warning=ontogen: malformed #[ontology(...)] on struct `{struct_name}` - attribute will be ignored"
            );
            continue;
        };

        let mut is_entity = false;
        let mut directory = None;
        let mut table = None;
        let mut type_name = None;
        let mut prefix = None;
        let mut id = None;

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
                Meta::NameValue(nv) if nv.path.is_ident("id") => {
                    id = Some(nv.value.clone());
                }
                _ => {}
            }
        }

        if is_entity {
            return Some(StructOntologyAttrs { directory, table, type_name, prefix, id });
        }
    }

    None
}

/// Parse a single struct field into a `FieldDef`.
fn parse_field(field: &Field) -> Result<FieldDef, String> {
    let name = field.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();

    reject_serde_shape_attrs(&field.attrs).map_err(|e| format!("field `{name}`: {e}"))?;
    let field_type = classify_type(&field.ty);
    let ontology_attrs = parse_field_ontology_attrs(&name, &field.attrs).map_err(|e| format!("field `{name}`: {e}"))?;
    let serde_default = has_serde_default(&field.attrs);

    Ok(FieldDef {
        name,
        doc: doc_comment(&field.attrs),
        field_type,
        role: ontology_attrs.role,
        serde_default,
        multiline_list: ontology_attrs.multiline_list,
        default_value: ontology_attrs.default_value,
        frontmatter_name: ontology_attrs.frontmatter_name,
    })
}

/// Join the `///` lines of an item into one string.
///
/// Rustc rewrites `/// text` into `#[doc = " text"]`, so one leading space is
/// trimmed per line and the lines are joined with newlines. An item with no
/// doc comment gets an empty string.
fn doc_comment(attrs: &[Attribute]) -> String {
    attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc")
                && let Meta::NameValue(nv) = &attr.meta
                && let Expr::Lit(syn::ExprLit { lit: Lit::Str(s), .. }) = &nv.value
            {
                let line = s.value();
                return Some(line.strip_prefix(' ').unwrap_or(&line).to_string());
            }
            None
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parsed field-level `#[ontology(...)]` attributes.
struct FieldOntologyAttrs {
    role: FieldRole,
    multiline_list: bool,
    default_value: Option<String>,
    frontmatter_name: Option<String>,
}

/// Parse all `#[ontology(...)]` field-level attributes, collecting role and rendering hints.
fn parse_field_ontology_attrs(field_name: &str, attrs: &[Attribute]) -> Result<FieldOntologyAttrs, String> {
    let mut result = FieldOntologyAttrs {
        role: FieldRole::Plain,
        multiline_list: false,
        default_value: None,
        frontmatter_name: None,
    };

    for attr in attrs {
        if !attr.path().is_ident("ontology") {
            continue;
        }

        let Ok(nested) = attr.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            println!(
                "cargo:warning=ontogen: malformed #[ontology(...)] on field `{field_name}` - attribute will be ignored"
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
                Meta::NameValue(nv) if nv.path.is_ident("frontmatter_name") => {
                    let key = expr_to_string(&nv.value).ok_or("frontmatter_name must be a string literal")?;
                    validate_frontmatter_key(&key)?;
                    result.frontmatter_name = Some(key);
                }
                _ => {}
            }
        }
    }

    Ok(result)
}

/// Validate a `frontmatter_name` value: a plain YAML key that never needs
/// quoting, `[A-Za-z_][A-Za-z0-9_-]*`. Key-level rules that need the whole
/// entity (collisions, the reserved `type`) are checked by the markdown
/// generator, the only consumer of the key.
fn validate_frontmatter_key(key: &str) -> Result<(), String> {
    let mut chars = key.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return Err(format!("invalid frontmatter_name=`{key}`: must match [A-Za-z_][A-Za-z0-9_-]*"));
    }
    if is_yaml_scalar_word(key) {
        return Err(format!(
            "invalid frontmatter_name=`{key}`: YAML reads it as a bool or null, not a string key; choose another name"
        ));
    }
    Ok(())
}

/// Plain words a YAML parser resolves to a bool or null rather than a
/// string. YAML 1.2's core schema only has true/false/null; YAML 1.1 (which
/// many vault tools still parse with) adds yes/no/on/off/y/n. Rejecting the
/// union keeps the key a string for every reader.
fn is_yaml_scalar_word(key: &str) -> bool {
    const WORDS: &[&str] = &["true", "false", "null", "yes", "no", "on", "off", "y", "n"];
    WORDS.iter().any(|w| key.eq_ignore_ascii_case(w))
}

/// Parse `#[ontology(relation(kind, target = "...", ...))]` into a `RelationInfo`.
///
/// The relation kind must be explicitly specified:
/// - `belongs_to` - FK column on this table (many-to-one)
/// - `has_many` - reverse of a belongs_to on the target (requires `foreign_key`)
/// - `many_to_many` - junction table (optionally override with `junction`)
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
                // Spelled as the field is named, so `"in"` finds `r#in`.
                foreign_key = expr_to_string(&nv.value).map(|fk| crate::ident::rust_ident(&fk));
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

/// Serde attributes that make an entity's serialized shape differ from its
/// Rust field names. Every layer that names fields on the wire (JSON:API
/// attributes, the TS bindings, the markdown frontmatter) assumes the two
/// agree, so an entity may not carry them (wire contract §5.3). `default`
/// and `deserialize_with` change only deserialization and stay allowed.
const SERDE_SHAPE_ATTRS: [&str; 9] = [
    "rename",
    "rename_all",
    "alias",
    "flatten",
    "skip",
    "skip_serializing",
    "skip_serializing_if",
    "serialize_with",
    "with",
];

fn reject_serde_shape_attrs(attrs: &[Attribute]) -> Result<(), String> {
    for attr in attrs.iter().filter(|a| a.path().is_ident("serde")) {
        let Ok(nested) = attr.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            continue;
        };
        if let Some(key) = nested.iter().find_map(|m| SERDE_SHAPE_ATTRS.into_iter().find(|k| m.path().is_ident(k))) {
            return Err(format!(
                "`#[serde({key})]` changes the serialized shape, which entities may not do: their wire member names \
                 are their Rust field names"
            ));
        }
    }
    Ok(())
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
                Some("i64") if segments.len() == 1 => FieldType::I64,
                Some("f32") if segments.len() == 1 => FieldType::F32,
                Some("f64") if segments.len() == 1 => FieldType::F64,
                Some("bool") if segments.len() == 1 => FieldType::Bool,
                Some("Option") => {
                    let inner = extract_generic_arg(last_segment);
                    match inner.as_deref() {
                        Some("String") => FieldType::OptionString,
                        Some("i32") => FieldType::OptionI32,
                        Some("i64") => FieldType::OptionI64,
                        Some("f32") => FieldType::OptionF32,
                        Some("f64") => FieldType::OptionF64,
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
    fn string_enums_parse_with_their_serde_names() {
        let src = r#"
            #[derive(Serialize, Deserialize)]
            #[serde(rename_all = "kebab-case")]
            pub enum Quality {
                PeerReviewed,
                #[serde(rename = "crowd")]
                Community,
                #[serde(skip)]
                Hidden,
            }

            pub enum AppError {
                SourceNotFound(String),
            }

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Source {
                #[ontology(id)]
                pub id: String,
                pub quality: Option<Quality>,
                pub name: String,
            }
        "#;
        let path = Path::new("source.rs");

        let enums = parse_schema_enums_source(src, path).expect("parse enums");
        assert_eq!(enums.len(), 1, "a payload variant keeps AppError out: {enums:?}");
        assert_eq!(enums[0].name, "Quality");
        let values: Vec<_> = enums[0].variants.iter().map(|v| v.value.as_str()).collect();
        assert_eq!(values, ["peer-reviewed", "crowd"]);

        let entities = parse_schema_source(src, path).expect("parse entities");
        let field = |name: &str| entities[0].fields.iter().find(|f| f.name == name).unwrap();
        assert_eq!(field("quality").enum_def(&enums).map(|e| e.name.as_str()), Some("Quality"));
        assert!(field("name").enum_def(&enums).is_none());
    }

    #[test]
    fn doc_comments_reach_the_entity_and_its_fields() {
        let src = r#"
            #[derive(OntologyEntity)]
            #[ontology(entity)]
            /// A single interval.
            /// Distances are metric.
            pub struct Interval {
                #[ontology(id)]
                pub id: String,

                /// Integer metres.
                pub distance_m: i32,

                pub note: Option<String>,
            }
        "#;

        let entities = parse_schema_source(src, Path::new("interval.rs")).expect("parse entities");
        assert_eq!(entities[0].doc, "A single interval.\nDistances are metric.");

        let field = |name: &str| entities[0].fields.iter().find(|f| f.name == name).unwrap();
        assert_eq!(field("distance_m").doc, "Integer metres.");
        assert_eq!(field("note").doc, "");
    }

    #[test]
    fn schema_dir_entities_follow_sorted_file_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let entity = |name: &str, directory: &str| {
            format!(
                r#"
                use ontogen_macros::OntologyEntity;

                #[derive(OntologyEntity)]
                #[ontology(entity, directory = "{directory}", table = "{directory}")]
                pub struct {name} {{
                    #[ontology(id)]
                    pub id: String,
                }}
                "#
            )
        };
        // Written in reverse alphabetical order on purpose: entity order must
        // come from the path sort, not from directory insertion order.
        std::fs::write(dir.path().join("zebra.rs"), entity("Zebra", "zebras")).expect("write");
        std::fs::write(dir.path().join("aardvark.rs"), entity("Aardvark", "aardvarks")).expect("write");

        let entities = parse_schema_dir(dir.path()).expect("parse");
        let names: Vec<_> = entities.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Aardvark", "Zebra"]);
    }

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
        assert_eq!(node.type_name, "Node", "the OKF type defaults to the struct name");
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

        // contains (has_many - reverse of parent_id, no junction table)
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

        // junction_relations - only fulfills (many_to_many)
        let junctions: Vec<_> = node.junction_relations().collect();
        assert_eq!(junctions.len(), 1);
        assert_eq!(junctions[0].0.name, "fulfills");

        // has_many_relations - only contains
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
        assert_eq!(agent.type_name, "Agent");
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
        assert_eq!(ws.type_name, "WorkSession"); // inferred from the struct name
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

    /// An entity with `id = "<value>"` on its `#[ontology(entity, ...)]`.
    fn entity_with_id(value: &str) -> Result<Vec<EntityDef>, String> {
        let source = format!(
            r#"
            #[derive(OntologyEntity)]
            #[ontology(entity, directory = "tasks", id = {value})]
            pub struct Task {{
                #[ontology(id)]
                pub id: String,
                pub title: String,
                pub summary: Option<String>,
                pub points: i32,
            }}
        "#
        );
        parse_schema_source(&source, Path::new("task.rs"))
    }

    #[test]
    fn entity_id_strategy_is_absent_without_the_key() {
        let source = r#"
            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Tag {
                #[ontology(id)]
                pub id: String,
            }
        "#;
        let entities = parse_schema_source(source, Path::new("tag.rs")).unwrap();
        assert_eq!(entities[0].id_strategy, None, "no key: the store default applies");
    }

    #[test]
    fn entity_id_strategy_accepts_every_form() {
        for (value, expected) in [
            (r#""provided""#, IdStrategy::Provided),
            (r#""uuid""#, IdStrategy::Uuid),
            (r#""slug(title)""#, IdStrategy::SlugFromField("title".into())),
        ] {
            let entities = entity_with_id(value).unwrap_or_else(|e| panic!("id = {value}: {e}"));
            assert_eq!(entities[0].id_strategy, Some(expected), "id = {value}");
        }
    }

    #[test]
    fn entity_id_strategy_rejects_unknown_and_malformed_values() {
        for value in [
            r#""Provided""#,
            r#""UUID""#,
            r#""random""#,
            r#""""#,
            r#""slug""#,
            r#""slug()""#,
            r#""slug(title""#,
            r#""slug title""#,
            r#""slug( title )""#,
            r#""slug(title, summary)""#,
            r#""slug(1title)""#,
            r#""slugify(title)""#,
            "uuid",
            "1",
        ] {
            let err = entity_with_id(value).expect_err(value);
            assert!(err.contains("entity `Task` in task.rs"), "id = {value}: the error names the entity: {err}");
            assert!(
                err.contains(r#"expected `id = "provided"`, `id = "uuid"` or `id = "slug(<field>)"`"#),
                "id = {value}: the error lists the accepted forms: {err}"
            );
        }
    }

    #[test]
    fn entity_id_slug_must_name_a_plain_string_field() {
        let err = entity_with_id(r#""slug(missing)""#).unwrap_err();
        assert!(err.contains("entity `Task`") && err.contains("has no field `missing`"), "{err}");

        for field in ["summary", "points"] {
            let err = entity_with_id(&format!(r#""slug({field})""#)).unwrap_err();
            assert!(
                err.contains("entity `Task`") && err.contains(&format!("field `{field}` must be a plain String")),
                "{err}"
            );
        }
    }

    #[test]
    fn parse_frontmatter_name() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity, directory = "tasks")]
            pub struct Task {
                #[ontology(id)]
                pub id: String,

                #[ontology(frontmatter_name = "task_status")]
                pub status: String,

                #[ontology(relation(belongs_to, target = "Task"), frontmatter_name = "parent")]
                pub parent_id: Option<String>,

                pub title: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        let field = |name: &str| entities[0].fields.iter().find(|f| f.name == name).unwrap();
        assert_eq!(field("status").frontmatter_name.as_deref(), Some("task_status"));
        assert_eq!(field("status").frontmatter_key(), "task_status");
        assert_eq!(field("parent_id").frontmatter_key(), "parent");
        assert!(matches!(field("parent_id").role, FieldRole::Relation(_)), "the rename leaves the role alone");
        assert_eq!(field("title").frontmatter_name, None);
        assert_eq!(field("title").frontmatter_key(), "title");
    }

    #[test]
    fn frontmatter_name_must_be_a_plain_yaml_key() {
        for bad in ["", "1st", "task status", "task:status", "-lead", "k\u{e9}y", "a.b"] {
            let source = format!(
                r#"
                #[derive(OntologyEntity)]
                #[ontology(entity)]
                pub struct Task {{
                    #[ontology(id)]
                    pub id: String,
                    #[ontology(frontmatter_name = "{bad}")]
                    pub status: String,
                }}
                "#
            );
            let err = parse_schema_source(&source, Path::new("test.rs")).unwrap_err();
            assert!(
                err.contains("entity `Task` in test.rs: field `status`: invalid frontmatter_name"),
                "{bad:?}: {err}"
            );
        }
        for good in ["task_status", "_private", "kebab-key", "Key2", "nothing", "online", "Truest"] {
            assert!(validate_frontmatter_key(good).is_ok(), "{good:?} must be accepted");
        }
    }

    #[test]
    fn frontmatter_name_must_not_be_a_yaml_bool_or_null_word() {
        for bad in
            ["true", "True", "TRUE", "false", "False", "null", "Null", "NULL", "yes", "No", "ON", "off", "y", "N"]
        {
            let err = validate_frontmatter_key(bad).unwrap_err();
            assert!(err.contains("YAML reads it as a bool or null"), "{bad:?}: {err}");
        }
        // `~` is null too, already outside the key grammar.
        assert!(validate_frontmatter_key("~").is_err());
    }

    #[test]
    fn frontmatter_name_must_be_a_string_literal() {
        let source = r#"
            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Task {
                #[ontology(id)]
                pub id: String,
                #[ontology(frontmatter_name = 3)]
                pub status: String,
            }
        "#;
        let err = parse_schema_source(source, Path::new("test.rs")).unwrap_err();
        assert!(err.contains("string literal"), "{err}");
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
        // `cargo:warning` diagnostic rather than panicking - the rest of the struct
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

        // The malformed attr is ignored - `name` still shows up as a plain field.
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

    /// Parse the embedded fixture schema directory and verify all entities are
    /// found with correct metadata.
    #[test]
    fn parse_all_real_schemas() {
        let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("failed to parse schema dir");
        let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();

        // All 4 fixture entities must be present
        let expected = ["Tag", "Exercise", "Workout", "WorkoutSet"];
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

        // Tag - simple entity, no relations
        let tag = find("Tag");
        assert_eq!(tag.directory, "tag");
        assert_eq!(tag.table, "tags");
        assert!(tag.id_field().is_some());
        assert_eq!(tag.id_field().unwrap().field_type, FieldType::String);
        assert_eq!(tag.relation_fields().count(), 0);

        // Exercise - simple entity with Option<String>, no relations
        let ex = find("Exercise");
        assert_eq!(ex.directory, "exercise");
        assert_eq!(ex.table, "exercises");
        assert_eq!(ex.relation_fields().count(), 0);
        let notes = ex.fields.iter().find(|f| f.name == "notes").unwrap();
        assert_eq!(notes.field_type, FieldType::OptionString);

        // Workout - 1 belongs_to (parent_id, self-ref) + 1 many_to_many (tags) + body field
        let w = find("Workout");
        assert_eq!(w.directory, "workout");
        assert_eq!(w.table, "workouts");
        assert!(w.body_field().is_some(), "Workout should have a body field");
        assert_eq!(w.body_field().unwrap().name, "notes");
        assert_eq!(w.belongs_to_relations().count(), 1, "Workout should have 1 belongs_to");
        assert_eq!(w.junction_relations().count(), 1, "Workout should have 1 many_to_many");

        // Multiline list flag on `tags`
        let tags = w.fields.iter().find(|f| f.name == "tags").unwrap();
        assert!(tags.multiline_list, "Workout.tags should be multiline_list");

        // WorkoutSet - 2 belongs_to (workout_id, exercise_id), several i32 fields
        let ws = find("WorkoutSet");
        assert_eq!(ws.directory, "workout_set");
        assert_eq!(ws.table, "workout_sets");
        assert_eq!(ws.belongs_to_relations().count(), 2, "WorkoutSet should have 2 belongs_to");
        let reps = ws.fields.iter().find(|f| f.name == "reps").unwrap();
        assert_eq!(reps.field_type, FieldType::I32);
        let rpe = ws.fields.iter().find(|f| f.name == "rpe").unwrap();
        assert_eq!(rpe.field_type, FieldType::OptionI32);
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

    #[test]
    fn parse_float_field_types() {
        let source = r#"
            use ontogen_macros::OntologyEntity;

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Measurement {
                #[ontology(id)]
                pub id: String,

                pub temperature: f32,
                pub humidity: Option<f32>,
                pub pressure: f64,
                pub altitude: Option<f64>,

                #[ontology(body)]
                pub body: String,
            }
        "#;

        let entities = parse_schema_source(source, Path::new("test.rs")).unwrap();
        assert_eq!(entities.len(), 1);
        let m = &entities[0];

        let temperature = m.fields.iter().find(|f| f.name == "temperature").unwrap();
        assert_eq!(temperature.field_type, FieldType::F32);

        let humidity = m.fields.iter().find(|f| f.name == "humidity").unwrap();
        assert_eq!(humidity.field_type, FieldType::OptionF32);

        let pressure = m.fields.iter().find(|f| f.name == "pressure").unwrap();
        assert_eq!(pressure.field_type, FieldType::F64);

        let altitude = m.fields.iter().find(|f| f.name == "altitude").unwrap();
        assert_eq!(altitude.field_type, FieldType::OptionF64);

        // Ensure none fell through to Other(...)
        for f in &m.fields {
            assert!(
                !matches!(f.field_type, FieldType::Other(_)),
                "field {} should not be Other(...): {:?}",
                f.name,
                f.field_type
            );
        }
    }

    #[test]
    fn serde_attrs_that_change_the_shape_are_rejected_on_entities() {
        for key in SERDE_SHAPE_ATTRS {
            let attr = match key {
                "flatten" | "skip" | "skip_serializing" => key.to_string(),
                _ => format!("{key} = \"x\""),
            };
            for (struct_attr, field_attr) in
                [(format!("#[serde({attr})]"), String::new()), (String::new(), format!("#[serde({attr})]"))]
            {
                let src = format!(
                    "#[derive(OntologyEntity, Serialize)]\n#[ontology(entity)]\n{struct_attr}\npub struct Note {{\n\
                     #[ontology(id)]\npub id: String,\n{field_attr}\npub title: String,\n}}"
                );
                let err = parse_schema_source(&src, Path::new("note.rs")).unwrap_err();
                assert!(err.contains(&format!("`#[serde({key})]`")) && err.contains("entity `Note`"), "{key}: {err}");
            }
        }
        let err = parse_schema_source(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Note { #[ontology(id)] pub id: String, \
             #[serde(default, rename(serialize = \"t\"))] pub title: String }",
            Path::new("note.rs"),
        )
        .unwrap_err();
        assert!(err.contains("field `title`") && err.contains("`#[serde(rename)]`"), "{err}");
    }

    #[test]
    fn serde_default_and_deserialize_with_are_allowed_on_entities() {
        let src = r#"
            #[derive(OntologyEntity)]
            #[ontology(entity)]
            #[serde(deny_unknown_fields)]
            pub struct Note {
                #[ontology(id)]
                pub id: String,
                #[serde(default, deserialize_with = "lenient")]
                pub title: String,
            }

            #[derive(Serialize)]
            #[serde(rename_all = "camelCase")]
            pub struct NotAnEntity {
                #[serde(rename = "x")]
                pub field: String,
            }
        "#;
        let entities = parse_schema_source(src, Path::new("note.rs")).expect("allowed attrs parse");
        assert_eq!(entities.len(), 1);
        assert!(entities[0].fields[1].serde_default);
    }

    #[test]
    fn an_entity_whose_snake_name_cannot_be_raw_is_refused() {
        for (name, snake) in [("Crate", "crate"), ("Super", "super")] {
            let src = format!(
                "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct {name} {{\n    #[ontology(id)]\n    pub id: String,\n}}\n"
            );
            let err = parse_schema_source(&src, Path::new("bad.rs")).expect_err("refused");
            assert!(
                err.contains(&format!("entity `{name}` in bad.rs: its snake_case name `{snake}` is a Rust keyword")),
                "{err}"
            );
            assert!(err.contains("rename the entity"), "{err}");
        }
        let src = "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Match {\n    #[ontology(id)]\n    pub id: String,\n}\n";
        parse_schema_source(src, Path::new("ok.rs")).expect("a keyword that can be raw is accepted");
    }

    #[test]
    fn a_keyword_foreign_key_is_spelled_as_its_field() {
        let entities = crate::schema::hostile_entities();
        let doc = entities.iter().find(|e| e.name == "Doc").expect("Doc");
        let (_, info) = doc.has_many_relations().next().expect("children");
        assert_eq!(info.foreign_key.as_deref(), Some("r#in"), "`foreign_key = \"in\"` names the field `r#in`");
    }
}
