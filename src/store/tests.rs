//! Integration tests for store generation against real schema files.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{StoreConfig, schema::parse::parse_schema_dir, store};

    /// Test that gen_store produces valid code for all real entities.
    /// This reads the actual schema files and generates store code to a temp dir.
    #[test]
    fn generate_all_real_schemas() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/schema");

        if !schema_dir.exists() {
            eprintln!("Skipping: schema dir not found at {}", schema_dir.display());
            return;
        }

        let entities = parse_schema_dir(&schema_dir).expect("parse_schema_dir failed");
        assert!(!entities.is_empty(), "Expected at least one entity from real schemas");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
        };

        let result = store::generate(&entities, None, &config);
        assert!(result.is_ok(), "gen_store failed: {:?}", result.err());

        let output = result.unwrap();

        // Should have 5 CRUD methods per entity
        assert_eq!(output.methods.len(), entities.len() * 5, "Expected 5 methods per entity");

        // Check that files were written
        let mod_rs = tmp.path().join("mod.rs");
        assert!(mod_rs.exists(), "mod.rs should be generated");

        // Check that each entity has a file
        for entity in &entities {
            let snake = crate::store::helpers::to_snake_case(&entity.name);
            let path = tmp.path().join(format!("{snake}.rs"));
            assert!(path.exists(), "Expected file for entity {}", entity.name);

            let content = std::fs::read_to_string(&path).unwrap();

            // Every generated file should have the CRUD methods
            assert!(content.contains(&format!("fn list_")), "Missing list for {}", entity.name);
            assert!(content.contains(&format!("fn get_")), "Missing get for {}", entity.name);
            assert!(content.contains(&format!("fn create_")), "Missing create for {}", entity.name);
            assert!(content.contains(&format!("fn update_")), "Missing update for {}", entity.name);
            assert!(content.contains(&format!("fn delete_")), "Missing delete for {}", entity.name);

            // Every entity should have Update struct + From impls
            assert!(
                content.contains(&format!("pub struct {}Update", entity.name)),
                "Missing Update struct for {}",
                entity.name
            );
            assert!(
                content.contains(&format!("From<crate::schema::Create{}Input>", entity.name)),
                "Missing From<CreateInput> for {}",
                entity.name
            );
        }
    }

    /// Test that Role (simplest entity) generates code matching the hand-written pattern.
    #[test]
    fn role_matches_hand_written_pattern() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/schema");

        if !schema_dir.exists() {
            return;
        }

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let role = entities.iter().find(|e| e.name == "Role").expect("Role entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
        };

        store::generate(&[role.clone()], None, &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("role.rs")).unwrap();

        // Key patterns from hand-written role.rs
        assert!(content.contains("list_roles"), "Missing list_roles");
        assert!(content.contains("get_role"), "Missing get_role");
        assert!(content.contains("create_role"), "Missing create_role");
        assert!(content.contains("update_role"), "Missing update_role");
        assert!(content.contains("delete_role"), "Missing delete_role");
        assert!(content.contains("RoleUpdate"), "Missing RoleUpdate struct");
        assert!(content.contains("pub body: Option<String>"), "Missing body field in RoleUpdate");
        assert!(content.contains("emit_change(ChangeOp::Created"), "Missing Created event");
        assert!(content.contains("emit_change(ChangeOp::Updated"), "Missing Updated event");
        assert!(content.contains("emit_change(ChangeOp::Deleted"), "Missing Deleted event");
        assert!(content.contains("RoleNotFound"), "Missing error variant");
        // Should NOT have populate_relations (simple entity)
        assert!(!content.contains("populate_role_relations"), "Role should not have populate_relations");

        // Hook calls should be present in generated CRUD
        assert!(content.contains("hooks::before_create("), "Missing before_create hook call");
        assert!(content.contains("hooks::after_create("), "Missing after_create hook call");
        assert!(content.contains("hooks::before_update("), "Missing before_update hook call");
        assert!(content.contains("hooks::after_update("), "Missing after_update hook call");
        assert!(content.contains("hooks::before_delete("), "Missing before_delete hook call");
        assert!(content.contains("hooks::after_delete("), "Missing after_delete hook call");

        // Hook module import
        assert!(content.contains("use crate::store::hooks::role as hooks;"), "Missing hooks import");
    }

    /// Test that a non-default `schema_module_path` flows into generated store code.
    #[test]
    fn custom_schema_module_path_is_respected() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/schema");

        if !schema_dir.exists() {
            return;
        }

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let role = entities.iter().find(|e| e.name == "Role").expect("Role entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let hooks = tmp.path().join("hooks");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: Some(hooks.clone()),
            schema_module_path: "my_crate::domain".to_string(),
        };

        store::generate(&[role.clone()], None, &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("role.rs")).unwrap();
        assert!(content.contains("use my_crate::domain::Role;"), "Expected custom schema path import, got:\n{content}");
        assert!(
            content.contains("use my_crate::domain::{AppError, ChangeOp, EntityKind};"),
            "Expected custom schema path for AppError/ChangeOp/EntityKind, got:\n{content}"
        );
        assert!(!content.contains("use crate::schema::Role;"), "Default schema path should not appear");

        // Hook file should also use the custom path
        let hook_content = std::fs::read_to_string(hooks.join("role.rs")).unwrap();
        assert!(
            hook_content.contains("use my_crate::domain::{Role, AppError};"),
            "Expected custom schema path in hook file, got:\n{hook_content}"
        );
    }

    /// Test that Capability (complex entity with junctions) generates junction sync code.
    #[test]
    fn capability_has_junction_sync() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/schema");

        if !schema_dir.exists() {
            return;
        }

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let cap = entities.iter().find(|e| e.name == "Capability").expect("Capability entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
        };

        store::generate(&[cap.clone()], None, &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("capability.rs")).unwrap();

        assert!(content.contains("populate_capability_relations"), "Missing populate_capability_relations");
        assert!(content.contains("sync_junction"), "Missing sync_junction call");
        assert!(content.contains("capability_goal_ids"), "Missing capability_goal_ids junction table");
        assert!(content.contains("set_capability_parent"), "Missing set_capability_parent helper");
        assert!(content.contains("goal_ids_changed"), "Missing conditional junction sync tracking");
    }
}
