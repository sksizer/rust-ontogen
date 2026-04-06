//! Tests for the servers module: types, classify, parse, and generators.
//!
//! Follows the same pattern as `store/tests.rs` and `api/tests.rs`:
//! real schema files from `src-tauri/src/schema/` are used where applicable,
//! with synthetic API files for parse-level tests.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::servers::classify::{OpKind, classify_op, is_read_operation};
use crate::servers::config::{ClientGenerator, Config, GeneratorConfig, PrefixParam, RoutePrefix, ServerGenerator};
use crate::servers::parse::{ApiFn, ApiModule, EventFn, Param};
use crate::servers::types::{
    NamingConfig, capitalize, collect_ts_import, collect_type_import, event_name, extract_input_type, inner_type,
    normalize_spaces, param_to_owned_type, rust_type_to_ts, snake_to_camel, strip_ref, to_pascal_case,
};

// ─── Helper ──────────────────────────────────────────────────────────────────

/// Build a minimal Config for testing generators.
fn test_config(api_dir: PathBuf) -> Config {
    Config {
        api_dir,
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming: NamingConfig::default(),
        generators: vec![],
        rustfmt_edition: "2024".to_string(),
        sse_route_overrides: HashMap::new(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        schema_entities: Vec::new(),
    }
}

/// Build a test config with route_prefix (project scoping).
fn test_config_with_prefix(api_dir: PathBuf) -> Config {
    let mut config = test_config(api_dir);
    config.route_prefix = Some(RoutePrefix {
        segments: "projects/:project_id".to_string(),
        state_accessor: "store_for".to_string(),
        params: vec![PrefixParam {
            name: "project_id".to_string(),
            rust_type: "uuid::Uuid".to_string(),
            ts_type: "string".to_string(),
        }],
    });
    config
}

/// Build a simple CRUD ApiModule for testing generators.
fn make_crud_module(name: &str, is_store_based: bool) -> ApiModule {
    ApiModule {
        name: name.to_string(),
        functions: vec![
            ApiFn {
                name: "list".to_string(),
                is_async: true,
                doc: format!("List all {}s.", name),
                params: vec![],
                return_type: format!("Vec<{}>", capitalize(name)),
                first_param_is_store: is_store_based,
            },
            ApiFn {
                name: "get_by_id".to_string(),
                is_async: true,
                doc: format!("Get a {} by ID.", name),
                params: vec![Param { name: "id".to_string(), ty: "&str".to_string() }],
                return_type: capitalize(name),
                first_param_is_store: is_store_based,
            },
            ApiFn {
                name: "create".to_string(),
                is_async: true,
                doc: format!("Create a new {}.", name),
                params: vec![Param { name: "input".to_string(), ty: format!("Create{}Input", capitalize(name)) }],
                return_type: capitalize(name),
                first_param_is_store: is_store_based,
            },
            ApiFn {
                name: "update".to_string(),
                is_async: true,
                doc: format!("Update a {}.", name),
                params: vec![
                    Param { name: "id".to_string(), ty: "&str".to_string() },
                    Param { name: "input".to_string(), ty: format!("Update{}Input", capitalize(name)) },
                ],
                return_type: capitalize(name),
                first_param_is_store: is_store_based,
            },
            ApiFn {
                name: "delete".to_string(),
                is_async: true,
                doc: format!("Delete a {}.", name),
                params: vec![Param { name: "id".to_string(), ty: "&str".to_string() }],
                return_type: "()".to_string(),
                first_param_is_store: is_store_based,
            },
        ],
        events: vec![],
    }
}

/// Build a module with custom (non-CRUD) functions.
fn make_custom_module() -> ApiModule {
    ApiModule {
        name: "graph".to_string(),
        functions: vec![
            ApiFn {
                name: "get_graph_snapshot".to_string(),
                is_async: true,
                doc: "Get the graph snapshot.".to_string(),
                params: vec![Param { name: "parent_id".to_string(), ty: "Option<&str>".to_string() }],
                return_type: "GraphSnapshot".to_string(),
                first_param_is_store: false,
            },
            ApiFn {
                name: "get_node_detail".to_string(),
                is_async: true,
                doc: "Get node detail.".to_string(),
                params: vec![Param { name: "node_id".to_string(), ty: "&str".to_string() }],
                return_type: "NodeDetail".to_string(),
                first_param_is_store: false,
            },
        ],
        events: vec![],
    }
}

/// Build a module with events.
fn make_event_module() -> ApiModule {
    ApiModule {
        name: "events".to_string(),
        functions: vec![],
        events: vec![EventFn { name: "graph_updated".to_string() }, EventFn { name: "entity_changed".to_string() }],
    }
}

/// Write a synthetic API source file for parse tests.
fn write_synthetic_api(dir: &std::path::Path, filename: &str, content: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(filename), content).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════════
// types.rs — Pure function tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_normalize_spaces() {
    assert_eq!(normalize_spaces("Vec < String >"), "Vec<String>");
    assert_eq!(normalize_spaces("std :: string :: String"), "std::string::String");
    assert_eq!(normalize_spaces("& str"), "&str");
    assert_eq!(normalize_spaces("Option < & str >"), "Option<&str>");
}

#[test]
fn test_capitalize() {
    assert_eq!(capitalize("hello"), "Hello");
    assert_eq!(capitalize(""), "");
    assert_eq!(capitalize("a"), "A");
    assert_eq!(capitalize("Hello"), "Hello");
}

#[test]
fn test_strip_ref() {
    assert_eq!(strip_ref("&str"), "str");
    assert_eq!(strip_ref("& str"), "str");
    assert_eq!(strip_ref("String"), "String");
    assert_eq!(strip_ref("&CreateNodeInput"), "CreateNodeInput");
}

#[test]
fn test_extract_input_type() {
    assert_eq!(extract_input_type("&CreateNodeInput"), "CreateNodeInput");
    assert_eq!(extract_input_type("CreateNodeInput"), "CreateNodeInput");
    assert_eq!(extract_input_type("& UpdateRoleInput"), "UpdateRoleInput");
}

#[test]
fn test_inner_type() {
    assert_eq!(inner_type("Vec<Node>"), "Node");
    assert_eq!(inner_type("Node"), "Node");
    assert_eq!(inner_type("()"), "()");
    assert_eq!(inner_type("Vec<GraphSnapshot>"), "GraphSnapshot");
}

#[test]
fn test_collect_type_import() {
    let mut imports = Vec::new();
    collect_type_import("Vec<Node>", &mut imports);
    assert_eq!(imports, vec!["Node"]);

    collect_type_import("()", &mut imports);
    assert_eq!(imports.len(), 1, "() should not add imports");

    collect_type_import("relation::Model", &mut imports);
    assert_eq!(imports.len(), 1, "entity-qualified types should not add imports");

    collect_type_import("Node", &mut imports);
    assert_eq!(imports.len(), 1, "duplicates should not be added");

    collect_type_import("Requirement", &mut imports);
    assert_eq!(imports, vec!["Node", "Requirement"]);
}

#[test]
fn test_event_name() {
    assert_eq!(event_name("graph_updated"), "graph-updated");
    assert_eq!(event_name("entity_changed"), "entity-changed");
    assert_eq!(event_name("simple"), "simple");
}

#[test]
fn test_to_pascal_case() {
    assert_eq!(to_pascal_case("graph_snapshot"), "GraphSnapshot");
    assert_eq!(to_pascal_case("node"), "Node");
    assert_eq!(to_pascal_case("work_execution"), "WorkExecution");
    assert_eq!(to_pascal_case("get_by_id"), "GetById");
}

#[test]
fn test_param_to_owned_type() {
    assert_eq!(param_to_owned_type("&str"), "String");
    assert_eq!(param_to_owned_type("& str"), "String");
    assert_eq!(param_to_owned_type("Option<&str>"), "Option<String>");
    assert_eq!(param_to_owned_type("String"), "String");
    assert_eq!(param_to_owned_type("i32"), "i32");
    assert_eq!(param_to_owned_type("CreateNodeInput"), "CreateNodeInput");
}

#[test]
fn test_snake_to_camel() {
    assert_eq!(snake_to_camel("get_nodes"), "getNodes");
    assert_eq!(snake_to_camel("create_node"), "createNode");
    assert_eq!(snake_to_camel("simple"), "simple");
    assert_eq!(snake_to_camel("get_by_id"), "getById");
    assert_eq!(snake_to_camel("project_id"), "projectId");
}

#[test]
fn test_rust_type_to_ts() {
    assert_eq!(rust_type_to_ts("()"), "null");
    assert_eq!(rust_type_to_ts("String"), "string");
    assert_eq!(rust_type_to_ts("&str"), "string");
    assert_eq!(rust_type_to_ts("i32"), "number");
    assert_eq!(rust_type_to_ts("u64"), "number");
    assert_eq!(rust_type_to_ts("f64"), "number");
    assert_eq!(rust_type_to_ts("bool"), "boolean");
    assert_eq!(rust_type_to_ts("Vec<Node>"), "Node[]");
    assert_eq!(rust_type_to_ts("Option<String>"), "string | null");
    assert_eq!(rust_type_to_ts("Option<i32>"), "number | null");
    assert_eq!(rust_type_to_ts("Node"), "Node");
    assert_eq!(rust_type_to_ts("relation::Model"), "RelationModel");
    assert_eq!(rust_type_to_ts("Vec<String>"), "string[]");
}

#[test]
fn test_collect_ts_import() {
    let mut imports = Vec::new();
    collect_ts_import("Node", &mut imports);
    assert_eq!(imports, vec!["Node"]);

    collect_ts_import("string", &mut imports);
    assert_eq!(imports.len(), 1, "primitives should not add imports");

    collect_ts_import("null", &mut imports);
    assert_eq!(imports.len(), 1, "null should not add imports");

    collect_ts_import("number", &mut imports);
    assert_eq!(imports.len(), 1);

    collect_ts_import("boolean", &mut imports);
    assert_eq!(imports.len(), 1);

    collect_ts_import("Node[]", &mut imports);
    assert_eq!(imports.len(), 1, "Node[] should reuse existing Node import");

    collect_ts_import("Requirement | null", &mut imports);
    assert!(imports.contains(&"Requirement".to_string()));
    assert_eq!(imports.len(), 2);
}

#[test]
fn test_naming_config_defaults() {
    let naming = NamingConfig::default();
    assert_eq!(naming.module_plural("node"), "nodes");
    assert_eq!(naming.module_plural("requirement"), "requirements");
    assert_eq!(naming.url_singular("node"), "node");
    assert_eq!(naming.label("node"), "Node");
    assert_eq!(naming.plural_label("node"), "Nodes");
}

#[test]
fn test_naming_config_overrides() {
    let mut naming = NamingConfig::default();
    naming.plural_overrides.insert("evidence".to_string(), "evidence".to_string());
    naming.singular_overrides.insert("work_execution".to_string(), "work_execution".to_string());
    naming.label_overrides.insert("work_execution".to_string(), "Work Execution".to_string());
    naming.plural_label_overrides.insert("evidence".to_string(), "Evidence".to_string());

    assert_eq!(naming.module_plural("evidence"), "evidence");
    assert_eq!(naming.url_singular("work_execution"), "work_execution");
    assert_eq!(naming.label("work_execution"), "Work Execution");
    assert_eq!(naming.plural_label("evidence"), "Evidence");
}

#[test]
fn test_derive_action() {
    let naming = NamingConfig::default();

    // Standard CRUD names should map to empty (no action segment)
    assert_eq!(naming.derive_action("node", "node"), "");
    assert_eq!(naming.derive_action("node", "nodes"), "");

    // Custom functions: strip module prefix and get_ prefix
    assert_eq!(naming.derive_action("graph", "get_graph_snapshot"), "snapshot");
    assert_eq!(naming.derive_action("node", "get_node_members"), "members");

    // Underscores become hyphens
    assert_eq!(naming.derive_action("graph", "get_graph_full_snapshot"), "full-snapshot");
}

#[test]
fn test_api_module_is_crud() {
    let crud = make_crud_module("node", true);
    assert!(crud.is_crud());

    let custom = make_custom_module();
    assert!(!custom.is_crud());

    let events = make_event_module();
    assert!(!events.is_crud());

    // Partial CRUD (missing delete) should return false
    let mut partial = make_crud_module("node", true);
    partial.functions.retain(|f| f.name != "delete");
    assert!(!partial.is_crud());
}

// ═══════════════════════════════════════════════════════════════════════════════
// classify.rs — Operation classification
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_classify_crud_operations() {
    let module = make_crud_module("node", true);

    for f in &module.functions {
        let op = classify_op(f);
        match f.name.as_str() {
            "list" => assert!(matches!(op, OpKind::List)),
            "get_by_id" => assert!(matches!(op, OpKind::GetById)),
            "create" => assert!(matches!(op, OpKind::Create)),
            "update" => assert!(matches!(op, OpKind::UpdateById)),
            "delete" => assert!(matches!(op, OpKind::DeleteById)),
            _ => panic!("unexpected function name: {}", f.name),
        }
    }
}

#[test]
fn test_classify_custom_operations() {
    let custom = make_custom_module();

    let snapshot = &custom.functions[0]; // get_graph_snapshot
    assert!(matches!(classify_op(snapshot), OpKind::CustomGet));

    let detail = &custom.functions[1]; // get_node_detail
    assert!(matches!(classify_op(detail), OpKind::CustomGet));

    // A non-get function with params should be CustomPost
    let post_fn = ApiFn {
        name: "switch_project".to_string(),
        is_async: true,
        doc: String::new(),
        params: vec![Param { name: "path".to_string(), ty: "&str".to_string() }],
        return_type: "()".to_string(),
        first_param_is_store: false,
    };
    assert!(matches!(classify_op(&post_fn), OpKind::CustomPost));
}

#[test]
fn test_classify_no_params_is_get() {
    let no_param_fn = ApiFn {
        name: "detect_installed_openers".to_string(),
        is_async: true,
        doc: String::new(),
        params: vec![],
        return_type: "Vec<String>".to_string(),
        first_param_is_store: false,
    };
    assert!(matches!(classify_op(&no_param_fn), OpKind::CustomGet));
}

#[test]
fn test_is_read_operation() {
    assert!(is_read_operation("get_by_id"));
    assert!(is_read_operation("get_graph_snapshot"));
    assert!(is_read_operation("list"));
    assert!(is_read_operation("detect_installed_openers"));

    assert!(!is_read_operation("create"));
    assert!(!is_read_operation("update"));
    assert!(!is_read_operation("delete"));
    assert!(!is_read_operation("switch_project"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// parse.rs — API module parsing
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_parse_store_based_module() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "agent.rs",
        r#"
use crate::store::Store;
use crate::schema::{Agent, CreateAgentInput, UpdateAgentInput};

/// List all agents.
pub async fn list(store: &Store) -> Result<Vec<Agent>, anyhow::Error> { todo!() }

/// Get an agent by ID.
pub async fn get_by_id(store: &Store, id: &str) -> Result<Agent, anyhow::Error> { todo!() }

/// Create a new agent.
pub async fn create(store: &Store, input: CreateAgentInput) -> Result<Agent, anyhow::Error> { todo!() }

/// Update an agent.
pub async fn update(store: &Store, id: &str, input: UpdateAgentInput) -> Result<Agent, anyhow::Error> { todo!() }

/// Delete an agent.
pub async fn delete(store: &Store, id: &str) -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert_eq!(modules.len(), 1);

    let m = &modules[0];
    assert_eq!(m.name, "agent");
    assert_eq!(m.functions.len(), 5);
    assert!(m.functions.iter().all(|f| f.first_param_is_store));
    assert!(m.functions.iter().all(|f| f.is_async));

    let list_fn = m.functions.iter().find(|f| f.name == "list").unwrap();
    assert_eq!(list_fn.return_type, "Vec<Agent>");
    assert!(list_fn.params.is_empty());

    let create_fn = m.functions.iter().find(|f| f.name == "create").unwrap();
    assert_eq!(create_fn.params.len(), 1);
    assert_eq!(create_fn.params[0].name, "input");
    assert!(create_fn.params[0].ty.contains("CreateAgentInput"));
}

#[test]
fn test_parse_state_based_module() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "project.rs",
        r#"
use crate::AppState;
use crate::schema::{Project, CreateProjectInput};

/// List all projects.
pub async fn list(state: &AppState) -> Result<Vec<Project>, anyhow::Error> { todo!() }

/// Create a project.
pub async fn create(state: &AppState, input: CreateProjectInput) -> Result<Project, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert_eq!(modules.len(), 1);

    let m = &modules[0];
    assert_eq!(m.name, "project");
    assert_eq!(m.functions.len(), 2);
    assert!(m.functions.iter().all(|f| !f.first_param_is_store));
}

#[test]
fn test_parse_event_functions() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "events.rs",
        r#"
use crate::AppState;
use tokio::sync::broadcast;

/// Subscribe to graph updates.
pub fn graph_updated(state: &AppState) -> broadcast::Receiver<String> {
    todo!()
}

/// Subscribe to entity changes.
pub fn entity_changed(state: &AppState) -> broadcast::Receiver<String> {
    todo!()
}
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert_eq!(modules.len(), 1);

    let m = &modules[0];
    assert_eq!(m.name, "events");
    assert!(m.functions.is_empty(), "event functions should not appear as regular functions");
    assert_eq!(m.events.len(), 2);
    assert_eq!(m.events[0].name, "graph_updated");
    assert_eq!(m.events[1].name, "entity_changed");
}

#[test]
fn test_parse_skips_mod_rs_and_impl_files() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(&api_dir, "mod.rs", "pub mod node;\npub mod graph;\n");
    write_synthetic_api(
        &api_dir,
        "node_impl.rs",
        "use crate::AppState;\npub async fn helper(state: &AppState) -> Result<(), anyhow::Error> { todo!() }\n",
    );
    write_synthetic_api(
        &api_dir,
        "node.rs",
        "use crate::store::Store;\npub async fn list(store: &Store) -> Result<Vec<String>, anyhow::Error> { todo!() }\n",
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert_eq!(modules.len(), 1, "should only find node.rs");
    assert_eq!(modules[0].name, "node");
}

#[test]
fn test_parse_subdirectory_scanning() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    let gen_dir = api_dir.join("generated");

    write_synthetic_api(
        &api_dir,
        "graph.rs",
        "use crate::AppState;\npub async fn get_graph_snapshot(state: &AppState) -> Result<String, anyhow::Error> { todo!() }\n",
    );
    write_synthetic_api(
        &gen_dir,
        "node.rs",
        "use crate::store::Store;\npub async fn list(store: &Store) -> Result<Vec<String>, anyhow::Error> { todo!() }\n",
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert_eq!(modules.len(), 2, "should find both top-level and generated/ files");
    let names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"graph"));
    assert!(names.contains(&"node"));
}

#[test]
fn test_parse_doc_comments() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "role.rs",
        r#"
use crate::store::Store;

/// List all roles in the system.
pub async fn list(store: &Store) -> Result<Vec<String>, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    let f = &modules[0].functions[0];
    assert_eq!(f.doc, "List all roles in the system.");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Generator integration tests — verify generated output structure
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_http_generator_crud_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config_with_prefix(tmp.path().to_path_buf());

    let modules = vec![make_crud_module("node", true)];
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // Store-based modules should generate scoped handlers only
    assert!(content.contains("list_nodes_scoped"), "should generate scoped list handler");
    assert!(content.contains("get_node_by_id_scoped"), "should generate scoped get handler");
    assert!(content.contains("create_node_handler_scoped"), "should generate scoped create handler");
    assert!(content.contains("update_node_handler_scoped"), "should generate scoped update handler");
    assert!(content.contains("delete_node_handler_scoped"), "should generate scoped delete handler");

    // Should have entity_routes function
    assert!(content.contains("pub fn entity_routes()"));
    assert!(content.contains("Router::new()"));

    // Should have scoped route paths
    assert!(content.contains("/api/projects/:project_id/nodes"));

    // Should have store construction
    assert!(content.contains("state.store_for(&project_id)"));

    // Standard Axum imports
    assert!(content.contains("use axum::"));
    assert!(content.contains("use crate::AppState"));
    assert!(content.contains("use crate::store::Store"));
}

#[test]
fn test_http_generator_state_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config_with_prefix(tmp.path().to_path_buf());

    let modules = vec![make_crud_module("project", false)];
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // State-based modules get unscoped routes
    assert!(content.contains("list_projects"));
    assert!(content.contains("/api/projects"));
    assert!(content.contains("project::list(&state)"), "should pass &state directly");
}

#[test]
fn test_http_generator_events() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let mut config = test_config_with_prefix(tmp.path().to_path_buf());
    config.sse_route_overrides.insert("graph_updated".to_string(), "/api/events/graph".to_string());

    let modules = vec![make_event_module()];
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    assert!(content.contains("graph_updated_sse"), "should generate SSE handler");
    assert!(content.contains("entity_changed_sse"));
    assert!(content.contains("Sse<impl futures::Stream"));
    assert!(content.contains("BroadcastStream"));
    assert!(content.contains("/api/events/graph"), "should use SSE route override");
}

#[test]
fn test_http_generator_store_module_no_prefix() {
    // Store-based modules without route_prefix should generate unscoped handlers
    // that construct a Store via state.store().await.
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config(tmp.path().to_path_buf()); // no route_prefix

    let modules = vec![make_crud_module("node", true)];
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // Should generate unscoped handlers (not scoped)
    assert!(content.contains("list_nodes"), "should generate list handler");
    assert!(content.contains("get_node_by_id"), "should generate get handler");
    assert!(content.contains("create_node_handler"), "should generate create handler");
    assert!(content.contains("update_node_handler"), "should generate update handler");
    assert!(content.contains("delete_node_handler"), "should generate delete handler");

    // Should construct store from state
    assert!(content.contains("state.store().await"), "should construct store from state");

    // Should pass store (already &Store) directly to service functions
    assert!(content.contains("node::list(store)"), "should pass store to list");
    assert!(content.contains("node::get_by_id(store, &id)"), "should pass store to get_by_id");
    assert!(content.contains("node::create(store, input)"), "should pass store to create");
    assert!(content.contains("node::update(store, &id, input)"), "should pass store to update");
    assert!(content.contains("node::delete(store, &id)"), "should pass store to delete");

    // Should have CRUD routes
    assert!(content.contains("/api/nodes"), "should have list route");
    assert!(content.contains("/api/nodes/:id"), "should have detail route");

    // Should NOT have scoped routes (no route_prefix)
    assert!(!content.contains("_scoped"), "should not have scoped handlers");
}

#[test]
fn test_http_generator_custom_functions() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config(tmp.path().to_path_buf());

    let modules = vec![make_custom_module()];
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // Custom GET with query params
    assert!(content.contains("get_graph_snapshot"));
    assert!(content.contains("/api/graphs/snapshot"), "should derive action from fn name");

    // Custom GET with path params
    assert!(content.contains("get_node_detail"));
}

#[test]
fn test_ipc_generator_crud_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("ipc_generated.rs");
    let config = test_config_with_prefix(tmp.path().to_path_buf());

    let modules = vec![make_crud_module("node", true)];
    crate::servers::generators::ipc::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // IPC command names
    assert!(content.contains("pub async fn get_nodes("), "list → get_nodes");
    assert!(content.contains("pub async fn get_node_by_id("));
    assert!(content.contains("pub async fn create_node("));
    assert!(content.contains("pub async fn update_node("));
    assert!(content.contains("pub async fn delete_node("));

    // Tauri attributes
    assert!(content.contains("#[tauri::command]"));
    assert!(!content.contains("#[specta::specta]"), "specta annotation should not be generated");

    // Store construction for store-based modules
    assert!(content.contains("state.store_for("));

    // Input types
    assert!(content.contains("CreateNodeInput"));
    assert!(content.contains("UpdateNodeInput"));

    // Optional project_id param
    assert!(content.contains("project_id: Option<String>"));
}

#[test]
fn test_mcp_generator_crud_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("mcp_generated.rs");
    let config = test_config_with_prefix(tmp.path().to_path_buf());

    let modules = vec![make_crud_module("node", true)];
    crate::servers::generators::mcp::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // MCP tool names
    assert!(content.contains(r#"name: "get_nodes""#));
    assert!(content.contains(r#"name: "get_node_by_id""#));
    assert!(content.contains(r#"name: "create_node""#));
    assert!(content.contains(r#"name: "update_node""#));
    assert!(content.contains(r#"name: "delete_node""#));

    // Registry function
    assert!(content.contains("pub fn generated_tool_registry()"));
    assert!(content.contains("Vec<McpToolDef>"));

    // Schema helpers
    assert!(content.contains("schema_for::<EmptyInput>"));
    assert!(content.contains("schema_for::<GetByIdInput>"));
    assert!(content.contains("with_project_id_schema"));

    // Store construction
    assert!(content.contains("state.store_for("));

    // Struct definitions
    assert!(content.contains("pub struct McpToolDef"));
    assert!(content.contains("pub struct GetByIdInput"));
    assert!(content.contains("pub struct EmptyInput"));
}

#[test]
fn test_ts_transport_generator_crud_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("generated.ts");
    let bindings = tmp.path().join("bindings.ts");

    // Write a minimal bindings file
    std::fs::write(
        &bindings,
        "export type Node = { id: string; name: string; };\n\
         export type CreateNodeInput = { id: string; name: string; };\n\
         export type UpdateNodeInput = { name?: string; };\n",
    )
    .unwrap();

    let config = test_config_with_prefix(tmp.path().to_path_buf());
    let modules = vec![make_crud_module("node", true)];

    crate::servers::generators::transport::generate(&output, &bindings, &modules, &config);
    let content = std::fs::read_to_string(&output).unwrap();

    // Transport interface
    assert!(content.contains("export interface Transport"));
    assert!(content.contains("getNodes("));
    assert!(content.contains("getNodeById("));
    assert!(content.contains("createNode("));
    assert!(content.contains("updateNode("));
    assert!(content.contains("deleteNode("));

    // HTTP transport
    assert!(content.contains("export function createHttpTransport(): Transport"));
    assert!(content.contains("httpGet("));
    assert!(content.contains("httpPost<"));
    assert!(content.contains("httpPut<"));
    assert!(content.contains("httpDelete("));

    // IPC transport
    assert!(content.contains("export function createIpcTransport(): Transport"));
    assert!(content.contains("invoke("));

    // Project scoping
    assert!(content.contains("projectId?: string"));
    assert!(content.contains("scopedPath("));

    // Type imports from bindings
    assert!(content.contains("import type {"));
    assert!(content.contains("Node"));
}

#[test]
fn test_admin_registry_generator() {
    let tmp = tempfile::tempdir().unwrap();
    // Prettier resolves config from the output file's directory; drop a
    // .prettierrc so it uses single quotes (matching the project convention).
    std::fs::write(tmp.path().join(".prettierrc"), r#"{ "singleQuote": true }"#).unwrap();
    let output = tmp.path().join("admin-registry.ts");
    let config = test_config(tmp.path().to_path_buf());

    let modules = vec![
        make_crud_module("node", true),
        make_crud_module("agent", true),
        make_custom_module(), // non-CRUD should be excluded
    ];

    crate::servers::generators::admin::generate(&output, &modules, &config);
    let content = std::fs::read_to_string(&output).unwrap();

    // Type import (definitions moved to @ontogen/admin-types)
    assert!(content.contains("import type { AdminFieldDef, AdminEntityConfig }"));

    // Registry array
    assert!(content.contains("export const adminEntities: AdminEntityConfig[]"));

    // Both CRUD modules registered
    assert!(content.contains("key: 'node'"));
    assert!(content.contains("key: 'agent'"));

    // Non-CRUD module excluded
    assert!(!content.contains("key: 'graph'"));

    // Naming/pluralization
    assert!(content.contains("plural: 'nodes'"));
    assert!(content.contains("plural: 'agents'"));
    assert!(content.contains("label: 'Node'"));
    assert!(content.contains("pluralLabel: 'Nodes'"));

    // Transport method names (camelCase)
    assert!(content.contains("listMethod: 'getNodes'"));
    assert!(content.contains("getMethod: 'getNodeById'"));
    assert!(content.contains("createMethod: 'createNode'"));
    assert!(content.contains("updateMethod: 'updateNode'"));
    assert!(content.contains("deleteMethod: 'deleteNode'"));

    // Type references
    assert!(content.contains("returnType: 'Node'"));
    assert!(content.contains("createInputType: 'CreateNodeInput'"));
    assert!(content.contains("updateInputType: 'UpdateNodeInput'"));

    // Lookup maps
    assert!(content.contains("export const adminEntityMap"));
    assert!(content.contains("export const adminEntityByPlural"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// E2E pipeline — scan real API modules → generate all outputs
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_e2e_generate_transport_with_real_api() {
    // Locate the real api directory
    let api_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/api/v1");
    if !api_dir.exists() {
        eprintln!("Skipping E2E test: API dir not found at {}", api_dir.display());
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let http_out = tmp.path().join("http/generated.rs");
    let ipc_out = tmp.path().join("ipc/generated.rs");
    let mcp_out = tmp.path().join("mcp/generated.rs");
    let ts_out = tmp.path().join("transport/generated.ts");
    let admin_out = tmp.path().join("admin/admin-registry.ts");

    // Create a dummy bindings.ts
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type Placeholder = unknown;\n").unwrap();

    let mut naming = NamingConfig::default();
    naming.plural_overrides.insert("evidence".to_string(), "evidence".to_string());
    naming.plural_overrides.insert("settings".to_string(), "settings".to_string());
    naming.plural_overrides.insert("status".to_string(), "status".to_string());
    naming.plural_overrides.insert("entity_counts".to_string(), "entity_counts".to_string());
    naming.plural_overrides.insert("unit_of_work".to_string(), "units_of_work".to_string());
    naming.plural_overrides.insert("step_result".to_string(), "step_results".to_string());
    naming.plural_overrides.insert("work_execution".to_string(), "work_executions".to_string());
    naming.plural_overrides.insert("workflow_template".to_string(), "workflow_templates".to_string());

    let config = Config {
        api_dir: api_dir.clone(),
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming,
        generators: vec![
            GeneratorConfig::Server(ServerGenerator::HttpAxum { output: http_out.clone() }),
            GeneratorConfig::Server(ServerGenerator::TauriIpc { output: ipc_out.clone() }),
            GeneratorConfig::Server(ServerGenerator::Mcp { output: mcp_out.clone() }),
            GeneratorConfig::Client(ClientGenerator::HttpTauriIpcSplit {
                output: ts_out.clone(),
                bindings_path: bindings.clone(),
            }),
            GeneratorConfig::Client(ClientGenerator::AdminRegistry { output: admin_out.clone() }),
        ],
        rustfmt_edition: "2024".to_string(),
        sse_route_overrides: HashMap::new(),
        ts_skip_commands: vec![],
        route_prefix: Some(RoutePrefix {
            segments: "projects/:project_id".to_string(),
            state_accessor: "store_for".to_string(),
            params: vec![PrefixParam {
                name: "project_id".to_string(),
                rust_type: "uuid::Uuid".to_string(),
                ts_type: "string".to_string(),
            }],
        }),
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        schema_entities: Vec::new(),
    };

    let modules = crate::servers::generate_transport(&config).expect("generate_transport failed");

    // Should find a reasonable number of modules
    assert!(modules.len() >= 5, "Expected at least 5 API modules from real API dir, got {}", modules.len());

    // All output files should exist and be non-empty
    for (path, label) in [
        (&http_out, "HTTP"),
        (&ipc_out, "IPC"),
        (&mcp_out, "MCP"),
        (&ts_out, "TS Transport"),
        (&admin_out, "Admin Registry"),
    ] {
        assert!(path.exists(), "{} output file should exist", label);
        let content = std::fs::read_to_string(path).unwrap();
        assert!(!content.is_empty(), "{} output should not be empty", label);
    }

    // HTTP should have entity_routes
    let http = std::fs::read_to_string(&http_out).unwrap();
    assert!(http.contains("pub fn entity_routes()"));
    assert!(http.contains("Router::new()"));

    // IPC should have tauri commands
    let ipc = std::fs::read_to_string(&ipc_out).unwrap();
    assert!(ipc.contains("#[tauri::command]"));

    // MCP should have tool registry
    let mcp = std::fs::read_to_string(&mcp_out).unwrap();
    assert!(mcp.contains("pub fn generated_tool_registry()"));

    // TS should have Transport interface and both implementations
    let ts = std::fs::read_to_string(&ts_out).unwrap();
    assert!(ts.contains("export interface Transport"));
    assert!(ts.contains("createHttpTransport"));
    assert!(ts.contains("createIpcTransport"));

    // Admin should have entity registry
    let admin = std::fs::read_to_string(&admin_out).unwrap();
    assert!(admin.contains("export const adminEntities"));

    // Verify module names include known entities
    let module_names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();
    for expected in &["capability", "agent", "role"] {
        assert!(
            module_names.contains(expected),
            "Expected module '{}' in parsed modules: {:?}",
            expected,
            module_names
        );
    }

    eprintln!("E2E test passed: {} modules → 5 outputs generated successfully", modules.len());
}

#[test]
fn test_e2e_scan_real_api_modules() {
    let api_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/src/api/v1");
    if !api_dir.exists() {
        eprintln!("Skipping: API dir not found at {}", api_dir.display());
        return;
    }

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));

    assert!(modules.len() >= 5, "Expected at least 5 modules, found {}", modules.len());

    // Check that known modules are present and have correct shapes
    for m in &modules {
        if m.functions.is_empty() && m.events.is_empty() {
            panic!("Module '{}' has no functions or events", m.name);
        }

        // Store-based CRUD modules should be consistent
        let has_list = m.functions.iter().any(|f| f.name == "list");
        let has_create = m.functions.iter().any(|f| f.name == "create");
        if has_list && has_create {
            // Full CRUD entity — should have all 5
            let fn_names: Vec<&str> = m.functions.iter().map(|f| f.name.as_str()).collect();
            assert!(
                fn_names.contains(&"list")
                    && fn_names.contains(&"get_by_id")
                    && fn_names.contains(&"create")
                    && fn_names.contains(&"update")
                    && fn_names.contains(&"delete"),
                "Module '{}' has list+create but not full CRUD: {:?}",
                m.name,
                fn_names
            );
        }
    }

    // Count store-based vs state-based modules
    let store_count = modules.iter().filter(|m| m.functions.first().is_some_and(|f| f.first_param_is_store)).count();
    let state_count = modules.iter().filter(|m| m.functions.first().is_some_and(|f| !f.first_param_is_store)).count();

    eprintln!(
        "Scanned {} modules: {} store-based, {} state-based, {} event-only",
        modules.len(),
        store_count,
        state_count,
        modules.len() - store_count - state_count,
    );
}
