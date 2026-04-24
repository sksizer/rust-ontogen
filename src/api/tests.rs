//! Integration tests for API generation against real schema files.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::ApiConfig;
    use crate::schema::parse::parse_schema_dir;
    use crate::store::helpers::to_snake_case;
    use crate::{api, ir};

    fn schema_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/schema")
    }

    fn api_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/api/v1")
    }

    fn base_config(output_dir: PathBuf) -> ApiConfig {
        ApiConfig {
            output_dir,
            exclude: vec![],
            scan_dirs: vec![],
            state_type: "AppState".to_string(),
            store_type: Some("Store".to_string()),
            schema_module_path: "crate::schema".to_string(),
        }
    }

    /// Test that gen_api produces valid code for all real entities (no scanning).
    #[test]
    fn generate_all_real_schemas() {
        let dir = schema_dir();
        if !dir.exists() {
            eprintln!("Skipping: schema dir not found at {}", dir.display());
            return;
        }

        let entities = parse_schema_dir(&dir).expect("parse_schema_dir failed");
        assert!(!entities.is_empty());

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = base_config(tmp.path().to_path_buf());

        let result = api::generate(&entities, &config);
        assert!(result.is_ok(), "gen_api failed: {:?}", result.err());

        let output = result.unwrap();

        // Should have one module per entity
        assert_eq!(output.modules.len(), entities.len(), "Expected one module per entity");

        // Each module should have 5 CRUD functions
        for module in &output.modules {
            assert_eq!(module.fns.len(), 5, "Module {} should have 5 CRUD fns", module.name);
            assert_eq!(module.state_type, ir::StateKind::Store);
        }

        // Check that files were written
        let mod_rs = tmp.path().join("mod.rs");
        assert!(mod_rs.exists(), "mod.rs should be generated");

        for entity in &entities {
            let snake = to_snake_case(&entity.name);
            let path = tmp.path().join(format!("{snake}.rs"));
            assert!(path.exists(), "Expected file for entity {}", entity.name);
        }
    }

    /// Test that generated code matches the hand-written pattern.
    #[test]
    fn agent_matches_hand_written_pattern() {
        let dir = schema_dir();
        if !dir.exists() {
            return;
        }

        let entities = parse_schema_dir(&dir).expect("parse failed");
        let agent = entities.iter().find(|e| e.name == "Agent").expect("Agent entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = base_config(tmp.path().to_path_buf());

        api::generate(&[agent.clone()], &config).expect("gen_api failed");

        let content = std::fs::read_to_string(tmp.path().join("agent.rs")).unwrap();

        // Should match the patterns from hand-written agent.rs
        assert!(content.contains("use crate::schema::Agent"), "Missing Agent import");
        assert!(content.contains("use crate::store::Store"), "Missing Store import");
        assert!(content.contains("CreateAgentInput"), "Missing CreateAgentInput");
        assert!(content.contains("UpdateAgentInput"), "Missing UpdateAgentInput");
        assert!(content.contains("AgentUpdate"), "Missing AgentUpdate import");

        // Function signatures
        assert!(content.contains("pub async fn list(store: &Store)"));
        assert!(content.contains("pub async fn get_by_id(store: &Store, id: &str)"));
        assert!(content.contains("pub async fn create(store: &Store, input: CreateAgentInput)"));
        assert!(content.contains("pub async fn update(store: &Store, id: &str, input: UpdateAgentInput)"));
        assert!(content.contains("pub async fn delete(store: &Store, id: &str)"));

        // Store delegation
        assert!(content.contains("store.list_agents()"));
        assert!(content.contains("store.get_agent(id)"));
        assert!(content.contains("store.create_agent(agent)"));
        assert!(content.contains("store.update_agent(id, updates)"));
        assert!(content.contains("store.delete_agent(id)"));
    }

    /// Test that a non-default `schema_module_path` flows into generated API code.
    #[test]
    fn custom_schema_module_path_is_respected() {
        let dir = schema_dir();
        if !dir.exists() {
            return;
        }

        let entities = parse_schema_dir(&dir).expect("parse failed");
        let agent = entities.iter().find(|e| e.name == "Agent").expect("Agent entity not found");

        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = base_config(tmp.path().to_path_buf());
        config.schema_module_path = "my_crate::domain".to_string();

        api::generate(&[agent.clone()], &config).expect("gen_api failed");

        let content = std::fs::read_to_string(tmp.path().join("agent.rs")).unwrap();
        assert!(
            content.contains("use my_crate::domain::AppError;"),
            "Expected custom schema path for AppError, got:\n{content}"
        );
        assert!(
            content.contains("use my_crate::domain::Agent;"),
            "Expected custom schema path for Agent, got:\n{content}"
        );
        assert!(
            content.contains("use my_crate::domain::{CreateAgentInput, UpdateAgentInput};"),
            "Expected custom schema path for input types, got:\n{content}"
        );
        assert!(!content.contains("use crate::schema::"), "Default schema path should not appear");
    }

    /// Test that excluded entities are skipped.
    #[test]
    fn exclude_skips_entities() {
        let dir = schema_dir();
        if !dir.exists() {
            return;
        }

        let entities = parse_schema_dir(&dir).expect("parse failed");
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = base_config(tmp.path().to_path_buf());
        config.exclude = vec!["Contract".to_string(), "Evidence".to_string()];

        let output = api::generate(&entities, &config).expect("gen_api failed");

        assert!(!output.modules.iter().any(|m| m.name == "contract"));
        assert!(!output.modules.iter().any(|m| m.name == "evidence"));
        assert!(!tmp.path().join("contract.rs").exists());
        assert!(!tmp.path().join("evidence.rs").exists());
        assert_eq!(output.modules.len(), entities.len() - 2);
    }

    /// Test that OpKind is correctly assigned.
    #[test]
    fn op_kinds_are_correct() {
        let dir = schema_dir();
        if !dir.exists() {
            return;
        }

        let entities = parse_schema_dir(&dir).expect("parse failed");
        let role = entities.iter().find(|e| e.name == "Role").expect("Role");

        let tmp = tempfile::tempdir().expect("tempdir");
        let config = base_config(tmp.path().to_path_buf());

        let output = api::generate(&[role.clone()], &config).expect("gen_api failed");
        let module = &output.modules[0];

        let find_fn = |name: &str| module.fns.iter().find(|f| f.name == name).unwrap();

        assert_eq!(find_fn("list").classified_op, ir::OpKind::List);
        assert_eq!(find_fn("get_by_id").classified_op, ir::OpKind::GetById);
        assert_eq!(find_fn("create").classified_op, ir::OpKind::Create);
        assert_eq!(find_fn("update").classified_op, ir::OpKind::Update);
        assert_eq!(find_fn("delete").classified_op, ir::OpKind::Delete);
    }

    // ─── Scanning + merge tests ──────────────────────────────────────────────

    /// Test that scanning discovers custom modules not in the entity list.
    #[test]
    fn scan_discovers_custom_modules() {
        let sdir = schema_dir();
        let adir = api_dir();
        if !sdir.exists() || !adir.exists() {
            return;
        }

        let entities = parse_schema_dir(&sdir).expect("parse failed");
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = base_config(tmp.path().to_path_buf());
        config.scan_dirs = vec![adir];

        let output = api::generate(&entities, &config).expect("gen_api failed");

        // Should have custom modules that aren't entity CRUD
        let custom_names: Vec<&str> = output
            .modules
            .iter()
            .filter(|m| !entities.iter().any(|e| to_snake_case(&e.name) == m.name))
            .map(|m| m.name.as_str())
            .collect();

        assert!(custom_names.contains(&"graph"), "graph module should be discovered. Found: {custom_names:?}");
        assert!(custom_names.contains(&"project"), "project module should be discovered. Found: {custom_names:?}");
        assert!(custom_names.contains(&"events"), "events module should be discovered. Found: {custom_names:?}");
    }

    /// Test that events module uses AppState, not Store.
    #[test]
    fn events_module_uses_app_state() {
        let sdir = schema_dir();
        let adir = api_dir();
        if !sdir.exists() || !adir.exists() {
            return;
        }

        // No entities — just scan
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = base_config(tmp.path().to_path_buf());
        config.scan_dirs = vec![adir];

        let output = api::generate(&[], &config).expect("gen_api failed");

        let events = output.modules.iter().find(|m| m.name == "events").expect("events module not found");

        assert_eq!(events.state_type, ir::StateKind::AppState, "events module should use AppState");

        // Should have event stream functions
        assert!(
            events.fns.iter().any(|f| f.classified_op == ir::OpKind::EventStream),
            "events module should have EventStream functions"
        );
    }

    /// Test that graph module has custom functions with correct OpKind.
    #[test]
    fn graph_module_has_custom_functions() {
        let sdir = schema_dir();
        let adir = api_dir();
        if !sdir.exists() || !adir.exists() {
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = base_config(tmp.path().to_path_buf());
        config.scan_dirs = vec![adir];

        let output = api::generate(&[], &config).expect("gen_api failed");

        let graph = output.modules.iter().find(|m| m.name == "graph").expect("graph module not found");

        assert_eq!(graph.state_type, ir::StateKind::Store);

        let snapshot_fn =
            graph.fns.iter().find(|f| f.name == "get_graph_snapshot").expect("get_graph_snapshot not found");

        // Has optional parent_id param — should be CustomGet
        assert_eq!(snapshot_fn.classified_op, ir::OpKind::CustomGet);
        assert!(!snapshot_fn.doc.is_empty(), "Should have doc comment");
    }

    /// Test that scanning an entity module doesn't duplicate CRUD functions.
    #[test]
    fn merge_does_not_duplicate_crud() {
        let sdir = schema_dir();
        let adir = api_dir();
        if !sdir.exists() || !adir.exists() {
            return;
        }

        let entities = parse_schema_dir(&sdir).expect("parse failed");
        let agent = entities.iter().find(|e| e.name == "Agent").expect("Agent");

        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = base_config(tmp.path().to_path_buf());
        config.scan_dirs = vec![adir];

        let output = api::generate(&[agent.clone()], &config).expect("gen_api failed");

        let agent_module = output.modules.iter().find(|m| m.name == "agent").expect("agent module");

        // Should still have exactly 5 CRUD functions — scanning finds the same
        // 5 in the hand-written file, but merge deduplicates by name
        assert_eq!(
            agent_module.fns.len(),
            5,
            "Agent should have exactly 5 functions after merge, got: {:?}",
            agent_module.fns.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
    }

    /// Test that project module (AppState-based) merges correctly with no entities.
    #[test]
    fn project_module_scanned_standalone() {
        let adir = api_dir();
        if !adir.exists() {
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = base_config(tmp.path().to_path_buf());
        config.scan_dirs = vec![adir];

        let output = api::generate(&[], &config).expect("gen_api failed");

        let project = output.modules.iter().find(|m| m.name == "project").expect("project module");

        assert_eq!(project.state_type, ir::StateKind::AppState);

        // project.rs has: switch_project, open_project, close_project,
        // list_loaded_projects, list, get_by_id, create, update, delete
        assert!(project.fns.len() >= 5, "project should have at least 5 functions, got {}", project.fns.len());

        // Should have custom functions
        let fn_names: Vec<&str> = project.fns.iter().map(|f| f.name.as_str()).collect();
        assert!(fn_names.contains(&"switch_project"), "Missing switch_project");
        assert!(fn_names.contains(&"open_project"), "Missing open_project");
    }
}
