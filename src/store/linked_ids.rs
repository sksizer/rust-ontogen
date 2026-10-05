//! The checks both store backends emit into `create_*` and `update_*` on the
//! ids a write lists in its to-many relations (JSON:API wire contract §5.4).
//! They run before anything is written, in this order, the first failure
//! answering:
//!
//! 1. a `has_many` list that names the record itself is
//!    `{Child}ParentCycle(id)`: a record cannot be its own child;
//! 2. every listed id must name a record, relations in field declaration
//!    order and ids in list order: the first missing one is
//!    `{Target}NotFound(id)`, `has_many` children and `many_to_many` targets
//!    alike. On update only the ids the write adds are checked, those not
//!    already in the record's list;
//! 3. on update, `has_many::emit_dropped_children`'s `{Child}ParentRequired`.
//!
//! SQLite's junction foreign key refuses a missing `many_to_many` target
//! only once the record's own row is written, and a markdown vault has no
//! foreign key at all. Checking every target up front gives both backends
//! the same answer and keeps a refused write from writing anything.
//!
//! A record may already hold a `many_to_many` id with no record behind it:
//! a markdown delete leaves the links to the deleted record (ADR 0001
//! amendment 5), and a vault file can be edited by hand. A relationship
//! `POST` or `DELETE` writes the whole list back, so re-checking the ids a
//! record holds would stop it gaining or losing any link. A stored
//! `has_many` list is read from the children's foreign keys and names only
//! records that exist, so the same rule changes nothing there.

use crate::schema::model::{EntityDef, FieldRole, RelationKind};
use crate::store::has_many::has_many_writes;
use crate::store::helpers::to_snake_case;

/// Where the listed ids come from.
pub(crate) enum Listed<'a> {
    /// The record being created, bound to this variable. Its id is known
    /// only when the caller gave one; a derived id cannot be listed, since
    /// the checks run before it is derived.
    Create(&'a str),
    /// The lists an `{Entity}Update` named `updates` sets, for the record
    /// whose id is bound to `id: &str` and whose stored relations are
    /// bound to `current`. An unset list checks nothing, and an id already
    /// in `current`'s list is not checked again.
    Update,
}

impl Listed<'_> {
    /// An iterator of `&String` over the new list for `field`.
    fn ids(&self, field: &str) -> String {
        match self {
            Listed::Create(var) => format!("&{var}.{field}"),
            Listed::Update => format!("updates.{field}.iter().flatten()"),
        }
    }

    /// The condition, ending in `&& `, under which `var` from the new
    /// `field` list is checked at all.
    fn added(&self, field: &str, var: &str) -> String {
        match self {
            Listed::Create(_) => String::new(),
            Listed::Update => format!("!current.{field}.contains({var}) && "),
        }
    }
}

/// The name of the `{snake}_exists(&self, id: &str) -> Result<bool, AppError>`
/// helper each backend emits for an entity a write can list (see
/// [`needs_exists_helper`]). An id the backend could never hold is `false`,
/// not an error. It is `pub(crate)`: another entity's `create_*` and
/// `update_*` call it for their `many_to_many` targets.
pub(crate) fn exists_helper(entity_name: &str) -> String {
    format!("{}_exists", to_snake_case(entity_name))
}

/// Whether `entity` gets an exists helper: it is the child of its own
/// `has_many` write, or the target of any entity's `many_to_many`.
pub(crate) fn needs_exists_helper(entity: &EntityDef, entities: &[EntityDef]) -> bool {
    !has_many_writes(entity).is_empty()
        || entities.iter().any(|e| e.junction_relations().any(|(_, info)| info.target == entity.name))
}

/// Emit check 1: one refusal per `has_many` write whose new list names the
/// record itself. On create only an id the caller gave can be listed.
pub(crate) fn emit_self_listing_check(code: &mut String, entity: &EntityDef, listed: &Listed<'_>) {
    let writes = has_many_writes(entity);
    for hm in &writes {
        let (f, child) = (hm.field, hm.child);
        match listed {
            Listed::Create(var) => {
                code.push_str(&format!("        if !{var}.id.trim().is_empty() && {var}.{f}.contains(&{var}.id) {{\n"));
                code.push_str(&format!("            return Err(AppError::{child}ParentCycle({var}.id.clone()));\n"));
            }
            Listed::Update => {
                code.push_str(&format!(
                    "        if updates.{f}.as_ref().is_some_and(|ids| ids.iter().any(|c| c == id)) {{\n"
                ));
                code.push_str(&format!("            return Err(AppError::{child}ParentCycle(id.to_string()));\n"));
            }
        }
        code.push_str("        }\n");
    }
    if !writes.is_empty() {
        code.push('\n');
    }
}

/// Emit check 2: every id a `has_many` write or a `many_to_many` lists names
/// a record; on update, every id it adds. A `many_to_many` whose target is
/// not an entity of the schema has no store to ask, so it is not checked.
/// An id listed twice is checked twice, which is harmless.
///
/// A self-referential `many_to_many` may list the record's own id on create
/// when the caller gave it: the record exists by the time its links are
/// written, as an update would find it.
pub(crate) fn emit_listed_ids_check(
    code: &mut String,
    entity: &EntityDef,
    entities: &[EntityDef],
    listed: &Listed<'_>,
) {
    let mut any = false;
    for field in &entity.fields {
        let FieldRole::Relation(info) = &field.role else { continue };
        let f = &field.name;
        let target = &info.target;
        let exists = exists_helper(target);
        match info.kind {
            RelationKind::HasMany if info.foreign_key.is_some() => {
                code.push_str(&format!("        for child_id in {} {{\n", listed.ids(f)));
                let added = listed.added(f, "child_id");
                code.push_str(&format!("            if {added}!self.{exists}(child_id).await? {{\n"));
                code.push_str(&format!("                return Err(AppError::{target}NotFound(child_id.clone()));\n"));
            }
            RelationKind::ManyToMany if entities.iter().any(|e| &e.name == target) => {
                code.push_str(&format!("        for target_id in {} {{\n", listed.ids(f)));
                match listed {
                    Listed::Create(var) if *target == entity.name => code.push_str(&format!(
                        "            if ({var}.id.trim().is_empty() || *target_id != {var}.id) && !self.{exists}(target_id).await? {{\n"
                    )),
                    _ => code.push_str(&format!(
                        "            if {}!self.{exists}(target_id).await? {{\n",
                        listed.added(f, "target_id")
                    )),
                }
                code.push_str(&format!("                return Err(AppError::{target}NotFound(target_id.clone()));\n"));
            }
            _ => continue,
        }
        code.push_str("            }\n");
        code.push_str("        }\n");
        any = true;
    }
    if any {
        code.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{FieldDef, FieldType, RelationInfo};

    fn relation(name: &str, kind: RelationKind, target: &str, fk: Option<&str>) -> FieldDef {
        FieldDef::new(
            name,
            if kind == RelationKind::BelongsTo { FieldType::OptionString } else { FieldType::VecString },
            FieldRole::Relation(RelationInfo {
                kind,
                target: target.to_string(),
                junction: None,
                foreign_key: fk.map(str::to_string),
            }),
        )
    }

    fn entity(name: &str, fields: Vec<FieldDef>) -> EntityDef {
        let mut all = vec![FieldDef::new("id", FieldType::String, FieldRole::Id)];
        all.extend(fields);
        EntityDef {
            name: name.to_string(),
            directory: name.to_lowercase(),
            table: name.to_lowercase(),
            type_name: name.to_lowercase(),
            prefix: name.to_lowercase(),
            id_strategy: None,
            fields: all,
            doc: String::new(),
        }
    }

    /// A node with, in declaration order, a `many_to_many` to `Tag`, a
    /// self-referential `has_many`, a `many_to_many` to a type that is no
    /// entity, and a self-referential `many_to_many`.
    fn schema() -> Vec<EntityDef> {
        let node = entity(
            "Node",
            vec![
                relation("tags", RelationKind::ManyToMany, "Tag", None),
                relation("parent_id", RelationKind::BelongsTo, "Node", None),
                relation("children", RelationKind::HasMany, "Node", Some("parent_id")),
                relation("labels", RelationKind::ManyToMany, "Label", None),
                relation("links", RelationKind::ManyToMany, "Node", None),
            ],
        );
        vec![node, entity("Tag", vec![]), entity("Other", vec![])]
    }

    fn checks(listed: Listed<'_>) -> String {
        let entities = schema();
        let mut code = String::new();
        emit_self_listing_check(&mut code, &entities[0], &listed);
        emit_listed_ids_check(&mut code, &entities[0], &entities, &listed);
        code
    }

    #[test]
    fn the_checks_run_self_listing_first_then_relations_in_declaration_order() {
        let code = checks(Listed::Update);
        let at = |needle: &str| code.find(needle).unwrap_or_else(|| panic!("missing `{needle}`:\n{code}"));
        let cycle = at("if updates.children.as_ref().is_some_and(|ids| ids.iter().any(|c| c == id)) {");
        let tags = at("for target_id in updates.tags.iter().flatten() {");
        let children = at("for child_id in updates.children.iter().flatten() {");
        let links = at("for target_id in updates.links.iter().flatten() {");
        assert!(cycle < tags && tags < children && children < links, "{code}");
        for expected in [
            "return Err(AppError::NodeParentCycle(id.to_string()));",
            "if !current.tags.contains(target_id) && !self.tag_exists(target_id).await? {",
            "return Err(AppError::TagNotFound(target_id.clone()));",
            "if !current.children.contains(child_id) && !self.node_exists(child_id).await? {",
            "return Err(AppError::NodeNotFound(child_id.clone()));",
            "if !current.links.contains(target_id) && !self.node_exists(target_id).await? {",
        ] {
            assert!(code.contains(expected), "missing `{expected}`:\n{code}");
        }
        assert!(!code.contains("labels") && !code.contains("label_exists"), "Label is no entity: {code}");
        assert!(!code.contains("parent_id"), "a belongs_to lists nothing: {code}");
    }

    #[test]
    fn a_create_checks_the_record_and_lets_a_self_link_name_its_given_id() {
        let code = checks(Listed::Create("node"));
        for expected in [
            "if !node.id.trim().is_empty() && node.children.contains(&node.id) {",
            "return Err(AppError::NodeParentCycle(node.id.clone()));",
            "for target_id in &node.tags {",
            "for child_id in &node.children {",
            "if (node.id.trim().is_empty() || *target_id != node.id) && !self.node_exists(target_id).await? {",
        ] {
            assert!(code.contains(expected), "missing `{expected}`:\n{code}");
        }
        assert!(code.contains("for target_id in &node.links {"), "{code}");
        assert!(code.contains("if !self.tag_exists(target_id).await? {"), "a create checks every id: {code}");
        assert!(!code.contains("current"), "a create has no stored record: {code}");
    }

    #[test]
    fn an_exists_helper_is_needed_by_a_has_many_child_or_a_many_to_many_target() {
        let entities = schema();
        assert!(needs_exists_helper(&entities[0], &entities), "Node: has_many and self many_to_many");
        assert!(needs_exists_helper(&entities[1], &entities), "Tag: a many_to_many target, with no relations");
        assert!(!needs_exists_helper(&entities[2], &entities), "Other: nothing lists it");
        assert_eq!(exists_helper("WorkItem"), "work_item_exists");
    }

    #[test]
    fn an_entity_without_to_many_relations_gets_no_checks() {
        let entities = schema();
        let mut code = String::new();
        emit_self_listing_check(&mut code, &entities[1], &Listed::Update);
        emit_listed_ids_check(&mut code, &entities[1], &entities, &Listed::Update);
        assert!(code.is_empty(), "{code}");
    }

    #[test]
    fn the_snippets_parse_inside_create_and_update_bodies() {
        for (listed, signature) in [
            (Listed::Create("node"), "async fn create(&self, node: Node) -> Result<(), AppError> {\n"),
            (
                Listed::Update,
                "async fn update(&self, id: &str, updates: NodeUpdate) -> Result<(), AppError> {\n    let current = self.get_node(id).await?;\n",
            ),
        ] {
            let mut code = String::from(signature);
            code.push_str(&checks(listed));
            code.push_str("        Ok(())\n}\n");
            syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{code}"));
        }
    }
}
