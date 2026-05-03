//! Generate CRUD methods (`list`, `get`, `create`, `update`, `delete`) and
//! `populate_relations` on `impl Store` for each entity.
//!
//! Handles three complexity tiers:
//! - **Simple** (no relations): Direct CRUD without junction sync or relation population
//! - **Junction** (many_to_many / has_many): CRUD + populate_relations + sync_junction
//! - Both tiers emit events via `self.emit_change()`

use super::helpers::{junction_source_col, junction_table_name, junction_target_col, pluralize, to_snake_case};
use crate::schema::model::EntityDef;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Generate the complete `impl Store { ... }` block with CRUD methods.
pub fn generate_crud_impl(code: &mut String, entity: &EntityDef) {
    let has_relations = entity.junction_relations().next().is_some() || entity.has_many_relations().next().is_some();

    code.push_str("impl Store {\n");

    generate_list(code, entity, has_relations);
    generate_get(code, entity, has_relations);
    generate_create(code, entity, has_relations);
    generate_update(code, entity, has_relations);
    generate_delete(code, entity);

    if has_relations {
        generate_populate_relations(code, entity);
    }

    // has_many reverse helpers (e.g., set_node_parent)
    for (_field, info) in entity.has_many_relations() {
        if let Some(ref fk) = info.foreign_key {
            generate_set_parent_helper(code, entity, fk);
        }
    }

    code.push_str("}\n");
}

// ─── Individual CRUD methods ─────────────────────────────────────────────────

fn generate_list(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let plural = pluralize(&snake);

    code.push_str(&format!(
        "    pub async fn list_{plural}(&self, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<{name}>, AppError> {{\n"
    ));
    code.push_str(&format!("        let mut query = {snake}::Entity::find();\n"));
    code.push_str("        if let Some(l) = limit {\n");
    code.push_str("            query = query.limit(l);\n");
    code.push_str("        }\n");
    code.push_str("        if let Some(o) = offset {\n");
    code.push_str("            query = query.offset(o);\n");
    code.push_str("        }\n");
    code.push_str("        let models = query\n");
    code.push_str("            .all(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n\n");

    if has_relations {
        code.push_str(&format!(
            "        let mut entities: Vec<{name}> = models.iter().map({name}::from_model).collect();\n"
        ));
        code.push_str("        for entity in &mut entities {\n");
        code.push_str(&format!("            self.populate_{snake}_relations(entity).await?;\n"));
        code.push_str("        }\n");
        code.push_str("        Ok(entities)\n");
    } else {
        code.push_str(&format!("        Ok(models.iter().map({name}::from_model).collect())\n"));
    }

    code.push_str("    }\n\n");
}

fn generate_get(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let not_found = not_found_variant(name);

    code.push_str(&format!("    pub async fn get_{snake}(&self, id: &str) -> Result<{name}, AppError> {{\n"));
    code.push_str(&format!("        let model = {snake}::Entity::find_by_id(id)\n"));
    code.push_str("            .one(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?\n");
    code.push_str(&format!("            .ok_or_else(|| AppError::{not_found}(id.to_string()))?;\n\n"));

    if has_relations {
        code.push_str(&format!("        let mut entity = {name}::from_model(&model);\n"));
        code.push_str(&format!("        self.populate_{snake}_relations(&mut entity).await?;\n"));
        code.push_str("        Ok(entity)\n");
    } else {
        code.push_str(&format!("        Ok({name}::from_model(&model))\n"));
    }

    code.push_str("    }\n\n");
}

fn generate_create(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!(
        "    pub async fn create_{snake}(&self, mut {snake}: {name}) -> Result<{name}, AppError> {{\n"
    ));

    // Hook: before_create (can modify entity or reject)
    code.push_str(&format!("        hooks::before_create(self, &mut {snake}).await?;\n\n"));

    code.push_str(&format!("        let id = {snake}.id.clone();\n"));

    // Extract junction/has_many field values before converting to active model
    let junctions: Vec<_> = entity.junction_relations().collect();
    let has_manys: Vec<_> = entity.has_many_relations().collect();

    for (field, _info) in &junctions {
        code.push_str(&format!("        let {fname} = {snake}.{fname}.clone();\n", fname = field.name));
    }
    for (field, _info) in &has_manys {
        code.push_str(&format!("        let {fname} = {snake}.{fname}.clone();\n", fname = field.name));
    }

    code.push_str(&format!("        let active = {snake}.to_active_model();\n\n"));
    code.push_str("        active\n");
    code.push_str("            .insert(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n\n");

    // Sync junction tables
    for (field, info) in &junctions {
        let jt = junction_table_name(&snake, &field.name, info.junction.as_deref());
        let src_col = junction_source_col(&snake);
        let target_snake = to_snake_case(&info.target);
        let is_self_ref = entity.name == info.target;
        let tgt_col = junction_target_col(&snake, &target_snake, is_self_ref);

        code.push_str(&format!(
            "        self.sync_junction(\"{jt}\", \"{src_col}\", \"{tgt_col}\", &id, &{fname}).await?;\n",
            fname = field.name
        ));
    }

    // Set parent_id on children (has_many reverse)
    for (field, info) in &has_manys {
        if info.foreign_key.is_some() {
            code.push_str(&format!("        for child_id in &{fname} {{\n", fname = field.name));
            code.push_str(&format!("            self.set_{snake}_parent(child_id, Some(&id)).await?;\n"));
            code.push_str("        }\n");
        }
    }

    if has_relations {
        code.push('\n');
    }

    code.push_str(&format!("        let created = self.get_{snake}(&id).await?;\n"));
    code.push_str(&format!("        self.emit_change(ChangeOp::Created, EntityKind::{entity_kind}, id);\n\n"));

    // Hook: after_create
    code.push_str("        hooks::after_create(self, &created).await?;\n");
    code.push_str("        Ok(created)\n");
    code.push_str("    }\n\n");
}

fn generate_update(code: &mut String, entity: &EntityDef, has_relations: bool) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let not_found = not_found_variant(name);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!(
        "    pub async fn update_{snake}(&self, id: &str, updates: {name}Update) -> Result<{name}, AppError> {{\n"
    ));

    // Fetch existing
    code.push_str(&format!("        let existing_model = {snake}::Entity::find_by_id(id)\n"));
    code.push_str("            .one(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?\n");
    code.push_str(&format!("            .ok_or_else(|| AppError::{not_found}(id.to_string()))?;\n\n"));

    code.push_str(&format!("        let mut current = {name}::from_model(&existing_model);\n"));

    if has_relations {
        code.push_str(&format!("        self.populate_{snake}_relations(&mut current).await?;\n\n"));
    }

    // Hook: before_update (can validate or reject)
    code.push_str("        hooks::before_update(self, &current, &updates).await?;\n\n");

    // Track which junction fields changed
    let junctions: Vec<_> = entity.junction_relations().collect();
    let has_manys: Vec<_> = entity.has_many_relations().collect();

    for (field, _info) in &junctions {
        code.push_str(&format!("        let {fname}_changed = updates.{fname}.is_some();\n", fname = field.name));
    }
    for (field, _info) in &has_manys {
        code.push_str(&format!("        let {fname}_changed = updates.{fname}.is_some();\n", fname = field.name));
    }
    if !junctions.is_empty() || !has_manys.is_empty() {
        code.push('\n');
    }

    // Apply updates
    code.push_str("        updates.apply(&mut current);\n\n");

    // Re-persist
    code.push_str("        let active = current.to_active_model();\n");
    code.push_str("        active\n");
    code.push_str("            .update(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n\n");

    // Conditional junction sync
    for (field, info) in &junctions {
        let jt = junction_table_name(&snake, &field.name, info.junction.as_deref());
        let src_col = junction_source_col(&snake);
        let target_snake = to_snake_case(&info.target);
        let is_self_ref = entity.name == info.target;
        let tgt_col = junction_target_col(&snake, &target_snake, is_self_ref);

        code.push_str(&format!("        if {fname}_changed {{\n", fname = field.name));
        code.push_str(&format!(
            "            self.sync_junction(\"{jt}\", \"{src_col}\", \"{tgt_col}\", id, &current.{fname}).await?;\n",
            fname = field.name
        ));
        code.push_str("        }\n");
    }

    // Conditional has_many reverse sync
    for (field, info) in &has_manys {
        if info.foreign_key.is_some() {
            code.push_str(&format!("        if {fname}_changed {{\n", fname = field.name));
            code.push_str(&format!("            for child_id in &current.{fname} {{\n", fname = field.name));
            code.push_str(&format!("                self.set_{snake}_parent(child_id, Some(id)).await?;\n"));
            code.push_str("            }\n");
            code.push_str("        }\n");
        }
    }

    if !junctions.is_empty() || !has_manys.is_empty() {
        code.push('\n');
    }

    code.push_str(&format!("        let result = self.get_{snake}(id).await?;\n"));
    code.push_str(&format!(
        "        self.emit_change(ChangeOp::Updated, EntityKind::{entity_kind}, id.to_string());\n\n"
    ));

    // Hook: after_update
    code.push_str("        hooks::after_update(self, &result).await?;\n");
    code.push_str("        Ok(result)\n");
    code.push_str("    }\n\n");
}

fn generate_delete(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);
    let not_found = not_found_variant(name);
    let entity_kind = entity_kind_variant(name);

    code.push_str(&format!("    pub async fn delete_{snake}(&self, id: &str) -> Result<(), AppError> {{\n"));

    // Hook: before_delete
    code.push_str("        hooks::before_delete(self, id).await?;\n\n");

    code.push_str(&format!("        let existing = {snake}::Entity::find_by_id(id)\n"));
    code.push_str("            .one(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?\n");
    code.push_str(&format!("            .ok_or_else(|| AppError::{not_found}(id.to_string()))?;\n\n"));
    code.push_str(&format!("        let active: {snake}::ActiveModel = existing.into();\n"));
    code.push_str("        active\n");
    code.push_str("            .delete(self.db())\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n\n");
    code.push_str(&format!(
        "        self.emit_change(ChangeOp::Deleted, EntityKind::{entity_kind}, id.to_string());\n\n"
    ));

    // Hook: after_delete
    code.push_str("        hooks::after_delete(self, id).await?;\n");
    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

// ─── populate_relations ──────────────────────────────────────────────────────

fn generate_populate_relations(code: &mut String, entity: &EntityDef) {
    let name = &entity.name;
    let snake = to_snake_case(name);

    code.push_str(&format!("    pub(crate) async fn populate_{snake}_relations(\n"));
    code.push_str(&format!("        &self,\n        {snake}: &mut crate::schema::{name},\n"));
    code.push_str("    ) -> Result<(), crate::schema::AppError> {\n");

    // has_many fields: load child IDs via SeaORM query
    for (field, info) in entity.has_many_relations() {
        if let Some(ref fk) = info.foreign_key {
            let target_snake = to_snake_case(&info.target);
            let fk_col = fk_to_column_enum(fk);

            code.push_str(&format!("        {snake}.{fname} = {{\n", fname = field.name,));
            code.push_str(&format!("            use crate::persistence::db::entities::{target_snake};\n"));
            code.push_str(&format!("            let children = {target_snake}::Entity::find()\n"));
            code.push_str(&format!("                .filter({target_snake}::Column::{fk_col}.eq(&{snake}.id))\n"));
            code.push_str("                .all(self.db())\n");
            code.push_str("                .await\n");
            code.push_str("                .map_err(|e| crate::schema::AppError::DbError(e.to_string()))?;\n");
            code.push_str("            children.into_iter().map(|m| m.id).collect()\n");
            code.push_str("        };\n");
        }
    }

    // many_to_many fields: load junction IDs
    for (field, info) in entity.junction_relations() {
        let jt = junction_table_name(&snake, &field.name, info.junction.as_deref());
        let src_col = junction_source_col(&snake);
        let target_snake = to_snake_case(&info.target);
        let is_self_ref = entity.name == info.target;
        let tgt_col = junction_target_col(&snake, &target_snake, is_self_ref);

        code.push_str(&format!(
            "        {snake}.{fname} = self\n            .load_junction_ids(\"{jt}\", \"{src_col}\", \"{tgt_col}\", &{snake}.id)\n            .await?;\n",
            fname = field.name,
        ));
    }

    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

// ─── has_many helper: set_parent ─────────────────────────────────────────────

fn generate_set_parent_helper(code: &mut String, entity: &EntityDef, fk: &str) {
    let snake = to_snake_case(&entity.name);
    let table = &entity.table;

    code.push_str(&format!("    async fn set_{snake}_parent(\n"));
    code.push_str("        &self,\n");
    code.push_str("        child_id: &str,\n");
    code.push_str("        parent_id: Option<&str>,\n");
    code.push_str("    ) -> Result<(), AppError> {\n");
    code.push_str("        use sea_orm::{ConnectionTrait, Value};\n");
    code.push_str("        let stmt = sea_orm::Statement::from_sql_and_values(\n");
    code.push_str("            sea_orm::DatabaseBackend::Sqlite,\n");
    code.push_str(&format!("            \"UPDATE {table} SET {fk} = ? WHERE id = ?\",\n"));
    code.push_str("            [\n");
    code.push_str("                parent_id\n");
    code.push_str("                    .map(|p| Value::from(p.to_string()))\n");
    code.push_str("                    .unwrap_or(Value::String(None)),\n");
    code.push_str("                Value::from(child_id.to_string()),\n");
    code.push_str("            ],\n");
    code.push_str("        );\n");
    code.push_str("        self.db()\n");
    code.push_str("            .execute_raw(stmt)\n");
    code.push_str("            .await\n");
    code.push_str("            .map_err(|e| AppError::DbError(e.to_string()))?;\n");
    code.push_str("        Ok(())\n");
    code.push_str("    }\n\n");
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Convert a snake_case foreign key name to its SeaORM Column enum variant.
/// E.g., `parent_id` → `ParentId`.
fn fk_to_column_enum(fk: &str) -> String {
    fk.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().collect::<String>();
                    s.push_str(chars.as_str());
                    s
                }
                None => String::new(),
            }
        })
        .collect()
}

/// Map entity name to its `AppError::*NotFound` variant.
fn not_found_variant(entity_name: &str) -> String {
    format!("{entity_name}NotFound")
}

/// Map entity name to its `EntityKind::*` variant.
fn entity_kind_variant(entity_name: &str) -> String {
    entity_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{EntityDef, FieldDef, FieldRole, FieldType, RelationInfo, RelationKind};

    fn make_role_entity() -> EntityDef {
        EntityDef {
            name: "Role".to_string(),
            directory: "role".to_string(),
            table: "roles".to_string(),
            type_name: "role".to_string(),
            prefix: "role".to_string(),
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("body", FieldType::String, FieldRole::Body),
            ],
        }
    }

    fn make_node_entity() -> EntityDef {
        EntityDef {
            name: "Node".to_string(),
            directory: "node".to_string(),
            table: "nodes".to_string(),
            type_name: "node".to_string(),
            prefix: "node".to_string(),
            fields: vec![
                FieldDef::new("id", FieldType::String, FieldRole::Id),
                FieldDef::new("name", FieldType::String, FieldRole::Plain),
                FieldDef::new(
                    "parent_id",
                    FieldType::OptionString,
                    FieldRole::Relation(RelationInfo {
                        kind: RelationKind::BelongsTo,
                        target: "Node".to_string(),
                        junction: None,
                        foreign_key: None,
                    }),
                ),
                FieldDef::new(
                    "contains",
                    FieldType::VecString,
                    FieldRole::Relation(RelationInfo {
                        kind: RelationKind::HasMany,
                        target: "Node".to_string(),
                        junction: None,
                        foreign_key: Some("parent_id".to_string()),
                    }),
                ),
                FieldDef::new(
                    "fulfills",
                    FieldType::VecString,
                    FieldRole::Relation(RelationInfo {
                        kind: RelationKind::ManyToMany,
                        target: "Requirement".to_string(),
                        junction: Some("node_fulfills".to_string()),
                        foreign_key: None,
                    }),
                ),
                FieldDef::new("body", FieldType::String, FieldRole::Body),
            ],
        }
    }

    #[test]
    fn test_simple_entity_crud() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity);

        assert!(code.contains("fn list_roles("));
        assert!(code.contains("fn get_role("));
        assert!(code.contains("fn create_role("));
        assert!(code.contains("fn update_role("));
        assert!(code.contains("fn delete_role("));
        // No populate_relations for simple entities
        assert!(!code.contains("populate_role_relations"));
    }

    #[test]
    fn test_complex_entity_has_populate_relations() {
        let entity = make_node_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity);

        assert!(code.contains("populate_node_relations"));
        assert!(code.contains("sync_junction(\"node_fulfills\""));
        assert!(code.contains("set_node_parent"));
    }

    #[test]
    fn test_junction_sync_in_create() {
        let entity = make_node_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity);

        // Create should sync junctions
        assert!(code.contains("let fulfills = node.fulfills.clone()"));
        assert!(code.contains("let contains = node.contains.clone()"));
    }

    #[test]
    fn test_list_has_pagination_params() {
        let entity = make_role_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity);

        // Generated list_* must accept optional limit / offset for SQL-level pagination
        assert!(
            code.contains("fn list_roles(&self, limit: Option<u64>, offset: Option<u64>)"),
            "list_roles should have pagination params, got:\n{code}"
        );
        // And wire them into the SeaORM query
        assert!(code.contains("query = query.limit(l);"), "missing .limit() call");
        assert!(code.contains("query = query.offset(o);"), "missing .offset() call");
    }

    #[test]
    fn test_update_tracks_junction_changes() {
        let entity = make_node_entity();
        let mut code = String::new();
        generate_crud_impl(&mut code, &entity);

        // Update should track which junction fields changed
        assert!(code.contains("let fulfills_changed = updates.fulfills.is_some()"));
        assert!(code.contains("let contains_changed = updates.contains.is_some()"));
        assert!(code.contains("if fulfills_changed {"));
        assert!(code.contains("if contains_changed {"));
    }

    /// Syntax check: verify the generated `impl Store` block parses as valid
    /// Rust for both simple and relation-heavy entities.
    #[test]
    fn generated_code_is_valid_rust() {
        for entity in [make_role_entity(), make_node_entity()] {
            let mut code = String::new();
            generate_crud_impl(&mut code, &entity);
            syn::parse_file(&code).unwrap_or_else(|e| {
                panic!(
                    "store::gen_crud::generate_crud_impl produced invalid Rust for entity '{}': {e}\n--- code ---\n{code}",
                    entity.name
                )
            });
        }
    }
}
