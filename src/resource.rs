//! The JSON:API resource model (wire contract §5.2–§5.4, §9.1): for each
//! entity, the `type` it is served under, which fields are attributes and
//! which are relationships, and the relationships its module's junction ops
//! define.
//!
//! The servers and clients stages each build it from the same schema, so the
//! Axum handlers and the TypeScript transport agree on every member name.

use ontogen_core::ir::OpKind;
use ontogen_core::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationKind};
use ontogen_core::naming::to_snake_case;

use crate::servers::NamingConfig;
use crate::servers::classify::classify_op;
use crate::servers::parse::{ApiFn, ApiModule};

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
pub(crate) struct Attribute {
    /// The member name: the field name as serde writes it ([`member_name`]).
    pub name: String,
    /// The Rust field name as declared (`r#type` keeps its prefix).
    pub field: String,
}

/// A relation field served under `relationships`.
#[derive(Debug, Clone)]
pub(crate) struct Relationship {
    /// The member name and `/relationships/{rel}` segment: a `belongs_to`
    /// field loses an `_id` suffix (`epic_id` → `epic`), every other relation
    /// field keeps its name.
    pub name: String,
    /// The Rust field holding the linkage.
    pub field: String,
    pub arity: Arity,
    /// The target entity's Rust name (`Epic`).
    pub target_entity: String,
    /// The target entity's API module (`epic`).
    pub target_module: String,
    /// The target's resource `type` (`epics`).
    pub target_type: String,
}

/// A to-many relationship a resource module's junction ops define (§9.1):
/// `list_X(parent)`, with `add_Y(parent, child)` and/or
/// `remove_Y(parent, child)` beside it.
///
/// It is derived per API module rather than stored on [`Resource`], because
/// the ops live in the module, not on the entity: there is no field behind
/// it, so it never carries linkage in a resource object.
#[derive(Debug, Clone)]
pub(crate) struct JunctionRelationship<'a> {
    /// `X` of `list_X`, verbatim (`labels`, `sub_tasks`): the member name
    /// and the `{rel}` URL segment.
    pub name: String,
    /// The target entity's API module (`tag`).
    pub target_module: String,
    /// The target's resource `type` (`tags`).
    pub target_type: String,
    /// `list_X`: reads the members.
    pub list: &'a ApiFn,
    /// `add_Y`, when the module defines it; without it a `POST` to the
    /// relationship is refused.
    pub add: Option<&'a ApiFn>,
    /// `remove_Y`, when the module defines it; without it a `DELETE` to the
    /// relationship is refused.
    pub remove: Option<&'a ApiFn>,
    /// `list` returns `Vec<Target>` (true) or `Vec<String>` ids (false).
    pub lists_entities: bool,
}

impl<'a> JunctionRelationship<'a> {
    /// The ops defining this relationship: `list_X`, then `add_Y` and
    /// `remove_Y` when the module has them.
    pub fn ops(&self) -> impl Iterator<Item = &'a ApiFn> {
        [Some(self.list), self.add, self.remove].into_iter().flatten()
    }

    /// Whether `f` is one of [`Self::ops`]. Matched by name, which is unique
    /// within a module.
    pub fn defines(&self, f: &ApiFn) -> bool {
        self.ops().any(|g| g.name == f.name)
    }
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

    /// The resource served by API module `module`. A module is a resource
    /// module exactly when this is `Some`.
    pub fn by_module(&self, module: &str) -> Option<&Resource> {
        self.resources.iter().find(|r| r.module == module)
    }

    /// The resource op `f` of API module `m` is served as over HTTP, or
    /// `None` when the op keeps a route of its own.
    ///
    /// Every CRUD op of a resource module is served as its resource (§5.1),
    /// a `list` that takes a filter included: its filter is read from the
    /// `filter[…]` family (§7.3). The server and both TypeScript HTTP clients
    /// decide with this one predicate, so a route and the call that reaches
    /// it always agree.
    pub fn serving(&self, m: &ApiModule, f: &ApiFn) -> Option<&Resource> {
        let resource = self.by_module(&m.name)?;
        match classify_op(m, f) {
            OpKind::List | OpKind::GetById | OpKind::Create | OpKind::Update | OpKind::Delete => Some(resource),
            _ => None,
        }
    }

    /// The `get_by_id` resource module `m` serves as its resource, if any. A
    /// relationship route reads its parent with it and is served in its
    /// scope, and a relationship to the module's type reads each related
    /// resource with it.
    pub fn get_by_id<'a>(&self, m: &'a ApiModule) -> Option<&'a ApiFn> {
        m.functions.iter().find(|f| self.serving(m, f).is_some() && classify_op(m, f) == OpKind::GetById)
    }

    /// True when resource module `m` serves `get_by_id` as its resource.
    pub fn serves_get_by_id(&self, m: &ApiModule) -> bool {
        self.get_by_id(m).is_some()
    }

    /// Whether resource module `m` serves relationship and related routes
    /// (§9): it serves `get_by_id` as its resource and has at least one
    /// relationship, field or junction. Relationship `links` are emitted
    /// exactly when this holds, since a server must serve every link it
    /// emits.
    ///
    /// A junction op of a resource module either defines a relationship or
    /// is an error of [`ResourceModel::junctions`], so having one is having
    /// a junction relationship. Asking that, rather than building them,
    /// keeps this total for a build that never checks the junction rules
    /// (one with no HTTP server or client).
    pub fn serves_relationships(&self, m: &ApiModule) -> bool {
        let Some(resource) = self.by_module(&m.name) else { return false };
        let has_junction = m.functions.iter().any(|f| {
            matches!(
                classify_op(m, f),
                OpKind::JunctionList { .. } | OpKind::JunctionAdd { .. } | OpKind::JunctionRemove { .. }
            )
        });
        self.serves_get_by_id(m) && (!resource.relationships.is_empty() || has_junction)
    }

    /// The junction relationship op `f` of resource module `m` belongs to:
    /// its `list_X`, `add_Y` or `remove_Y`. `None` for any other op.
    ///
    /// # Panics
    ///
    /// When `m`'s junction relationships do not build: callers run after
    /// `check_http_ops`, which refuses such a module.
    pub fn junction_of<'a>(&self, m: &'a ApiModule, f: &ApiFn) -> Option<JunctionRelationship<'a>> {
        self.junctions(m)
            .expect("`check_http_ops` refuses a module whose junction relationships do not build")
            .into_iter()
            .find(|j| j.defines(f))
    }

    /// The junction-op relationships of resource module `m` (§9.1), in the
    /// declaration order of their `list_X`; empty for a module that is not a
    /// resource module, whose junction ops stay custom ops (§10.4).
    ///
    /// # Errors
    ///
    /// The first junction op, named `module::fn`, that cannot define a
    /// relationship: an `add_Y` or `remove_Y` with no `list_X`; a `list_X`
    /// whose return type is not `Vec<Target>` or `Vec<String>`, or whose
    /// target names no entity; an argument that is not a string id; a name
    /// that is not a legal member name or collides with another member of
    /// the resource; or a module that serves no `get_by_id`.
    pub fn junctions<'a>(&self, m: &'a ApiModule) -> Result<Vec<JunctionRelationship<'a>>, String> {
        let Some(resource) = self.by_module(&m.name) else { return Ok(Vec::new()) };
        // Each junction op with its kind and the child segment that groups
        // it with its partners.
        let ops: Vec<(&ApiFn, Junction, String)> = m
            .functions
            .iter()
            .filter_map(|f| match classify_op(m, f) {
                OpKind::JunctionList { child_segment } => Some((f, Junction::List, child_segment)),
                OpKind::JunctionAdd { child_segment } => Some((f, Junction::Add, child_segment)),
                OpKind::JunctionRemove { child_segment } => Some((f, Junction::Remove, child_segment)),
                _ => None,
            })
            .collect();
        let find = |kind: Junction, segment: &str| ops.iter().find(|(_, k, s)| *k == kind && s == segment);

        if let Some((orphan, _, segment)) = ops.iter().find(|(_, _, s)| find(Junction::List, s).is_none()) {
            let name = segment.replace('-', "_");
            return Err(format!(
                "ontogen: `{}::{}` is a junction op, but the module `{}` defines the relationship `{name}` only with \
                 `list_{name}(parent_id)`, which it does not have; add it, or rename the fn so it is served as a \
                 custom op",
                m.name, orphan.name, m.name
            ));
        }
        for (i, (f, kind, segment)) in ops.iter().enumerate() {
            if let Some((g, _, _)) = ops[..i].iter().find(|(_, k, s)| k == kind && s == segment) {
                return Err(format!(
                    "ontogen: `{}::{}` and `{}::{}` are both junction ops for the same relationship; keep one",
                    m.name, g.name, m.name, f.name
                ));
            }
        }

        let mut junctions: Vec<JunctionRelationship<'a>> = Vec::new();
        for (list, _, segment) in ops.iter().filter(|(_, k, _)| *k == Junction::List) {
            let name = list.name["list_".len()..].to_string();
            let (target, lists_entities) = self.junction_target(m, list, &name)?;
            let add = find(Junction::Add, segment).map(|(f, _, _)| *f);
            let remove = find(Junction::Remove, segment).map(|(f, _, _)| *f);
            for f in [Some(*list), add, remove].into_iter().flatten() {
                if let Some(p) = f.params.iter().find(|p| !is_string_id(&p.ty_ast)) {
                    return Err(format!(
                        "ontogen: `{}::{}` is a junction op of the relationship `{name}`, so it takes the parent id \
                         from the URL and the child id from the request body; `{}: {}` must be `&str`, `String` or \
                         `&String`",
                        m.name, f.name, p.name, p.ty
                    ));
                }
            }
            self.check_junction_name(m, list, &name, resource, &junctions)?;
            if !self.serves_get_by_id(m) {
                return Err(format!(
                    "ontogen: `{}::{}` defines the relationship `{name}` of the JSON:API resource `{}`, whose \
                     routes read the parent with `{}::get_by_id`, which the module does not serve; add it, or move \
                     the junction ops to a module that is not a resource module",
                    m.name, list.name, resource.resource_type, m.name
                ));
            }
            junctions.push(JunctionRelationship {
                name,
                target_module: target.module.clone(),
                target_type: target.resource_type.clone(),
                list,
                add,
                remove,
                lists_entities,
            });
        }
        Ok(junctions)
    }

    /// The target of the junction relationship `name` that `list` reads, and
    /// whether `list` returns the target's entities (rather than ids).
    fn junction_target(&self, m: &ApiModule, list: &ApiFn, name: &str) -> Result<(&Resource, bool), String> {
        let refuse =
            |why: String| format!("ontogen: `{}::{}` defines the relationship `{name}`, so {why}", m.name, list.name);
        let Some(element) = vec_element(&list.return_type_ast) else {
            return Err(refuse(format!(
                "it must return `Vec<Target>` or `Vec<String>`, where `Target` is the related entity; it returns `{}`",
                list.return_type
            )));
        };
        if let Some(target) = self.by_item_type(element) {
            return Ok((target, true));
        }
        if !matches!(element, syn::Type::Path(_)) || !is_string_id(element) {
            return Err(refuse(format!(
                "it must return `Vec<Target>` or `Vec<String>`, where `Target` is the related entity; it returns `{}`",
                list.return_type
            )));
        }
        let resource_type = name.replace('_', "-");
        self.resources.iter().find(|r| r.resource_type == resource_type).map(|r| (r, false)).ok_or_else(|| {
            refuse(format!(
                "its target is the entity served as `{resource_type}`, since it returns ids; no entity is. Return \
                 `Vec<Target>` to name the target, or rename the fn after the target's collection"
            ))
        })
    }

    /// The §5.4 member-name rules, for a junction relationship named `name`.
    fn check_junction_name(
        &self,
        m: &ApiModule,
        list: &ApiFn,
        name: &str,
        resource: &Resource,
        earlier: &[JunctionRelationship<'_>],
    ) -> Result<(), String> {
        let refuse = |why: String| {
            Err(format!(
                "ontogen: `{}::{}` defines the relationship `{name}` of the JSON:API resource `{}`, {why}; rename \
                 the junction ops",
                m.name, list.name, resource.resource_type
            ))
        };
        let entity = &resource.entity.name;
        if !is_legal_member_name(name) {
            return refuse("which is not a legal JSON:API member name (it may not start or end with `_`)".to_string());
        }
        if matches!(name, "type" | "id" | "relationships") {
            return refuse("a name JSON:API reserves".to_string());
        }
        if let Some(a) = resource.attributes.iter().find(|a| a.name == name) {
            return refuse(format!(
                "which is also the attribute `{entity}.{}`: JSON:API gives attributes and relationships one namespace",
                a.field
            ));
        }
        if let Some(r) = resource.relationships.iter().find(|r| r.name == name) {
            return refuse(format!("which is also the relationship of the field `{entity}.{}`", r.field));
        }
        if let Some(j) = earlier.iter().find(|j| j.name == name) {
            return refuse(format!("which `{}::{}` defines too", m.name, j.list.name));
        }
        Ok(())
    }

    /// The resource whose entity an event op's item type names, by its last
    /// path segment (`Task`, `crate::schema::Task`). Such an op's frames
    /// carry the resource object (§12); any other item type, `Vec<Task>`
    /// included, is not a resource.
    pub fn by_item_type(&self, ty: &syn::Type) -> Option<&Resource> {
        let syn::Type::Path(tp) = ty else { return None };
        let last = tp.path.segments.last().filter(|seg| tp.qself.is_none() && seg.arguments.is_none())?;
        self.resources.iter().find(|r| last.ident == r.entity.name)
    }
}

impl Relationship {
    pub fn is_to_many(&self) -> bool {
        self.arity == Arity::ToMany
    }
}

/// The three junction op kinds, without their child segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Junction {
    List,
    Add,
    Remove,
}

/// The element type of a `Vec<T>`.
fn vec_element(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(tp) = ty else { return None };
    let last = tp.path.segments.last().filter(|seg| tp.qself.is_none() && seg.ident == "Vec")?;
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else { return None };
    match args.args.first() {
        Some(syn::GenericArgument::Type(inner)) if args.args.len() == 1 => Some(inner),
        _ => None,
    }
}

/// True for `String`, `&String` and `&str`: the types a junction op's ids
/// are forwarded as, since the generated handlers read them as strings.
fn is_string_id(ty: &syn::Type) -> bool {
    let (ty, borrowed) = match ty {
        syn::Type::Reference(r) if r.mutability.is_none() => (&*r.elem, true),
        ty => (ty, false),
    };
    let syn::Type::Path(tp) = ty else { return false };
    let Some(seg) = tp.path.segments.last().filter(|_| tp.qself.is_none() && tp.path.segments.len() == 1) else {
        return false;
    };
    seg.arguments.is_none() && (seg.ident == "String" || (borrowed && seg.ident == "str"))
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
                    arity,
                    target_entity: target.name.clone(),
                    target_type: naming.url_plural(&target_module),
                    target_module,
                });
            }
            FieldRole::Body | FieldRole::EnumField | FieldRole::Plain => {
                attributes.push(Attribute { name: member_name(&field.name).to_string(), field: field.name.clone() });
            }
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

/// The member name serde writes for the Rust field `field`: a raw
/// identifier without its `r#` (`r#type` is written `type`).
pub(crate) fn member_name(field: &str) -> &str {
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
    use crate::servers::parse::Param;

    impl ResourceModel {
        fn by_entity(&self, entity: &str) -> Option<&Resource> {
            self.resources.iter().find(|r| r.entity.name == entity)
        }
    }

    impl Resource {
        fn relationship(&self, name: &str) -> Option<&Relationship> {
            self.relationships.iter().find(|r| r.name == name)
        }
    }

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
        assert!(model.by_module("stats").is_none());
        assert!(model.by_entity("Stats").is_none());
    }

    #[test]
    fn attributes_are_every_field_but_id_relations_and_skips_in_order() {
        let model = model(TASKS).unwrap();
        let item = model.by_entity("WorkItem").unwrap();
        let attrs: Vec<(&str, &str)> = item.attributes.iter().map(|a| (a.name.as_str(), a.field.as_str())).collect();
        assert_eq!(attrs, vec![("title", "title"), ("estimate", "estimate"), ("body", "body"), ("kind", "r#kind")]);
    }

    #[test]
    fn relationships_are_named_typed_and_ordered() {
        let model = model(TASKS).unwrap();
        let item = model.by_entity("WorkItem").unwrap();
        let rels: Vec<(&str, &str, Arity, &str, &str, &str)> = item
            .relationships
            .iter()
            .map(|r| {
                (
                    r.name.as_str(),
                    r.field.as_str(),
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
                ("epic", "epic_id", Arity::ToOne { nullable: true }, "Epic", "epic", "epics"),
                ("parent", "parent_id", Arity::ToOne { nullable: false }, "WorkItem", "work_item", "work-items"),
                ("sub_items", "sub_items", Arity::ToMany, "WorkItem", "work_item", "work-items"),
                ("related_epics", "related_epics", Arity::ToMany, "Epic", "epic", "epics"),
            ]
        );
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

    fn op(name: &str, params: &[(&str, &str)]) -> ApiFn {
        ApiFn {
            name: name.to_string(),
            params: params
                .iter()
                .map(|(name, ty)| Param {
                    name: (*name).to_string(),
                    ty: (*ty).to_string(),
                    ty_ast: syn::parse_str(ty).unwrap(),
                })
                .collect(),
            return_type: "Vec<Epic>".to_string(),
            ..ApiFn::default()
        }
    }

    fn module(name: &str, functions: Vec<ApiFn>) -> ApiModule {
        ApiModule { name: name.to_string(), functions, events: vec![], is_singleton: false, has_count: false }
    }

    #[test]
    fn every_crud_op_of_a_resource_module_is_served_as_its_resource() {
        let model = model(TASKS).unwrap();
        let page = [("limit", "Option<u64>"), ("offset", "Option<u64>")];
        let id = [("id", "&str")];
        let filtered = [("title", "&str")];
        let filtered_page = [("title", "&str"), ("limit", "Option<u64>"), ("offset", "Option<u64>")];
        for f in [
            op("list", &[]),
            op("list", &page),
            op("list", &filtered),
            op("list", &filtered_page),
            op("get_by_id", &id),
            op("create", &[("input", "CreateEpicInput")]),
            op("update", &[("id", "&str"), ("input", "UpdateEpicInput")]),
            op("delete", &id),
        ] {
            let m = module("epic", vec![f.clone()]);
            assert_eq!(model.serving(&m, &f).map(|r| r.resource_type.as_str()), Some("epics"), "{}", f.name);
        }
        let archive = op("archive", &id);
        assert!(model.serving(&module("epic", vec![archive.clone()]), &archive).is_none(), "a custom op is not");
        let list = op("list", &[]);
        assert!(model.serving(&module("stats", vec![list.clone()]), &list).is_none(), "no entity, no resource");
        assert!(op("list", &filtered_page).takes_filter());
        assert!(!op("list", &page).takes_filter() && !op("list", &[]).takes_filter());
    }

    /// `name(params) -> ret`, every param and the return type parsed.
    fn op_returning(name: &str, params: &[(&str, &str)], ret: &str) -> ApiFn {
        ApiFn { return_type: ret.to_string(), return_type_ast: syn::parse_str(ret).unwrap(), ..op(name, params) }
    }

    /// A schema for the junction rules: `Task` has an attribute `notes` and
    /// a field relationship `tags`; `Tag`, `Label` and `Epic` are targets.
    const JUNCTIONS: &str = r#"
        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Task {
            #[ontology(id)]
            pub id: String,
            pub title: String,
            pub notes: Vec<String>,
            #[ontology(relation(many_to_many, target = "Tag"))]
            pub tags: Vec<String>,
        }

        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Tag {
            #[ontology(id)]
            pub id: String,
        }

        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Label {
            #[ontology(id)]
            pub id: String,
        }

        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Epic {
            #[ontology(id)]
            pub id: String,
        }
    "#;

    fn get_by_id() -> ApiFn {
        op_returning("get_by_id", &[("id", "&str")], "Task")
    }

    fn list(name: &str, ret: &str) -> ApiFn {
        op_returning(name, &[("id", "&str")], ret)
    }

    fn write(name: &str) -> ApiFn {
        op_returning(name, &[("id", "&str"), ("child_id", "&str")], "()")
    }

    /// A junction relationship's name, target type, `lists_entities`, and
    /// whether it has an add and a remove.
    type JunctionSummary = (String, String, bool, bool, bool);

    /// The junction relationships of a `task` module with `get_by_id` and
    /// `functions`, or the error naming the first op that cannot define one.
    fn task_junctions(functions: Vec<ApiFn>) -> Result<Vec<JunctionSummary>, String> {
        let model = model(JUNCTIONS).unwrap();
        let m = module("task", [vec![get_by_id()], functions].concat());
        let junctions = model.junctions(&m)?;
        Ok(junctions
            .into_iter()
            .map(|j| {
                assert_eq!(model.by_module(&j.target_module).unwrap().resource_type, j.target_type);
                assert_eq!(j.list.name, format!("list_{}", j.name));
                (j.name, j.target_type, j.lists_entities, j.add.is_some(), j.remove.is_some())
            })
            .collect())
    }

    fn task_junction_error(functions: Vec<ApiFn>) -> String {
        task_junctions(functions).unwrap_err()
    }

    #[test]
    fn junction_ops_define_relationships_in_list_order() {
        let found = task_junctions(vec![
            list("list_labels", "Vec<Label>"),
            write("remove_label"),
            list("list_epics", "Vec<String>"),
            write("add_epic"),
            write("remove_epic"),
            list("list_sub_tasks", "Vec<crate::schema::Task>"),
            write("add_sub_task"),
        ])
        .unwrap();
        assert_eq!(
            found,
            vec![
                // Target from the element type; a list without `add_label`.
                ("labels".into(), "labels".into(), true, false, true),
                // `Vec<String>`: target from the name.
                ("epics".into(), "epics".into(), false, true, true),
                // Two words, verbatim; a qualified element type.
                ("sub_tasks".into(), "tasks".into(), true, true, false),
            ]
        );
        let model = model(JUNCTIONS).unwrap();
        let tag = module("tag", vec![list("list_labels", "Vec<Label>"), write("add_label")]);
        assert!(model.junctions(&tag).is_err(), "a resource module without `get_by_id`");
        let bookmark = module("bookmark", vec![list("list_labels", "Vec<Label>"), write("add_label")]);
        assert!(model.junctions(&bookmark).unwrap().is_empty(), "not a resource module: custom ops");
    }

    #[test]
    fn a_lone_list_defines_no_relationship() {
        assert!(task_junctions(vec![list("list_labels", "Vec<Label>")]).unwrap().is_empty());
        // Nor does one beside a two-argument custom op, or a forced add.
        assert!(task_junctions(vec![list("list_labels", "Vec<Label>"), write("tag_label")]).unwrap().is_empty());
        let forced = ApiFn { force_method: Some(crate::servers::parse::ForcedMethod::Post), ..write("add_label") };
        assert!(task_junctions(vec![list("list_labels", "Vec<Label>"), forced]).unwrap().is_empty());
    }

    #[test]
    fn an_add_or_remove_needs_its_list() {
        for name in ["add_label", "remove_label"] {
            let err = task_junction_error(vec![write(name)]);
            assert_eq!(
                err,
                format!(
                    "ontogen: `task::{name}` is a junction op, but the module `task` defines the relationship \
                     `labels` only with `list_labels(parent_id)`, which it does not have; add it, or rename the fn \
                     so it is served as a custom op"
                )
            );
        }
    }

    #[test]
    fn two_writes_of_one_kind_may_not_share_a_relationship() {
        // `bus` and `buse` both pluralize to `buses`.
        let err = task_junction_error(vec![list("list_buses", "Vec<Label>"), write("add_bus"), write("add_buse")]);
        assert_eq!(
            err,
            "ontogen: `task::add_bus` and `task::add_buse` are both junction ops for the same relationship; keep one"
        );
    }

    #[test]
    fn a_junction_list_returns_its_targets_or_their_ids() {
        for ret in ["Label", "Vec<Widget>", "Vec<&str>", "Vec<Vec<String>>", "Option<Vec<Label>>", "()"] {
            let err = task_junction_error(vec![list("list_labels", ret), write("add_label")]);
            assert_eq!(
                err,
                format!(
                    "ontogen: `task::list_labels` defines the relationship `labels`, so it must return \
                     `Vec<Target>` or `Vec<String>`, where `Target` is the related entity; it returns `{ret}`"
                ),
                "{ret}"
            );
        }
        let err = task_junction_error(vec![list("list_widgets", "Vec<String>"), write("add_widget")]);
        assert_eq!(
            err,
            "ontogen: `task::list_widgets` defines the relationship `widgets`, so its target is the entity served \
             as `widgets`, since it returns ids; no entity is. Return `Vec<Target>` to name the target, or rename \
             the fn after the target's collection"
        );
    }

    #[test]
    fn junction_ops_take_string_ids() {
        for ty in ["&str", "String", "&String"] {
            let found = task_junctions(vec![
                op_returning("list_labels", &[("id", ty)], "Vec<Label>"),
                op_returning("add_label", &[("id", ty), ("label_id", ty)], "()"),
            ]);
            assert!(found.is_ok(), "{ty}: {found:?}");
        }
        for (f, bad) in [
            (op_returning("list_labels", &[("id", "u64")], "Vec<Label>"), "id: u64"),
            (op_returning("add_label", &[("id", "&str"), ("label_id", "Uuid")], "()"), "label_id: Uuid"),
            (op_returning("remove_label", &[("id", "&mut String"), ("label_id", "&str")], "()"), "id: &mut String"),
            (op_returning("remove_label", &[("id", "str"), ("label_id", "&str")], "()"), "id: str"),
        ] {
            let name = f.name.clone();
            let mut functions = vec![list("list_labels", "Vec<Label>"), write("add_label")];
            functions.retain(|g| g.name != name);
            functions.push(f);
            let err = task_junction_error(functions);
            assert_eq!(
                err,
                format!(
                    "ontogen: `task::{name}` is a junction op of the relationship `labels`, so it takes the parent \
                     id from the URL and the child id from the request body; `{bad}` must be `&str`, `String` or \
                     `&String`"
                ),
                "{name}"
            );
        }
    }

    #[test]
    fn a_junction_relationship_name_follows_the_member_name_rules() {
        let err = task_junction_error(vec![list("list_tags", "Vec<Tag>"), write("add_tag")]);
        assert_eq!(
            err,
            "ontogen: `task::list_tags` defines the relationship `tags` of the JSON:API resource `tasks`, which is \
             also the relationship of the field `Task.tags`; rename the junction ops"
        );
        let err = task_junction_error(vec![list("list_notes", "Vec<Label>"), write("add_note")]);
        assert_eq!(
            err,
            "ontogen: `task::list_notes` defines the relationship `notes` of the JSON:API resource `tasks`, which is \
             also the attribute `Task.notes`: JSON:API gives attributes and relationships one namespace; rename the \
             junction ops"
        );
        let err = task_junction_error(vec![list("list_relationships", "Vec<Label>"), write("add_relationship")]);
        assert_eq!(
            err,
            "ontogen: `task::list_relationships` defines the relationship `relationships` of the JSON:API resource \
             `tasks`, a name JSON:API reserves; rename the junction ops"
        );

        // Names no pair of fn names can reach, checked directly.
        let model = model(JUNCTIONS).unwrap();
        let task = model.by_module("task").unwrap();
        let m = module("task", vec![]);
        let labels = list("list_labels", "Vec<Label>");
        for (name, why) in [
            ("_x", "which is not a legal JSON:API member name"),
            ("x_", "which is not a legal JSON:API member name"),
            ("type", "a name JSON:API reserves"),
            ("id", "a name JSON:API reserves"),
        ] {
            let err = model.check_junction_name(&m, &labels, name, task, &[]).unwrap_err();
            assert!(err.contains(why), "{name}: {err}");
        }
        let with_labels = module("task", vec![get_by_id(), labels.clone(), write("add_label")]);
        let earlier = model.junctions(&with_labels).unwrap();
        let err = model.check_junction_name(&m, &labels, "labels", task, &earlier).unwrap_err();
        assert!(err.contains("which `task::list_labels` defines too"), "{err}");
    }

    #[test]
    fn a_module_with_junction_relationships_serves_get_by_id() {
        let model = model(JUNCTIONS).unwrap();
        let m = module("task", vec![list("list_labels", "Vec<Label>"), write("add_label")]);
        assert_eq!(
            model.junctions(&m).unwrap_err(),
            "ontogen: `task::list_labels` defines the relationship `labels` of the JSON:API resource `tasks`, whose \
             routes read the parent with `task::get_by_id`, which the module does not serve; add it, or move the \
             junction ops to a module that is not a resource module"
        );
        // A forced `get_by_id` is not served as the resource.
        let forced = ApiFn { force_method: Some(crate::servers::parse::ForcedMethod::Post), ..get_by_id() };
        let m = module("task", vec![forced, list("list_labels", "Vec<Label>"), write("add_label")]);
        assert!(model.junctions(&m).is_err());
    }

    #[test]
    fn a_module_serves_relationships_with_get_by_id_and_a_relationship() {
        let model = model(JUNCTIONS).unwrap();
        // A field relationship.
        assert!(model.serves_relationships(&module("task", vec![get_by_id()])));
        assert!(!model.serves_relationships(&module("task", vec![op_returning("list", &[], "Vec<Task>")])));
        // No relationship at all.
        let tag = op_returning("get_by_id", &[("id", "&str")], "Tag");
        assert!(!model.serves_relationships(&module("tag", vec![tag.clone()])));
        // A junction relationship only.
        let junction = vec![tag, list("list_labels", "Vec<Label>"), write("add_label")];
        assert!(model.serves_relationships(&module("tag", junction.clone())));
        // Not a resource module.
        assert!(!model.serves_relationships(&module("bookmark", junction)));
    }

    #[test]
    fn check_http_ops_raises_the_junction_rules() {
        let model = model(JUNCTIONS).unwrap();
        let modules = [module("task", vec![get_by_id(), list("list_tags", "Vec<Tag>"), write("add_tag")])];
        let err = crate::servers::classify::check_http_ops(&modules, &model).unwrap_err();
        assert!(err.starts_with("ontogen: `task::list_tags` defines the relationship `tags`"), "{err}");
    }

    #[test]
    fn every_relationship_target_serves_get_by_id() {
        let model = model(JUNCTIONS).unwrap();
        let target =
            |entity: &str| module(&entity.to_lowercase(), vec![op_returning("get_by_id", &[("id", "&str")], entity)]);
        let task = module("task", vec![get_by_id(), list("list_labels", "Vec<Label>"), write("add_label")]);
        let check = |modules: &[ApiModule]| crate::servers::classify::check_http_ops(modules, &model);

        check(&[task.clone(), target("Tag"), target("Label")]).unwrap();
        assert_eq!(
            check(&[task.clone(), target("Label")]).unwrap_err(),
            "ontogen: `task::get_by_id` serves the relationship `tags` of the JSON:API resource `tasks`, whose \
             related link reads every `tags` it links with `tag::get_by_id`, which no module serves"
        );
        assert_eq!(
            check(&[task.clone(), target("Tag"), module("label", vec![])]).unwrap_err(),
            "ontogen: `task::list_labels` serves the relationship `labels` of the JSON:API resource `tasks`, whose \
             related link reads every `labels` it links with `label::get_by_id`, which no module serves"
        );
        // A module that serves no relationship routes reads no target.
        check(&[module("task", vec![op_returning("list", &[], "Vec<Task>")])]).unwrap();
    }

    #[test]
    fn an_item_type_naming_an_entity_is_its_resource() {
        let model = model(TASKS).unwrap();
        for ty in [syn::parse_quote!(Epic), syn::parse_quote!(crate::schema::Epic)] {
            assert_eq!(model.by_item_type(&ty).map(|r| r.resource_type.as_str()), Some("epics"));
        }
        for ty in [syn::parse_quote!(Vec<Epic>), syn::parse_quote!(Option<Epic>), syn::parse_quote!(EpicChange)] {
            assert!(model.by_item_type(&ty).is_none());
        }
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
        ] {
            let entities = parse_schema_dir(&root.join(dir)).unwrap_or_else(|e| panic!("{dir}: {e}"));
            assert!(!entities.is_empty(), "{dir}: no entities");
            ResourceModel::build(&entities, &NamingConfig::default()).unwrap_or_else(|e| panic!("{dir}: {e}"));
        }
    }
}
