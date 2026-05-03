//! Integration tests for store generation against real schema files.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{StoreConfig, schema::parse::parse_schema_dir, store};

    /// Test that gen_store produces valid code for all real entities.
    /// This reads the embedded fixture schema files and generates store code to a temp dir.
    #[test]
    fn generate_all_real_schemas() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("parse_schema_dir failed");
        assert!(!entities.is_empty(), "Expected at least one entity from fixture schemas");

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

    /// Test that Tag (simplest entity, no relations) generates code matching the hand-written pattern.
    #[test]
    fn tag_matches_hand_written_pattern() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let tag = entities.iter().find(|e| e.name == "Tag").expect("Tag entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
        };

        store::generate(&[tag.clone()], None, &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("tag.rs")).unwrap();

        // Key CRUD method names
        assert!(content.contains("list_tags"), "Missing list_tags");
        assert!(content.contains("get_tag"), "Missing get_tag");
        assert!(content.contains("create_tag"), "Missing create_tag");
        assert!(content.contains("update_tag"), "Missing update_tag");
        assert!(content.contains("delete_tag"), "Missing delete_tag");
        assert!(content.contains("TagUpdate"), "Missing TagUpdate struct");
        assert!(content.contains("emit_change(ChangeOp::Created"), "Missing Created event");
        assert!(content.contains("emit_change(ChangeOp::Updated"), "Missing Updated event");
        assert!(content.contains("emit_change(ChangeOp::Deleted"), "Missing Deleted event");
        assert!(content.contains("TagNotFound"), "Missing error variant");
        // Should NOT have populate_relations (simple entity, no relations)
        assert!(!content.contains("populate_tag_relations"), "Tag should not have populate_relations");

        // Hook calls should be present in generated CRUD
        assert!(content.contains("hooks::before_create("), "Missing before_create hook call");
        assert!(content.contains("hooks::after_create("), "Missing after_create hook call");
        assert!(content.contains("hooks::before_update("), "Missing before_update hook call");
        assert!(content.contains("hooks::after_update("), "Missing after_update hook call");
        assert!(content.contains("hooks::before_delete("), "Missing before_delete hook call");
        assert!(content.contains("hooks::after_delete("), "Missing after_delete hook call");

        // Hook module import
        assert!(content.contains("use crate::store::hooks::tag as hooks;"), "Missing hooks import");
    }

    /// Test that a non-default `schema_module_path` flows into generated store code.
    #[test]
    fn custom_schema_module_path_is_respected() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let tag = entities.iter().find(|e| e.name == "Tag").expect("Tag entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let hooks = tmp.path().join("hooks");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: Some(hooks.clone()),
            schema_module_path: "my_crate::domain".to_string(),
        };

        store::generate(&[tag.clone()], None, &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("tag.rs")).unwrap();
        assert!(content.contains("use my_crate::domain::Tag;"), "Expected custom schema path import, got:\n{content}");
        assert!(
            content.contains("use my_crate::domain::{AppError, ChangeOp, EntityKind};"),
            "Expected custom schema path for AppError/ChangeOp/EntityKind, got:\n{content}"
        );
        assert!(!content.contains("use crate::schema::Tag;"), "Default schema path should not appear");

        // Hook file should also use the custom path
        let hook_content = std::fs::read_to_string(hooks.join("tag.rs")).unwrap();
        assert!(
            hook_content.contains("use my_crate::domain::{AppError, Tag};"),
            "Expected custom schema path in hook file, got:\n{hook_content}"
        );
    }

    /// Test that Workout (entity with junction many_to_many + self belongs_to) generates junction sync code.
    #[test]
    fn workout_has_junction_sync() {
        let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema");

        let entities = parse_schema_dir(&schema_dir).expect("parse failed");
        let workout = entities.iter().find(|e| e.name == "Workout").expect("Workout entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = StoreConfig {
            output_dir: tmp.path().to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".to_string(),
        };

        store::generate(&[workout.clone()], None, &config).expect("gen_store failed");

        let content = std::fs::read_to_string(tmp.path().join("workout.rs")).unwrap();

        assert!(content.contains("populate_workout_relations"), "Missing populate_workout_relations");
        assert!(content.contains("sync_junction"), "Missing sync_junction call");
        assert!(content.contains("workout_tags"), "Missing workout_tags junction table");
        assert!(content.contains("tags_changed"), "Missing conditional junction sync tracking");
    }

    /// `StoreMethodMeta.params` should match the actual generated method signatures.
    #[test]
    fn method_meta_params_match_signatures() {
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

        let output = store::generate(&[role.clone()], None, &config).expect("gen_store failed");

        let by_name =
            |n: &str| output.methods.iter().find(|m| m.name == n).unwrap_or_else(|| panic!("missing method {n}"));

        let list = by_name("list_roles");
        assert_eq!(list.params.len(), 2);
        assert_eq!(list.params[0].name, "limit");
        assert_eq!(list.params[0].param_type, "Option<u64>");
        assert_eq!(list.params[1].name, "offset");

        let get = by_name("get_role");
        assert_eq!(get.params.len(), 1);
        assert_eq!(get.params[0].name, "id");
        assert_eq!(get.params[0].param_type, "&str");

        let create = by_name("create_role");
        assert_eq!(create.params.len(), 1);
        assert_eq!(create.params[0].name, "role");
        assert_eq!(create.params[0].param_type, "Role");

        let update = by_name("update_role");
        assert_eq!(update.params.len(), 2);
        assert_eq!(update.params[0].name, "id");
        assert_eq!(update.params[1].name, "updates");
        assert_eq!(update.params[1].param_type, "RoleUpdate");

        let delete = by_name("delete_role");
        assert_eq!(delete.params.len(), 1);
        assert_eq!(delete.params[0].name, "id");
    }
}
