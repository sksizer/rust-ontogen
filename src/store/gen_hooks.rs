//! Generate scaffold hook files for store entities.
//!
//! For each entity, generates a `hooks/{entity}.rs` file containing no-op
//! lifecycle hook functions:
//! - `before_create`, `after_create`
//! - `before_update`, `after_update`
//! - `before_delete`, `after_delete`
//!
//! Hook files are **scaffolded once** — if the file already exists, it is
//! never overwritten. Fill in the function bodies with your custom logic
//! (validation, status transitions, side effects, etc.).

use std::fs;
use std::path::Path;

use super::helpers::to_snake_case;
use crate::schema::model::EntityDef;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Scaffold hook files for all entities. Returns paths of newly created files.
///
/// Only creates files that don't already exist — never overwrites user edits.
pub fn scaffold_hooks(entities: &[EntityDef], hooks_dir: &Path) -> Result<Vec<String>, String> {
    fs::create_dir_all(hooks_dir).map_err(|e| format!("Failed to create {}: {e}", hooks_dir.display()))?;

    let mut created = Vec::new();

    for entity in entities {
        let snake = to_snake_case(&entity.name);
        let path = hooks_dir.join(format!("{snake}.rs"));

        if !path.exists() {
            let code = generate_hook_file(entity);
            fs::write(&path, &code).map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
            crate::rustfmt(&path);
            created.push(snake.clone());
        }
    }

    // Generate/update mod.rs to declare all hook modules
    let mod_rs = generate_hooks_mod_rs(entities, hooks_dir);
    let mod_path = hooks_dir.join("mod.rs");
    crate::write_and_format(&mod_path, &mod_rs).map_err(|e| format!("Failed to write {}: {e}", mod_path.display()))?;

    Ok(created)
}

// ─── Per-entity hook file ────────────────────────────────────────────────────

fn generate_hook_file(entity: &EntityDef) -> String {
    let name = &entity.name;
    let snake = to_snake_case(name);

    let mut code = String::with_capacity(2048);

    code.push_str(&format!("//! Lifecycle hooks for {name}.\n"));
    code.push_str("//!\n");
    code.push_str("//! This file was scaffolded by ontogen. It is yours to edit.\n");
    code.push_str("//! Fill in hook bodies with custom logic (validation, side effects, etc.).\n");
    code.push_str("//! This file is NEVER overwritten by the generator.\n\n");

    code.push_str("#![allow(unused_variables, clippy::unnecessary_wraps, clippy::unused_async)]\n\n");

    code.push_str(&format!("use crate::schema::{{{name}, AppError}};\n"));
    code.push_str("use crate::store::Store;\n");
    code.push_str(&format!("use crate::store::generated::{snake}::{name}Update;\n\n"));

    // before_create
    code.push_str(&format!("/// Called before a {snake} is inserted. Modify the entity or return Err to reject.\n"));
    code.push_str(&format!(
        "pub async fn before_create(_store: &Store, _{snake}: &mut {name}) -> Result<(), AppError> {{\n"
    ));
    code.push_str("    Ok(())\n");
    code.push_str("}\n\n");

    // after_create
    code.push_str(&format!("/// Called after a {snake} is successfully created.\n"));
    code.push_str(&format!(
        "pub async fn after_create(_store: &Store, _{snake}: &{name}) -> Result<(), AppError> {{\n"
    ));
    code.push_str("    Ok(())\n");
    code.push_str("}\n\n");

    // before_update
    code.push_str(&format!("/// Called before a {snake} is updated. Receives current state and pending changes.\n"));
    code.push_str(&format!(
        "pub async fn before_update(\n    _store: &Store,\n    _current: &{name},\n    _updates: &{name}Update,\n) -> Result<(), AppError> {{\n"
    ));
    code.push_str("    Ok(())\n");
    code.push_str("}\n\n");

    // after_update
    code.push_str(&format!("/// Called after a {snake} is successfully updated.\n"));
    code.push_str(&format!(
        "pub async fn after_update(_store: &Store, _{snake}: &{name}) -> Result<(), AppError> {{\n"
    ));
    code.push_str("    Ok(())\n");
    code.push_str("}\n\n");

    // before_delete
    code.push_str(&format!("/// Called before a {snake} is deleted.\n"));
    code.push_str("pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {\n");
    code.push_str("    Ok(())\n");
    code.push_str("}\n\n");

    // after_delete
    code.push_str(&format!("/// Called after a {snake} is successfully deleted.\n"));
    code.push_str("pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {\n");
    code.push_str("    Ok(())\n");
    code.push_str("}\n");

    code
}

// ─── mod.rs generation ───────────────────────────────────────────────────────

/// Generate the hooks `mod.rs`. This IS regenerated each build to pick up new
/// entities, but it only contains `pub mod` declarations — no user code.
fn generate_hooks_mod_rs(entities: &[EntityDef], hooks_dir: &Path) -> String {
    let mut code = String::new();
    code.push_str("//! Hook modules — regenerated each build to track entities.\n");
    code.push_str("//! Each module file is scaffolded once and never overwritten.\n\n");

    let mut names: Vec<String> = entities.iter().map(|e| to_snake_case(&e.name)).collect();
    names.sort();

    for name in &names {
        // Only declare the module if the hook file exists
        let path = hooks_dir.join(format!("{name}.rs"));
        if path.exists() {
            code.push_str(&format!("pub mod {name};\n"));
        }
    }

    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::model::{EntityDef, FieldDef, FieldRole, FieldType};

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

    #[test]
    fn test_hook_file_has_all_lifecycle_functions() {
        let entity = make_role_entity();
        let code = generate_hook_file(&entity);

        assert!(code.contains("pub async fn before_create("));
        assert!(code.contains("pub async fn after_create("));
        assert!(code.contains("pub async fn before_update("));
        assert!(code.contains("pub async fn after_update("));
        assert!(code.contains("pub async fn before_delete("));
        assert!(code.contains("pub async fn after_delete("));
    }

    #[test]
    fn test_hook_file_uses_correct_types() {
        let entity = make_role_entity();
        let code = generate_hook_file(&entity);

        assert!(code.contains("use crate::schema::{Role, AppError};"));
        assert!(code.contains("use crate::store::generated::role::RoleUpdate;"));
        assert!(code.contains("_role: &mut Role"));
        assert!(code.contains("_updates: &RoleUpdate"));
    }

    #[test]
    fn test_hook_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");

        // First scaffold — creates the file
        let created = scaffold_hooks(&[make_role_entity()], &hooks_dir).unwrap();
        assert_eq!(created, vec!["role"]);

        // Write custom content
        let path = hooks_dir.join("role.rs");
        std::fs::write(&path, "// custom logic").unwrap();

        // Second scaffold — should NOT overwrite
        let created = scaffold_hooks(&[make_role_entity()], &hooks_dir).unwrap();
        assert!(created.is_empty());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("// custom logic"), "hook file should not be overwritten");
    }
}
