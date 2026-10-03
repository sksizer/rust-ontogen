//! The JSON:API resource model (wire contract §5.2–§5.4): for each entity,
//! the `type` it is served under, which fields are attributes and which are
//! relationships.
//!
//! The servers and clients stages each build it from the same schema, so the
//! Axum handlers and the TypeScript transport agree on every member name.

use ontogen_core::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationKind};
use ontogen_core::naming::to_snake_case;

use crate::servers::NamingConfig;

/// Every entity of the schema as a JSON:API resource, in schema order.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResourceModel {
    resources: Vec<Resource>,
}

/// One entity served as a resource.
#[derive(Debug, Clone)]
pub(crate) struct Resource {
    pub entity: EntityDef,
    /// The API module serving the entity: the entity name in snake_case, as
    /// `gen_api` names it (`workout_set`).
    pub module: String,
    /// The resource `type`, the module's kebab-case `url_plural`
    /// (`workout-sets`). Also the collection's URL segment.
    pub resource_type: String,
    /// The `#[ontology(id)]` field, whatever it is called; on the wire it is
    /// always the `id` member.
    pub id_field: String,
    /// In declaration order.
    pub attributes: Vec<Attribute>,
    /// In declaration order.
    pub relationships: Vec<Relationship>,
}

/// A field served under `attributes`.
#[derive(Debug, Clone)]
#[allow(dead_code)] // read by the JSON:API TS emitter
pub(crate) struct Attribute {
    /// The member name: the field name without any `r#` prefix, as serde
    /// writes it.
    pub name: String,
    /// The Rust field name as declared (`r#type` keeps its prefix).
    pub field: String,
    pub field_type: FieldType,
    /// The field is an `Option` and serializes as `null` when `None`.
    pub nullable: bool,
    /// The field carries `#[serde(default)]`.
    pub serde_default: bool,
    /// The `#[ontology(body)]` field.
    pub is_body: bool,
}

/// A relation field served under `relationships`.
#[derive(Debug, Clone)]
#[allow(dead_code)] // read by the JSON:API TS emitter
pub(crate) struct Relationship {
    /// The member name and `/relationships/{rel}` segment: a `belongs_to`
    /// field loses an `_id` suffix (`epic_id` → `epic`), every other relation
    /// field keeps its name.
    pub name: String,
    /// The Rust field holding the linkage.
    pub field: String,
    pub kind: RelationKind,
    pub arity: Arity,
    /// The `has_many` foreign key on the target entity.
    pub foreign_key: Option<String>,
    /// The target entity's Rust name (`Epic`).
    pub target_entity: String,
    /// The target entity's API module (`epic`).
    pub target_module: String,
    /// The target's resource `type` (`epics`).
    pub target_type: String,
}

/// Whether a relationship's `data` is one identifier or an array of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arity {
    /// A `belongs_to`. `nullable` when the field is an `Option`: its `data`
    /// may be `null`, and a client may clear it.
    ToOne { nullable: bool },
    /// A `has_many` or `many_to_many`.
    ToMany,
}

#[allow(dead_code)] // read by the JSON:API TS emitter
impl ResourceModel {
    /// Build the model, checking the rules of contract §5.2–§5.4 that the
    /// schema parser cannot check alone.
    ///
    /// # Errors
    ///
    /// When an entity has no `String` id field, an attribute or relationship
    /// name is not a legal JSON:API member name or collides with another
    /// member, or a relation targets an entity outside `entities`.
    pub fn build(entities: &[EntityDef], naming: &NamingConfig) -> Result<Self, String> {
        let resources = entities.iter().map(|e| resource_of(e, entities, naming)).collect::<Result<_, _>>()?;
        Ok(Self { resources })
    }

    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// The resource served by API module `module`. A module is a resource
    /// module exactly when this is `Some`.
    pub fn by_module(&self, module: &str) -> Option<&Resource> {
        self.resources.iter().find(|r| r.module == module)
    }

    /// The resource for the entity named `entity` (`WorkoutSet`).
    pub fn by_entity(&self, entity: &str) -> Option<&Resource> {
        self.resources.iter().find(|r| r.entity.name == entity)
    }

    pub fn is_resource_module(&self, module: &str) -> bool {
        self.by_module(module).is_some()
    }
}

#[allow(dead_code)] // read by the JSON:API TS emitter
impl Resource {
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    pub fn relationship(&self, name: &str) -> Option<&Relationship> {
        self.relationships.iter().find(|r| r.name == name)
    }
}

impl Relationship {
    pub fn is_to_many(&self) -> bool {
        self.arity == Arity::ToMany
    }
}

/// The API module `gen_api` generates for an entity.
pub(crate) fn module_name(entity: &EntityDef) -> String {
    to_snake_case(&entity.name)
}

fn resource_of(entity: &EntityDef, entities: &[EntityDef], naming: &NamingConfig) -> Result<Resource, String> {
    let entity_name = &entity.name;
    let id = entity.id_field().ok_or_else(|| {
        format!("ontogen: entity `{entity_name}` has no `#[ontology(id)]` field; a JSON:API resource needs an id")
    })?;
    if id.field_type != FieldType::String {
        return Err(format!(
            "ontogen: the id field `{entity_name}.{}` must be a `String` to serve as a JSON:API resource id, found {:?}",
            id.name, id.field_type
        ));
    }

    let module = module_name(entity);
    let resource_type = naming.url_plural(&module);

    let mut attributes = Vec::new();
    let mut relationships: Vec<Relationship> = Vec::new();
    for field in &entity.fields {
        match &field.role {
            FieldRole::Id | FieldRole::Skip => {}
            FieldRole::Relation(info) => {
                let target = entities.iter().find(|e| e.name == info.target).ok_or_else(|| {
                    format!(
                        "ontogen: relation `{entity_name}.{}` targets `{}`, which is not an entity in the schema",
                        field.name, info.target
                    )
                })?;
                let target_module = module_name(target);
                let arity = match info.kind {
                    RelationKind::BelongsTo => Arity::ToOne { nullable: is_option(&field.field_type) },
                    RelationKind::HasMany | RelationKind::ManyToMany => Arity::ToMany,
                };
                relationships.push(Relationship {
                    name: relationship_name(field, &info.kind),
                    field: field.name.clone(),
                    kind: info.kind.clone(),
                    arity,
                    foreign_key: info.foreign_key.clone(),
                    target_entity: target.name.clone(),
                    target_type: naming.url_plural(&target_module),
                    target_module,
                });
            }
            FieldRole::Body | FieldRole::EnumField | FieldRole::Plain => attributes.push(Attribute {
                name: member_name(&field.name).to_string(),
                field: field.name.clone(),
                field_type: field.field_type.clone(),
                nullable: is_option(&field.field_type),
                serde_default: field.serde_default,
                is_body: field.role == FieldRole::Body,
            }),
        }
    }

    for a in &attributes {
        check_member_name(entity_name, "attribute", &a.name, &a.field)?;
        if a.name == "type" || a.name == "id" {
            return Err(format!(
                "ontogen: `{entity_name}.{}` would be an attribute named `{}`, which JSON:API reserves; rename the \
                 field",
                a.field, a.name
            ));
        }
    }
    for (i, r) in relationships.iter().enumerate() {
        check_member_name(entity_name, "relationship", &r.name, &r.field)?;
        if matches!(r.name.as_str(), "type" | "id" | "relationships") {
            return Err(format!(
                "ontogen: relation `{entity_name}.{}` would be a relationship named `{}`, which is reserved; rename \
                 the field",
                r.field, r.name
            ));
        }
        if let Some(a) = attributes.iter().find(|a| a.name == r.name) {
            return Err(format!(
                "ontogen: relation `{entity_name}.{}` and field `{entity_name}.{}` would both be the member `{}`; \
                 JSON:API gives attributes and relationships one namespace, so rename one",
                r.field, a.field, r.name
            ));
        }
        if let Some(other) = relationships[..i].iter().find(|o| o.name == r.name) {
            return Err(format!(
                "ontogen: relations `{entity_name}.{}` and `{entity_name}.{}` would both be the relationship `{}`; \
                 rename one",
                other.field, r.field, r.name
            ));
        }
    }

    Ok(Resource { entity: entity.clone(), module, resource_type, id_field: id.name.clone(), attributes, relationships })
}

fn relationship_name(field: &FieldDef, kind: &RelationKind) -> String {
    let name = member_name(&field.name);
    match kind {
        RelationKind::BelongsTo => name.strip_suffix("_id").unwrap_or(name).to_string(),
        RelationKind::HasMany | RelationKind::ManyToMany => name.to_string(),
    }
}

/// Serde writes a raw identifier without its `r#`.
fn member_name(field: &str) -> &str {
    field.strip_prefix("r#").unwrap_or(field)
}

fn is_option(ty: &FieldType) -> bool {
    match ty {
        FieldType::OptionString
        | FieldType::OptionEnum(_)
        | FieldType::OptionI32
        | FieldType::OptionI64
        | FieldType::OptionF32
        | FieldType::OptionF64
        | FieldType::OptionBool => true,
        FieldType::Other(rendered) => rendered.starts_with("Option <") || rendered.starts_with("Option<"),
        _ => false,
    }
}

/// JSON:API 1.1 member names: non-empty; letters, digits, `-`, `_`, space and
/// any non-ASCII character, with `-`, `_` and space not first or last.
fn is_legal_member_name(name: &str) -> bool {
    let edge_ok = |c: char| c.is_ascii_alphanumeric() || !c.is_ascii();
    let inner_ok = |c: char| edge_ok(c) || matches!(c, '-' | '_' | ' ');
    let (Some(first), Some(last)) = (name.chars().next(), name.chars().last()) else { return false };
    edge_ok(first) && edge_ok(last) && name.chars().all(inner_ok)
}

fn check_member_name(entity: &str, what: &str, name: &str, field: &str) -> Result<(), String> {
    if is_legal_member_name(name) {
        Ok(())
    } else {
        Err(format!(
            "ontogen: `{entity}.{field}` would be the {what} `{name}`, which is not a legal JSON:API member name (it \
             may not start or end with `_`); rename the field"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::schema::parse::{parse_schema_dir, parse_schema_source};

    fn model(source: &str) -> Result<ResourceModel, String> {
        let entities = parse_schema_source(source, Path::new("test.rs")).expect("schema parses");
        ResourceModel::build(&entities, &NamingConfig::default())
    }

    const TASKS: &str = r#"
        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Epic {
            #[ontology(id)]
            pub key: String,
            pub title: String,
        }

        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct WorkItem {
            #[ontology(id)]
            pub id: String,
            pub title: String,
            #[ontology(relation(belongs_to, target = "Epic"))]
            pub epic_id: Option<String>,
            #[serde(default)]
            pub estimate: Option<i32>,
            #[ontology(relation(belongs_to, target = "WorkItem"))]
            pub parent_id: String,
            #[ontology(relation(has_many, target = "WorkItem", foreign_key = "parent_id"))]
            pub sub_items: Vec<String>,
            #[ontology(relation(many_to_many, target = "Epic"))]
            pub related_epics: Vec<String>,
            #[ontology(skip)]
            pub cache: Vec<String>,
            #[ontology(body)]
            pub body: String,
            pub r#kind: Option<Vec<String>>,
        }
    "#;

    #[test]
    fn resources_carry_type_module_and_id_field() {
        let model = model(TASKS).unwrap();
        let epic = model.by_entity("Epic").unwrap();
        assert_eq!(
            (epic.module.as_str(), epic.resource_type.as_str(), epic.id_field.as_str()),
            ("epic", "epics", "key")
        );
        assert!(epic.relationships.is_empty());
        let item = model.by_module("work_item").unwrap();
        assert_eq!(item.entity.name, "WorkItem");
        assert_eq!(item.resource_type, "work-items");
        assert!(model.is_resource_module("work_item") && !model.is_resource_module("stats"));
        assert!(model.by_entity("Stats").is_none());
    }

    #[test]
    fn attributes_are_every_field_but_id_relations_and_skips_in_order() {
        let model = model(TASKS).unwrap();
        let item = model.by_entity("WorkItem").unwrap();
        let attrs: Vec<(&str, &str, bool, bool, bool)> = item
            .attributes
            .iter()
            .map(|a| (a.name.as_str(), a.field.as_str(), a.nullable, a.serde_default, a.is_body))
            .collect();
        assert_eq!(
            attrs,
            vec![
                ("title", "title", false, false, false),
                ("estimate", "estimate", true, true, false),
                ("body", "body", false, false, true),
                ("kind", "r#kind", true, false, false),
            ]
        );
        assert_eq!(item.attribute("estimate").unwrap().field_type, FieldType::OptionI32);
    }

    #[test]
    fn relationships_are_named_typed_and_ordered() {
        let model = model(TASKS).unwrap();
        let item = model.by_entity("WorkItem").unwrap();
        let rels: Vec<(&str, &str, RelationKind, Arity, &str, &str, &str)> = item
            .relationships
            .iter()
            .map(|r| {
                (
                    r.name.as_str(),
                    r.field.as_str(),
                    r.kind.clone(),
                    r.arity,
                    r.target_entity.as_str(),
                    r.target_module.as_str(),
                    r.target_type.as_str(),
                )
            })
            .collect();
        assert_eq!(
            rels,
            vec![
                ("epic", "epic_id", RelationKind::BelongsTo, Arity::ToOne { nullable: true }, "Epic", "epic", "epics"),
                (
                    "parent",
                    "parent_id",
                    RelationKind::BelongsTo,
                    Arity::ToOne { nullable: false },
                    "WorkItem",
                    "work_item",
                    "work-items"
                ),
                ("sub_items", "sub_items", RelationKind::HasMany, Arity::ToMany, "WorkItem", "work_item", "work-items"),
                ("related_epics", "related_epics", RelationKind::ManyToMany, Arity::ToMany, "Epic", "epic", "epics"),
            ]
        );
        assert_eq!(item.relationship("sub_items").unwrap().foreign_key.as_deref(), Some("parent_id"));
        assert!(item.relationship("sub_items").unwrap().is_to_many());
        assert!(!item.relationship("epic").unwrap().is_to_many());
    }

    #[test]
    fn plural_overrides_reach_both_ends_of_a_relationship() {
        let entities = parse_schema_source(TASKS, Path::new("test.rs")).unwrap();
        let mut naming = NamingConfig::default();
        naming.plural_overrides.insert("epic".into(), "sagas".into());
        let model = ResourceModel::build(&entities, &naming).unwrap();
        assert_eq!(model.by_entity("Epic").unwrap().resource_type, "sagas");
        assert_eq!(model.by_entity("WorkItem").unwrap().relationship("epic").unwrap().target_type, "sagas");
    }

    #[test]
    fn a_belongs_to_without_an_id_suffix_keeps_its_name() {
        let model = model(
            r#"
            #[derive(OntologyEntity)]
            #[ontology(entity)]
            pub struct Note {
                #[ontology(id)]
                pub id: String,
                #[ontology(relation(belongs_to, target = "Note"))]
                pub parent: Option<String>,
            }
            "#,
        )
        .unwrap();
        assert_eq!(model.by_entity("Note").unwrap().relationships[0].name, "parent");
    }

    fn entity_error(fields: &str) -> String {
        model(&format!(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Note {{\n#[ontology(id)]\npub id: String,\n{fields}\n}}\n\
             #[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Tag {{\n#[ontology(id)]\npub id: String,\n}}"
        ))
        .unwrap_err()
    }

    #[test]
    fn the_id_field_must_be_a_string() {
        let err =
            model("#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Note { #[ontology(id)] pub id: i64 }")
                .unwrap_err();
        assert!(err.contains("Note.id") && err.contains("String"), "{err}");
    }

    #[test]
    fn an_entity_needs_an_id_field() {
        let err =
            model("#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Note { pub title: String }").unwrap_err();
        assert!(err.contains("Note") && err.contains("#[ontology(id)]"), "{err}");
    }

    #[test]
    fn attribute_names_must_be_legal_member_names() {
        for field in ["pub _draft: bool,", "pub draft_: bool,"] {
            let err = entity_error(field);
            assert!(err.contains("not a legal JSON:API member name"), "{field}: {err}");
        }
    }

    #[test]
    fn attributes_may_not_be_named_type_or_id() {
        let err = entity_error("pub r#type: String,");
        assert!(err.contains("`type`") && err.contains("reserves"), "{err}");
        let err = model(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Note { #[ontology(id)] pub key: String, pub id: String }",
        )
        .unwrap_err();
        assert!(err.contains("`id`") && err.contains("reserves"), "{err}");
    }

    #[test]
    fn relationship_names_may_not_be_reserved() {
        for (field, name) in [
            ("#[ontology(relation(belongs_to, target = \"Tag\"))]\npub type_id: String,", "type"),
            ("#[ontology(relation(belongs_to, target = \"Tag\"))]\npub id_id: String,", "id"),
            ("#[ontology(relation(many_to_many, target = \"Tag\"))]\npub relationships: Vec<String>,", "relationships"),
        ] {
            let err = entity_error(field);
            assert!(err.contains(&format!("relationship named `{name}`")), "{name}: {err}");
        }
    }

    #[test]
    fn a_relationship_may_not_share_an_attribute_name() {
        let err =
            entity_error("pub tag: String,\n#[ontology(relation(belongs_to, target = \"Tag\"))]\npub tag_id: String,");
        assert!(err.contains("Note.tag_id") && err.contains("Note.tag") && err.contains("one namespace"), "{err}");
    }

    #[test]
    fn two_relationships_may_not_share_a_name() {
        let err = entity_error(
            "#[ontology(relation(belongs_to, target = \"Tag\"))]\npub tag_id: String,\n\
             #[ontology(relation(many_to_many, target = \"Tag\"))]\npub tag: Vec<String>,",
        );
        assert!(
            err.contains("Note.tag_id") && err.contains("Note.tag`") && err.contains("relationship `tag`"),
            "{err}"
        );
    }

    #[test]
    fn a_relationship_name_must_be_a_legal_member_name() {
        let err = entity_error("#[ontology(relation(belongs_to, target = \"Tag\"))]\npub _tag_id: String,");
        assert!(err.contains("not a legal JSON:API member name"), "{err}");
    }

    #[test]
    fn a_relation_target_must_be_an_entity() {
        let err = entity_error("#[ontology(relation(many_to_many, target = \"Label\"))]\npub labels: Vec<String>,");
        assert!(err.contains("Note.labels") && err.contains("`Label`"), "{err}");
    }

    #[test]
    fn member_name_legality() {
        for ok in ["title", "a", "created_at", "x1", "max-weight", "名前"] {
            assert!(is_legal_member_name(ok), "{ok}");
        }
        for bad in ["", "_", "_x", "x_", "-x", "x-", "a.b", "a/b", "@x"] {
            assert!(!is_legal_member_name(bad), "{bad}");
        }
    }

    /// Every schema in the tree is served as resources, so none of them may
    /// trip a §5 rule.
    #[test]
    fn every_in_tree_schema_builds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for dir in [
            "tests/fixtures/schema",
            "examples/tasks-tracker/src/schema",
            "examples/notes-kb/src/schema",
            "examples/iron-log-md/src/schema",
            "examples/iron-log/src-tauri/src/schema",
            "crates/markdown-pilot/src/schema",
            "crates/parity/schema",
            "crates/parity/schema-provided",
        ] {
            let entities = parse_schema_dir(&root.join(dir)).unwrap_or_else(|e| panic!("{dir}: {e}"));
            assert!(!entities.is_empty(), "{dir}: no entities");
            ResourceModel::build(&entities, &NamingConfig::default()).unwrap_or_else(|e| panic!("{dir}: {e}"));
        }
    }
}
