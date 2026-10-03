//! `has_many` writes, shared by both store backends (JSON:API wire contract
//! §5.4, E0004 decision 9).
//!
//! A `has_many` list is stored on the children, as their foreign keys. An
//! update that sets the list points every listed child at the record and
//! clears the foreign key of every child it drops. When the foreign key is
//! not `Option` a child cannot be left without a parent, so the update fails
//! with `{Child}ParentRequired` before anything is written. A listed child
//! that does not exist fails a create or update with `{Child}NotFound`, also
//! before anything is written.
//!
//! Like the rest of the store emission, this assumes the self-referential
//! shape: the children are records of the declaring entity, whose
//! `set_{snake}_parent` helper rewrites them.

use crate::schema::model::{EntityDef, FieldType};
use crate::store::helpers::to_snake_case;

/// One `has_many` field whose children carry a foreign key back to the
/// record.
pub(crate) struct HasManyWrite<'a> {
    /// The `Vec<String>` field on the parent, e.g. `subtasks`.
    pub field: &'a str,
    /// The foreign-key field on the child, e.g. `parent_id`.
    pub fk: &'a str,
    /// The child entity, which names the `{Child}ParentRequired` variant.
    pub child: &'a str,
    /// Whether the foreign key is a non-`Option` `String`, so a child cannot
    /// be dropped from the list.
    pub fk_required: bool,
}

/// The `has_many` fields of `entity` that write through a foreign key.
pub(crate) fn has_many_writes(entity: &EntityDef) -> Vec<HasManyWrite<'_>> {
    entity
        .has_many_relations()
        .filter_map(|(field, info)| {
            let fk = info.foreign_key.as_deref()?;
            Some(HasManyWrite { field: &field.name, fk, child: &info.target, fk_required: fk_required(entity, fk) })
        })
        .collect()
}

/// Whether the declaring entity's `fk` field is a plain `String`. A missing
/// field (a cross-entity `has_many`, whose foreign key lives elsewhere)
/// keeps the `Option` emission.
pub(crate) fn fk_required(entity: &EntityDef, fk: &str) -> bool {
    entity.fields.iter().any(|f| f.name == fk && f.field_type == FieldType::String)
}

/// The type of the `parent_id` parameter of `set_{snake}_parent`.
pub(crate) fn parent_param_type(fk_required: bool) -> &'static str {
    if fk_required { "&str" } else { "Option<&str>" }
}

/// The `parent_id` argument a create or update passes to set a child's
/// parent to `id_expr` (an `&str` expression).
pub(crate) fn set_parent_arg(fk_required: bool, id_expr: &str) -> String {
    if fk_required { id_expr.to_string() } else { format!("Some({id_expr})") }
}

/// The name of the `{snake}_exists(&self, id: &str) -> Result<bool, AppError>`
/// helper each backend emits for an entity with `has_many` writes. An id the
/// backend could never hold is `false`, not an error.
pub(crate) fn exists_helper(entity: &EntityDef) -> String {
    format!("{}_exists", to_snake_case(&entity.name))
}

/// Emit, inside `create_*` or `update_*` and before anything is written, the
/// check that every listed child exists: the first missing one, in list
/// order, is `{Child}NotFound`. A child listed twice is checked twice, which
/// is harmless. `listed` names an iterator of `&String` over the new list
/// for a field: `&subtasks` on create, `updates.subtasks.iter().flatten()`
/// on update, where an unset list checks nothing.
///
/// On update this runs before [`emit_dropped_children`], so a list that both
/// names a missing child and drops a required one is `{Child}NotFound`.
pub(crate) fn emit_missing_children_check(
    code: &mut String,
    entity: &EntityDef,
    writes: &[HasManyWrite<'_>],
    listed: impl Fn(&str) -> String,
) {
    let exists = exists_helper(entity);
    for hm in writes {
        code.push_str(&format!("        for child_id in {} {{\n", listed(hm.field)));
        code.push_str(&format!("            if !self.{exists}(child_id).await? {{\n"));
        code.push_str(&format!("                return Err(AppError::{}NotFound(child_id.clone()));\n", hm.child));
        code.push_str("            }\n");
        code.push_str("        }\n");
    }
    if !writes.is_empty() {
        code.push('\n');
    }
}

/// Emit, inside `update_*` and before anything is written, the children
/// the update drops from each `has_many` list (`{field}_dropped`), and the
/// `{Child}ParentRequired` refusal where the foreign key is required.
///
/// Expects `current` (the stored record, relations populated) and
/// `updates` in scope.
pub(crate) fn emit_dropped_children(code: &mut String, writes: &[HasManyWrite<'_>]) {
    for hm in writes {
        let f = hm.field;
        code.push_str(&format!("        let {f}_dropped: Vec<String> = match &updates.{f} {{\n"));
        code.push_str(&format!(
            "            Some(new_ids) => current.{f}.iter().filter(|c| !new_ids.contains(c)).cloned().collect(),\n"
        ));
        code.push_str("            None => Vec::new(),\n");
        code.push_str("        };\n");
        if hm.fk_required {
            code.push_str(&format!("        if let Some(child_id) = {f}_dropped.first() {{\n"));
            code.push_str(&format!(
                "            return Err(AppError::{}ParentRequired(child_id.clone()));\n",
                hm.child
            ));
            code.push_str("        }\n");
        }
    }
    if !writes.is_empty() {
        code.push('\n');
    }
}

/// Emit, inside `update_*` after the record is written, the foreign-key
/// writes for each changed `has_many` list: every listed child is pointed at
/// `id`, then every dropped child is cleared. `listed` names the new list
/// for a field, e.g. `current.subtasks`.
pub(crate) fn emit_update_children(
    code: &mut String,
    entity: &EntityDef,
    writes: &[HasManyWrite<'_>],
    listed: impl Fn(&str) -> String,
) {
    let snake = to_snake_case(&entity.name);
    for hm in writes {
        let f = hm.field;
        code.push_str(&format!("        if {f}_changed {{\n"));
        code.push_str(&format!("            for child_id in {} {{\n", listed(f)));
        code.push_str(&format!(
            "                self.set_{snake}_parent(child_id, {}).await?;\n",
            set_parent_arg(hm.fk_required, "id")
        ));
        code.push_str("            }\n");
        if !hm.fk_required {
            code.push_str(&format!("            for child_id in &{f}_dropped {{\n"));
            code.push_str(&format!("                self.set_{snake}_parent(child_id, None).await?;\n"));
            code.push_str("            }\n");
        }
        code.push_str("        }\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{FieldDef, FieldRole, RelationInfo, RelationKind};

    fn node(fk_type: FieldType) -> EntityDef {
        EntityDef {
            name: "Node".to_string(),
            directory: "nodes".to_string(),
            table: "nodes".to_string(),
            type_name: "Node".to_string(),
            prefix: "node".to_string(),
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new(
                    "parent_id",
                    fk_type,
                    FieldRole::Relation(RelationInfo {
                        kind: RelationKind::BelongsTo,
                        target: "Node".to_string(),
                        junction: None,
                        foreign_key: None,
                    }),
                ),
                FieldDef::new(
                    "children",
                    FieldType::VecString,
                    FieldRole::Relation(RelationInfo {
                        kind: RelationKind::HasMany,
                        target: "Node".to_string(),
                        junction: None,
                        foreign_key: Some("parent_id".to_string()),
                    }),
                ),
            ],
            doc: String::new(),
        }
    }

    #[test]
    fn an_optional_foreign_key_clears_dropped_children() {
        let entity = node(FieldType::OptionString);
        let writes = has_many_writes(&entity);
        assert!(!writes[0].fk_required);

        let mut code = String::new();
        emit_dropped_children(&mut code, &writes);
        emit_update_children(&mut code, &entity, &writes, |f| format!("&current.{f}"));
        assert!(code.contains("let children_dropped: Vec<String> = match &updates.children {"), "{code}");
        assert!(!code.contains("ParentRequired"), "{code}");
        assert!(code.contains("self.set_node_parent(child_id, Some(id)).await?;"), "{code}");
        assert!(code.contains("for child_id in &children_dropped {"), "{code}");
        assert!(code.contains("self.set_node_parent(child_id, None).await?;"), "{code}");
    }

    #[test]
    fn a_required_foreign_key_refuses_to_drop_a_child() {
        let entity = node(FieldType::String);
        let writes = has_many_writes(&entity);
        assert!(writes[0].fk_required);

        let mut code = String::new();
        emit_dropped_children(&mut code, &writes);
        emit_update_children(&mut code, &entity, &writes, |f| format!("&current.{f}"));
        assert!(code.contains("return Err(AppError::NodeParentRequired(child_id.clone()));"), "{code}");
        assert!(code.contains("self.set_node_parent(child_id, id).await?;"), "{code}");
        assert!(!code.contains("None).await"), "a required foreign key is never cleared: {code}");
    }

    #[test]
    fn a_missing_listed_child_is_the_child_not_found() {
        let entity = node(FieldType::OptionString);
        let writes = has_many_writes(&entity);
        assert_eq!(exists_helper(&entity), "node_exists");

        let mut code = String::new();
        emit_missing_children_check(&mut code, &entity, &writes, |f| format!("updates.{f}.iter().flatten()"));
        assert!(code.contains("for child_id in updates.children.iter().flatten() {"), "{code}");
        assert!(code.contains("if !self.node_exists(child_id).await? {"), "{code}");
        assert!(code.contains("return Err(AppError::NodeNotFound(child_id.clone()));"), "{code}");
    }

    #[test]
    fn the_snippets_parse_inside_a_create_body() {
        let entity = node(FieldType::String);
        let writes = has_many_writes(&entity);
        let mut code = String::from("async fn create(&self, node: Node) -> Result<(), AppError> {\n");
        code.push_str("        let children = node.children.clone();\n");
        emit_missing_children_check(&mut code, &entity, &writes, |f| format!("&{f}"));
        code.push_str("        Ok(())\n}\n");
        syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{code}"));
    }

    #[test]
    fn the_snippets_parse_inside_an_update_body() {
        for fk_type in [FieldType::OptionString, FieldType::String] {
            let entity = node(fk_type);
            let writes = has_many_writes(&entity);
            let mut code = String::from("async fn update(&self, id: &str) -> Result<(), AppError> {\n");
            emit_missing_children_check(&mut code, &entity, &writes, |f| format!("updates.{f}.iter().flatten()"));
            emit_dropped_children(&mut code, &writes);
            code.push_str("        let children_changed = true;\n");
            emit_update_children(&mut code, &entity, &writes, |f| format!("&current.{f}"));
            code.push_str("        Ok(())\n}\n");
            syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{code}"));
        }
    }
}
