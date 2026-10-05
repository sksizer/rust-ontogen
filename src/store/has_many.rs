//! `has_many` writes, shared by both store backends (JSON:API wire contract
//! §5.4, E0004 decision 9).
//!
//! A `has_many` list is stored on the children, as their foreign keys. An
//! update that sets the list points every listed child at the record and
//! clears the foreign key of every child it drops. When the foreign key is
//! not `Option` a child cannot be left without a parent, so the update fails
//! with `{Child}ParentRequired` before anything is written. The checks on
//! the listed ids themselves (the record listing itself, a missing child)
//! run before that one, in `linked_ids`.
//!
//! Only the self-referential shape is supported: the children are records
//! of the declaring entity, whose `set_{snake}_parent` helper rewrites them,
//! and the `foreign_key` is that entity's `belongs_to` back to itself.
//! [`validate_targets`] refuses any other `has_many` at build time.

use crate::resource::member_name;
use crate::schema::model::{EntityDef, FieldRole, FieldType, RelationKind};
use crate::store::helpers::to_snake_case;

/// Refuse a `has_many` the store cannot write.
///
/// Its target must be the declaring entity: both backends emit it against
/// the declaring entity's own records (the SeaORM set-parent would rewrite
/// the wrong table; the markdown store does not compile). Its `foreign_key`
/// must name that entity's `belongs_to` back to itself, typed `String` or
/// `Option<String>`: the generated code reads and writes the key as a parent
/// id (a wikilink in markdown), so any other field either fails to compile
/// in the generated store or stores an id the field does not hold.
pub(crate) fn validate_targets(entities: &[EntityDef]) -> Result<(), String> {
    for entity in entities {
        for (field, info) in entity.has_many_relations() {
            let at = format!("`{}.{}`", entity.name, field.name);
            if info.target != entity.name {
                return Err(format!(
                    "{at}: has_many target `{target}` is not `{entity}`. Only the self-referential has_many \
                     (target = \"{entity}\") is supported. Declare the belongs_to on `{target}` instead and list \
                     `{target}` records by that foreign key in a hand-written API function.",
                    entity = entity.name,
                    target = info.target,
                ));
            }
            validate_foreign_key(entity, &at, info.foreign_key.as_deref())?;
        }
    }
    Ok(())
}

/// Check a self-referential `has_many`'s `foreign_key` (see [`validate_targets`]).
fn validate_foreign_key(entity: &EntityDef, at: &str, fk: Option<&str>) -> Result<(), String> {
    let name = &entity.name;
    let fix = format!(
        "Set `foreign_key` to a field of `{name}` declared \
         `#[ontology(relation(belongs_to, target = \"{name}\"))]` and typed `String` or `Option<String>`, \
         e.g. `parent_id`."
    );
    let Some(fk) = fk else {
        return Err(format!("{at}: has_many has no `foreign_key`. {fix}"));
    };
    let Some(fk_field) = entity.fields.iter().find(|f| f.name == fk) else {
        return Err(format!("{at}: has_many foreign_key `{fk}` names no field of `{name}`. {fix}"));
    };
    match &fk_field.role {
        FieldRole::Relation(rel) if rel.kind == RelationKind::BelongsTo && rel.target == *name => {}
        FieldRole::Relation(rel) if rel.kind == RelationKind::BelongsTo => {
            return Err(format!(
                "{at}: has_many foreign_key `{name}.{fk}` is a belongs_to to `{target}`, not to `{name}`, so it \
                 cannot hold the parent's id. {fix}",
                target = rel.target,
            ));
        }
        _ => {
            return Err(format!("{at}: has_many foreign_key `{name}.{fk}` is not a belongs_to relation. {fix}"));
        }
    }
    if !matches!(fk_field.field_type, FieldType::String | FieldType::OptionString) {
        return Err(format!(
            "{at}: has_many foreign_key `{name}.{fk}` is not typed `String` or `Option<String>`. {fix}"
        ));
    }
    Ok(())
}

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
/// field keeps the `Option` emission.
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

/// The `{field}_{suffix}` local an update keeps per relationship field
/// (`subtasks_changed`), from the field's bare name: `r#loop` gives
/// `loop_changed`.
pub(crate) fn local(field: &str, suffix: &str) -> String {
    format!("{}_{suffix}", member_name(field))
}

/// Emit, inside `update_*` and before anything is written, the children
/// the update drops from each `has_many` list (`{field}_dropped`), and the
/// `{Child}ParentRequired` refusal where the foreign key is required.
///
/// Expects `current` (the stored record, relations populated) and
/// `updates` in scope.
pub(crate) fn emit_dropped_children(code: &mut String, writes: &[HasManyWrite<'_>]) {
    for hm in writes {
        let (f, dropped) = (hm.field, local(hm.field, "dropped"));
        code.push_str(&format!("        let {dropped}: Vec<String> = match &updates.{f} {{\n"));
        code.push_str(&format!(
            "            Some(new_ids) => current.{f}.iter().filter(|c| !new_ids.contains(c)).cloned().collect(),\n"
        ));
        code.push_str("            None => Vec::new(),\n");
        code.push_str("        };\n");
        if hm.fk_required {
            code.push_str(&format!("        if let Some(child_id) = {dropped}.first() {{\n"));
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
/// for a field, e.g. `current.subtasks`. `conn` is the connection argument
/// `set_{snake}_parent` takes first, when the backend's takes one (SeaORM's
/// transaction).
pub(crate) fn emit_update_children(
    code: &mut String,
    entity: &EntityDef,
    writes: &[HasManyWrite<'_>],
    listed: impl Fn(&str) -> String,
    conn: Option<&str>,
) {
    let snake = to_snake_case(&entity.name);
    let conn = conn.map(|c| format!("{c}, ")).unwrap_or_default();
    for hm in writes {
        let f = hm.field;
        code.push_str(&format!("        if {} {{\n", local(f, "changed")));
        code.push_str(&format!("            for child_id in {} {{\n", listed(f)));
        code.push_str(&format!(
            "                self.set_{snake}_parent({conn}child_id, {}).await?;\n",
            set_parent_arg(hm.fk_required, "id")
        ));
        code.push_str("            }\n");
        if !hm.fk_required {
            code.push_str(&format!("            for child_id in &{} {{\n", local(f, "dropped")));
            code.push_str(&format!("                self.set_{snake}_parent({conn}child_id, None).await?;\n"));
            code.push_str("            }\n");
        }
        code.push_str("        }\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{FieldDef, RelationInfo};

    fn node(fk_type: FieldType) -> EntityDef {
        EntityDef {
            name: "Node".to_string(),
            directory: "nodes".to_string(),
            table: "nodes".to_string(),
            type_name: "Node".to_string(),
            prefix: "node".to_string(),
            id_strategy: None,
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
    fn a_self_referential_has_many_is_accepted() {
        validate_targets(&[node(FieldType::OptionString)]).expect("the self-referential shape is supported");
    }

    #[test]
    fn a_cross_entity_has_many_fails_the_build() {
        let mut entity = node(FieldType::OptionString);
        if let FieldRole::Relation(info) = &mut entity.fields[2].role {
            info.target = "Leaf".to_string();
        }
        let err = validate_targets(&[entity]).expect_err("a cross-entity has_many must be refused");
        for needle in ["`Node.children`", "target `Leaf`", "self-referential", "belongs_to on `Leaf`", "hand-written"] {
            assert!(err.contains(needle), "missing {needle}: {err}");
        }
    }

    /// `node`, with its `children` has_many's foreign_key set to `fk`.
    fn node_with_fk(fk: Option<&str>) -> EntityDef {
        let mut entity = node(FieldType::OptionString);
        if let FieldRole::Relation(info) = &mut entity.fields[2].role {
            info.foreign_key = fk.map(str::to_string);
        }
        entity
    }

    fn assert_fk_refused(entity: EntityDef, what: &str) {
        let err = validate_targets(&[entity]).expect_err("the foreign key must be refused");
        for needle in ["`Node.children`", what, "relation(belongs_to, target = \"Node\")", "`Option<String>`"] {
            assert!(err.contains(needle), "missing {needle}: {err}");
        }
    }

    #[test]
    fn a_required_foreign_key_is_accepted() {
        validate_targets(&[node(FieldType::String)]).expect("a String foreign key is supported");
    }

    #[test]
    fn a_missing_foreign_key_fails_the_build() {
        assert_fk_refused(node_with_fk(None), "has no `foreign_key`");
    }

    #[test]
    fn a_foreign_key_naming_no_field_fails_the_build() {
        assert_fk_refused(node_with_fk(Some("parnet_id")), "foreign_key `parnet_id` names no field of `Node`");
    }

    #[test]
    fn a_foreign_key_that_is_not_a_belongs_to_fails_the_build() {
        let mut entity = node(FieldType::OptionString);
        entity.fields[1].role = FieldRole::Plain;
        assert_fk_refused(entity, "foreign_key `Node.parent_id` is not a belongs_to relation");

        // The has_many field itself is a relation, but not a belongs_to.
        assert_fk_refused(node_with_fk(Some("children")), "foreign_key `Node.children` is not a belongs_to relation");
    }

    #[test]
    fn a_foreign_key_to_another_entity_fails_the_build() {
        let mut entity = node(FieldType::OptionString);
        if let FieldRole::Relation(info) = &mut entity.fields[1].role {
            info.target = "Owner".to_string();
        }
        assert_fk_refused(entity, "foreign_key `Node.parent_id` is a belongs_to to `Owner`, not to `Node`");
    }

    #[test]
    fn a_foreign_key_of_another_type_fails_the_build() {
        for fk_type in [FieldType::I64, FieldType::OptionI64, FieldType::VecString] {
            assert_fk_refused(node(fk_type), "foreign_key `Node.parent_id` is not typed `String` or `Option<String>`");
        }
    }

    #[test]
    fn an_optional_foreign_key_clears_dropped_children() {
        let entity = node(FieldType::OptionString);
        let writes = has_many_writes(&entity);
        assert!(!writes[0].fk_required);

        let mut code = String::new();
        emit_dropped_children(&mut code, &writes);
        emit_update_children(&mut code, &entity, &writes, |f| format!("&current.{f}"), None);
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
        emit_update_children(&mut code, &entity, &writes, |f| format!("&current.{f}"), None);
        assert!(code.contains("return Err(AppError::NodeParentRequired(child_id.clone()));"), "{code}");
        assert!(code.contains("self.set_node_parent(child_id, id).await?;"), "{code}");
        assert!(!code.contains("None).await"), "a required foreign key is never cleared: {code}");
    }

    #[test]
    fn the_snippets_parse_inside_an_update_body() {
        for fk_type in [FieldType::OptionString, FieldType::String] {
            let entity = node(fk_type);
            let writes = has_many_writes(&entity);
            let mut code = String::from("async fn update(&self, id: &str) -> Result<(), AppError> {\n");
            emit_dropped_children(&mut code, &writes);
            code.push_str("        let children_changed = true;\n");
            emit_update_children(&mut code, &entity, &writes, |f| format!("&current.{f}"), None);
            code.push_str("        Ok(())\n}\n");
            syn::parse_file(&code).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{code}"));
        }
    }
}
