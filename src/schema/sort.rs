//! An entity's sortable fields (ADR 0006 §2).
//!
//! The store, servers and clients stages all derive their sort fields from
//! [`sort_fields`], so the store's `{Entity}SortField`, the MCP tool schema
//! and the TypeScript sort keys name the same fields in the same order.

use crate::persistence::seaorm::gen_entity::is_integer_primitive;
use crate::resource::member_name;
use crate::schema::model::{EntityDef, EnumDef, FieldDef, FieldRole, FieldType};

/// How a sortable field's values compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortKind<'a> {
    /// The `#[ontology(id)]` field, always a `String`.
    Id,
    String,
    /// Every integer width, compared numerically.
    Integer,
    /// `f32` or `f64`.
    Float,
    Bool,
    /// A schema enum, compared by its stored string, not declaration order:
    /// that is what SQL compares.
    Enum(&'a EnumDef),
}

/// One sortable field of an entity.
#[derive(Debug, Clone)]
pub(crate) struct SortFieldSpec<'a> {
    /// The `{Entity}SortField` variant: `Id` for the id field, else the
    /// PascalCase field name.
    pub variant: String,
    /// The sort key: `id` for the id field, else the serialized field name,
    /// which is the JSON:API attribute name.
    pub name: String,
    pub field: &'a FieldDef,
    pub kind: SortKind<'a>,
    /// The field is an `Option`, so `None` takes part in the order.
    pub optional: bool,
}

/// `entity`'s sortable fields: the id field first, then every other
/// sortable field in declaration order. An entity with no id field has
/// none.
pub(crate) fn sort_fields<'a>(entity: &'a EntityDef, enums: &'a [EnumDef]) -> Vec<SortFieldSpec<'a>> {
    let Some(id) = entity.id_field() else {
        return Vec::new();
    };
    let mut specs = vec![SortFieldSpec {
        variant: "Id".to_string(),
        name: "id".to_string(),
        field: id,
        kind: SortKind::Id,
        optional: false,
    }];
    for field in &entity.fields {
        if !matches!(field.role, FieldRole::Plain | FieldRole::EnumField) {
            continue;
        }
        let Some((kind, optional)) = classify(field, enums) else {
            continue;
        };
        let name = member_name(&field.name);
        specs.push(SortFieldSpec {
            variant: ontogen_core::naming::to_pascal_case(name),
            name: name.to_string(),
            field,
            kind,
            optional,
        });
    }
    specs
}

/// The sort keys `specs` accept, ascending then descending per field:
/// `id, -id, title, -title, …`. The MCP tool schema and the TypeScript
/// sort-key union enumerate these.
pub(crate) fn sort_keys(specs: &[SortFieldSpec<'_>]) -> Vec<String> {
    specs.iter().flat_map(|s| [s.name.clone(), format!("-{}", s.name)]).collect()
}

/// The sort kind of a non-id field and whether it is optional, or `None`
/// when the field is not sortable.
fn classify<'a>(field: &FieldDef, enums: &'a [EnumDef]) -> Option<(SortKind<'a>, bool)> {
    let classified = match &field.field_type {
        FieldType::String => (SortKind::String, false),
        FieldType::OptionString => (SortKind::String, true),
        FieldType::I32 | FieldType::I64 => (SortKind::Integer, false),
        FieldType::OptionI32 | FieldType::OptionI64 => (SortKind::Integer, true),
        FieldType::F32 | FieldType::F64 => (SortKind::Float, false),
        FieldType::OptionF32 | FieldType::OptionF64 => (SortKind::Float, true),
        FieldType::Bool => (SortKind::Bool, false),
        FieldType::OptionBool => (SortKind::Bool, true),
        FieldType::Other(t) | FieldType::OptionEnum(t) => {
            let optional = matches!(field.field_type, FieldType::OptionEnum(_));
            if let Some(def) = enums.iter().find(|e| &e.name == t) {
                (SortKind::Enum(def), optional)
            } else if is_integer_primitive(t) {
                (SortKind::Integer, optional)
            } else {
                return None;
            }
        }
        FieldType::VecString | FieldType::VecStruct(_) => return None,
    };
    Some(classified)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::schema::parse::{parse_schema_enums_source, parse_schema_source};

    fn parse(source: &str) -> (Vec<EntityDef>, Vec<EnumDef>) {
        let path = Path::new("schema.rs");
        let entities = parse_schema_source(source, path).expect("schema parses");
        let enums = parse_schema_enums_source(source, path).expect("enums parse");
        (entities, enums)
    }

    fn entity<'a>(entities: &'a [EntityDef], name: &str) -> &'a EntityDef {
        entities.iter().find(|e| e.name == name).expect("entity is parsed")
    }

    /// `(name, variant, kind, optional)` per spec, the enum kind by name.
    fn summary(specs: &[SortFieldSpec<'_>]) -> Vec<(String, String, String, bool)> {
        specs
            .iter()
            .map(|s| {
                let kind = match s.kind {
                    SortKind::Enum(def) => format!("Enum({})", def.name),
                    other => format!("{other:?}"),
                };
                (s.name.clone(), s.variant.clone(), kind, s.optional)
            })
            .collect()
    }

    fn row(name: &str, variant: &str, kind: &str, optional: bool) -> (String, String, String, bool) {
        (name.into(), variant.into(), kind.into(), optional)
    }

    const TASK: &str = r#"
        #[derive(OntologyEntity)]
        #[ontology(entity, directory = "tasks", table = "tasks")]
        pub struct Task {
            #[ontology(id)]
            pub id: String,
            pub title: String,
            pub status: String,
            pub created: String,
            #[ontology(relation(belongs_to, target = "Epic"))]
            pub epic_id: Option<String>,
            #[ontology(relation(many_to_many, target = "Tag"))]
            pub tags: Vec<String>,
            #[ontology(relation(belongs_to, target = "Task"))]
            pub parent_id: Option<String>,
            #[ontology(relation(has_many, target = "Task", foreign_key = "parent_id"))]
            pub subtasks: Vec<String>,
            #[ontology(body)]
            pub body: String,
        }
    "#;

    #[test]
    fn a_task_sorts_by_its_id_and_plain_fields_only() {
        let (entities, enums) = parse(TASK);
        let specs = sort_fields(entity(&entities, "Task"), &enums);
        assert_eq!(
            summary(&specs),
            vec![
                row("id", "Id", "Id", false),
                row("title", "Title", "String", false),
                row("status", "Status", "String", false),
                row("created", "Created", "String", false),
            ]
        );
        assert_eq!(specs[0].field.name, "id");
        assert_eq!(specs[1].field.name, "title");
        assert_eq!(sort_keys(&specs), ["id", "-id", "title", "-title", "status", "-status", "created", "-created"]);
    }

    #[test]
    fn the_id_field_is_named_id_whatever_it_is_called() {
        let (entities, enums) = parse(
            r#"
            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Page {
                pub title: String,
                #[ontology(id)]
                pub slug: String,
            }
            "#,
        );
        let specs = sort_fields(entity(&entities, "Page"), &enums);
        assert_eq!(summary(&specs), vec![row("id", "Id", "Id", false), row("title", "Title", "String", false)]);
        assert_eq!(specs[0].field.name, "slug");
    }

    #[test]
    fn an_entity_without_an_id_has_no_sort_fields() {
        let (entities, enums) = parse(
            r#"
            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Loose {
                pub title: String,
            }
            "#,
        );
        assert!(sort_fields(entity(&entities, "Loose"), &enums).is_empty());
    }

    #[test]
    fn every_parity_item_field_classifies_as_adr_0006_says() {
        let (entities, enums) = parse(include_str!("../../crates/parity/schema/item.rs"));
        let specs = sort_fields(entity(&entities, "Item"), &enums);
        assert_eq!(
            summary(&specs),
            vec![
                row("id", "Id", "Id", false),
                row("title", "Title", "String", false),
                row("int32", "Int32", "Integer", false),
                row("int64", "Int64", "Integer", false),
                row("float32", "Float32", "Float", false),
                row("float64", "Float64", "Float", false),
                row("flag", "Flag", "Bool", false),
                row("kind", "Kind", "Enum(Kind)", false),
                row("maybe_text", "MaybeText", "String", true),
                row("maybe_int32", "MaybeInt32", "Integer", true),
                row("maybe_int64", "MaybeInt64", "Integer", true),
                row("maybe_float32", "MaybeFloat32", "Float", true),
                row("maybe_float64", "MaybeFloat64", "Float", true),
                row("maybe_flag", "MaybeFlag", "Bool", true),
                row("maybe_kind", "MaybeKind", "Enum(Kind)", true),
                row("n_u8", "NU8", "Integer", false),
                row("n_u16", "NU16", "Integer", false),
                row("n_u32", "NU32", "Integer", false),
                row("n_u64", "NU64", "Integer", false),
                row("n_usize", "NUsize", "Integer", false),
                row("n_u128", "NU128", "Integer", false),
                row("n_i8", "NI8", "Integer", false),
                row("n_i16", "NI16", "Integer", false),
                row("n_isize", "NIsize", "Integer", false),
                row("n_i128", "NI128", "Integer", false),
                row("maybe_u32", "MaybeU32", "Integer", true),
                row("maybe_u64", "MaybeU64", "Integer", true),
            ]
        );
        let SortKind::Enum(kind) = specs[7].kind else { panic!("kind is an enum") };
        assert_eq!(
            kind.variants.iter().map(|v| v.value.as_str()).collect::<Vec<_>>(),
            ["gamma", "alpha", "beta", "delta"]
        );
    }

    #[test]
    fn vecs_skipped_fields_structs_and_unknown_types_are_not_sortable() {
        let (entities, enums) = parse(
            r#"
            pub enum Mood { Calm, Busy }

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Thing {
                #[ontology(id)]
                pub id: String,
                pub labels: Vec<String>,
                pub parts: Vec<Part>,
                pub detail: Part,
                pub maybe_detail: Option<Part>,
                #[ontology(enum_field)]
                pub mood_elsewhere: Elsewhere,
                pub count: Count,
                #[ontology(skip)]
                pub cache: String,
                pub mood: Mood,
            }
            "#,
        );
        let specs = sort_fields(entity(&entities, "Thing"), &enums);
        assert_eq!(summary(&specs), vec![row("id", "Id", "Id", false), row("mood", "Mood", "Enum(Mood)", false)]);
    }

    #[test]
    fn a_plain_field_of_a_schema_enum_type_is_sortable() {
        let (entities, enums) = parse(
            r#"
            #[serde(rename_all = "kebab-case")]
            pub enum Kind { PeerReviewed, Draft }

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Paper {
                #[ontology(id)]
                pub id: String,
                pub kind: Kind,
                pub maybe_kind: Option<Kind>,
            }
            "#,
        );
        let paper = entity(&entities, "Paper");
        assert!(paper.fields.iter().all(|f| f.role != FieldRole::EnumField), "the fixture has no enum_field");
        let specs = sort_fields(paper, &enums);
        assert_eq!(
            summary(&specs),
            vec![
                row("id", "Id", "Id", false),
                row("kind", "Kind", "Enum(Kind)", false),
                row("maybe_kind", "MaybeKind", "Enum(Kind)", true),
            ]
        );
        assert_eq!(sort_keys(&specs), ["id", "-id", "kind", "-kind", "maybe_kind", "-maybe_kind"]);
    }

    #[test]
    fn an_enum_absent_from_the_schema_enums_is_not_sortable() {
        let (entities, _) = parse(
            r#"
            pub enum Kind { Alpha }

            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Item {
                #[ontology(id)]
                pub id: String,
                #[ontology(enum_field)]
                pub kind: Kind,
                pub maybe_kind: Option<Kind>,
            }
            "#,
        );
        assert_eq!(summary(&sort_fields(entity(&entities, "Item"), &[])), vec![row("id", "Id", "Id", false)]);
    }

    #[test]
    fn a_raw_identifier_sorts_by_its_serialized_name() {
        let (entities, enums) = parse(
            r#"
            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Doc {
                #[ontology(id)]
                pub id: String,
                pub r#type: String,
            }
            "#,
        );
        let specs = sort_fields(entity(&entities, "Doc"), &enums);
        assert_eq!(summary(&specs), vec![row("id", "Id", "Id", false), row("type", "Type", "String", false)]);
        assert_eq!(specs[1].field.name, "r#type");
    }
}
