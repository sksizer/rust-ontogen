//! Proof that the generated MCP tool registry compiles and behaves: each
//! kind of tool the pilot's API has (CRUD, custom GET and POST, junction
//! ops, CRUD in a module without an entity, and filtered lists) is called
//! through its generated handler over a real temp vault.
//!
//! The pilot paginates every module with `default_limit: 2, max_limit: 3`.

use std::collections::BTreeSet;

use markdown_pilot::api::transport::mcp::generated::{generated_tool_registry, handle_tool_call, tool_definitions};
use markdown_pilot::persistence::markdown::generated::open_vault;
use markdown_pilot::schema::Section;
use markdown_pilot::{AppState, Store};
use serde_json::{Value, json};

/// A vault in a tempdir behind the state the tools run against.
struct Server {
    _dir: tempfile::TempDir,
    state: AppState,
}

impl Server {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(open_vault(dir.path()));
        Server { _dir: dir, state: AppState::new(store) }
    }

    /// Calls the tool `name` with `args` through its generated handler.
    async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        let registry = generated_tool_registry();
        let tool = registry.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no tool {name}"));
        (tool.handler)(&self.state, &args).await
    }

    async fn ok(&self, name: &str, args: Value) -> Value {
        self.call(name, args.clone()).await.unwrap_or_else(|e| panic!("{name} {args}: {e}"))
    }

    async fn err(&self, name: &str, args: Value) -> String {
        match self.call(name, args.clone()).await {
            Ok(value) => panic!("{name} {args} answered {value}"),
            Err(e) => e,
        }
    }

    async fn section(&self, id: &str, title: &str, parent_id: &str) {
        let section = Section { id: id.into(), title: title.into(), parent_id: parent_id.into(), children: vec![] };
        self.state.store.create_section(section).await.expect("create section");
    }

    /// An outline: `root` is its own parent, `usage` has two children.
    async fn seed_sections(&self) {
        self.section("root", "Root", "root").await;
        self.section("intro", "Introduction", "root").await;
        self.section("usage", "Usage", "root").await;
        self.section("install", "Install it", "usage").await;
        self.section("cli", "Usage on the CLI", "usage").await;
    }
}

/// The ids of a list tool's items, in order.
fn ids(page: &Value) -> Vec<&str> {
    let items = page.get("items").unwrap_or(page);
    items.as_array().expect("items").iter().map(|i| i["id"].as_str().expect("id")).collect()
}

/// The tool's input schema's property names.
fn properties(tool: &str) -> BTreeSet<String> {
    let definitions = tool_definitions();
    let tool = definitions.iter().find(|t| t.name == tool).unwrap_or_else(|| panic!("no tool {tool}"));
    tool.input_schema["properties"].as_object().expect("properties").keys().cloned().collect()
}

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

/// `section::list` takes a `ListSectionsQuery` that refuses unknown fields
/// beside a required bare `parent_id` and the page: the struct is read from
/// the arguments the tool does not read itself.
#[tokio::test]
async fn a_list_reads_a_strict_filter_struct_beside_a_bare_filter_and_the_page() {
    let server = Server::new();
    server.seed_sections().await;

    let page = server.ok("section_list", json!({ "parent_id": "root", "title_contains": "o", "limit": 1 })).await;
    assert_eq!(page, json!({ "items": page["items"], "total": 2, "limit": 1, "offset": 0 }));
    assert_eq!(ids(&page), ["intro"]);
    let page = server.ok("section_list", json!({ "parent_id": "root", "title_contains": "o", "offset": 1 })).await;
    assert_eq!(ids(&page), ["root"]);

    // Every struct field with the bare filter and the page.
    let args = json!({ "parent_id": "usage", "title_contains": "on the", "min_children": 0, "max_children": 0,
                       "limit": 3, "offset": 0 });
    let page = server.ok("section_list", args).await;
    assert_eq!(ids(&page), ["cli"]);
    assert_eq!(page["total"], 1);

    // The bare filter alone: the struct reads as every field absent.
    let page = server.ok("section_list", json!({ "parent_id": "root" })).await;
    assert_eq!(ids(&page), ["intro", "root"]);
    assert_eq!(page["total"], 3);
}

#[tokio::test]
async fn a_list_refuses_an_argument_it_does_not_read() {
    let server = Server::new();
    server.seed_sections().await;

    let e = server.err("section_list", json!({ "parent_id": "root", "colour": "red" })).await;
    assert_eq!(e, "Unknown argument: colour");
    // The same on a list with optional bare filters only, and one with no
    // filter: an unread argument is never an unfiltered list.
    assert_eq!(server.err("tag_list", json!({ "colour": "red" })).await, "Unknown argument: colour");
    assert_eq!(server.err("note_list", json!({ "colour": "red" })).await, "Unknown argument: colour");
}

#[tokio::test]
async fn a_list_reports_a_missing_or_mistyped_filter() {
    let server = Server::new();
    server.seed_sections().await;

    let e = server.err("section_list", json!({ "title_contains": "o" })).await;
    assert_eq!(e, "Missing required parameter: parent_id");
    // A string filter's type error words its type as every other read does.
    let e = server.err("section_list", json!({ "parent_id": 5 })).await;
    assert_eq!(e, "Invalid parameter parent_id: invalid type: integer `5`, expected a string");
    let e = server.err("section_list", json!({ "parent_id": "root", "min_children": "one" })).await;
    assert!(e.starts_with("Invalid filter: invalid type: string \"one\""), "{e}");
    let e = server.err("tag_list", json!({ "min_title_len": -1 })).await;
    assert!(e.starts_with("Invalid parameter min_title_len: invalid value: integer `-1`"), "{e}");
}

/// The input schema names the bare filter, every field of the struct (the
/// generated `OntogenSectionListFilter` flattens it) and the page, and
/// requires the bare filter only.
#[test]
fn a_list_advertises_every_argument_it_reads() {
    assert_eq!(
        properties("section_list"),
        names(&["parent_id", "title_contains", "min_children", "max_children", "limit", "offset"])
    );
    let definitions = tool_definitions();
    let section_list = definitions.iter().find(|t| t.name == "section_list").unwrap();
    assert_eq!(section_list.input_schema["required"], json!(["parent_id"]));

    assert_eq!(properties("tag_list"), names(&["title_prefix", "min_title_len", "limit", "offset"]));
    assert_eq!(properties("bookmark_list"), names(&["url_contains", "limit", "offset"]));
    assert_eq!(properties("outline_list"), names(&["title_contains", "limit", "offset"]));
    assert_eq!(properties("task_get_summary"), names(&["status", "verbose", "limit"]));
    assert_eq!(properties("task_capture"), names(&["input", "status"]));
}

/// Every tool, not only a list, refuses an argument its input schema does
/// not name, with the list's error, and does nothing: CRUD (in a module
/// with an entity and in one without), custom `GET` and `POST`, and
/// junction ops.
#[tokio::test]
async fn every_tool_refuses_an_argument_it_does_not_read() {
    let server = Server::new();
    let task = json!({ "title": "Write docs", "status": "open", "body": "" });
    server.ok("task_create", task.clone()).await;
    server.ok("tag_create", json!({ "title": "Docs" })).await;

    let with = |args: Value, key: &str| {
        let mut args = args;
        args.as_object_mut().expect("object").insert(key.to_string(), json!(1));
        args
    };
    for (tool, args) in [
        ("task_get_by_id", json!({ "id": "write-docs" })),
        ("task_create", json!({ "title": "Other", "status": "open", "body": "" })),
        ("task_update", json!({ "id": "write-docs", "status": "done" })),
        ("task_delete", json!({ "id": "write-docs" })),
        ("bookmark_create", json!({ "url": "https://a.example", "title": "A" })),
        ("task_get_summary", json!({ "status": "open" })),
        ("task_capture", json!({ "input": { "title": "Plan", "status": "open", "body": "" } })),
        ("task_set_state", json!({ "id": "write-docs", "state": "blocked" })),
        ("task_complete", json!({ "id": "write-docs" })),
        ("task_purge_done", json!({})),
        ("task_add_tag", json!({ "id": "write-docs", "tag_id": "docs" })),
        ("task_remove_tag", json!({ "id": "write-docs", "tag_id": "docs" })),
        ("task_list_tags", json!({ "id": "write-docs" })),
    ] {
        assert_eq!(server.err(tool, with(args, "extra")).await, "Unknown argument: extra", "{tool}");
    }

    // Nothing was written: the task is as created, with no tag, and no
    // other task or bookmark exists.
    let stored = server.ok("task_get_by_id", json!({ "id": "write-docs" })).await;
    assert_eq!((stored["title"].as_str(), stored["status"].as_str()), (Some("Write docs"), Some("open")));
    assert_eq!(server.ok("task_list", json!({})).await["total"], 1);
    assert_eq!(server.ok("task_list_tags", json!({ "id": "write-docs" })).await["total"], 0);
    assert_eq!(server.ok("bookmark_list", json!({})).await["total"], 0);

    // The body's own fields stay nested under it: one at the top level is
    // no argument of a tool that takes the body beside another argument.
    let e = server.err("task_capture", json!({ "input": task, "title": "Ship it" })).await;
    assert_eq!(e, "Unknown argument: title");
}

/// The arguments a tool's schema names are still read: a custom op and a
/// CRUD tool called with each of them answer as before.
#[tokio::test]
async fn a_tool_still_reads_every_argument_it_names() {
    let server = Server::new();
    let task = server
        .ok("task_capture", json!({ "input": { "title": "Ship it", "status": "open", "body": "" }, "status": "done" }))
        .await;
    assert_eq!(task["status"], "done");
    assert_eq!(properties("task_update"), names(&["id", "title", "status", "parent_id", "subtasks", "tags", "body"]));
    let args = json!({ "id": "ship-it", "title": "Shipped", "status": "open", "parent_id": null, "subtasks": [],
                       "tags": [], "body": "b" });
    assert_eq!(server.ok("task_update", args).await, json!({ "success": true }));
    let stored = server.ok("task_get_by_id", json!({ "id": "ship-it" })).await;
    assert_eq!((stored["title"].as_str(), stored["body"].as_str()), (Some("Shipped"), Some("b")));
    let summary = server.ok("task_get_summary", json!({ "status": "open", "verbose": true, "limit": 1 })).await;
    assert_eq!(summary["count"], 1);
}

#[tokio::test]
async fn lists_filtered_by_optional_bare_filters() {
    let server = Server::new();
    for title in ["Alpha", "Alpine Lake", "Beta", "Gamma ray"] {
        assert_eq!(server.ok("tag_create", json!({ "title": title })).await, json!({ "success": true }));
    }

    let page = server.ok("tag_list", json!({})).await;
    assert_eq!(page["total"], 4);
    assert_eq!(page["limit"], 2);
    let page = server.ok("tag_list", json!({ "title_prefix": "Al", "min_title_len": 6 })).await;
    assert_eq!(ids(&page), ["alpine-lake"]);
    assert_eq!(page["total"], 1);

    // The entity-less list that takes the store, filtered by one optional
    // bare filter.
    server.seed_sections().await;
    let page = server.ok("outline_list", json!({ "title_contains": "Us", "limit": 3 })).await;
    assert_eq!(ids(&page), ["cli", "usage"]);
    assert_eq!(page["total"], 2);
}

/// `bookmark` has no entity: its CRUD-named fns take the state.
#[tokio::test]
async fn crud_in_a_module_without_an_entity() {
    let server = Server::new();
    for (url, title) in [("https://a.example", "A"), ("https://b.example", "B"), ("https://ab.example", "AB")] {
        server.ok("bookmark_create", json!({ "url": url, "title": title })).await;
    }

    let page = server.ok("bookmark_list", json!({ "url_contains": "://a", "limit": 1 })).await;
    assert_eq!(ids(&page), ["bm-1"]);
    assert_eq!(page["total"], 2);
    assert_eq!(server.err("bookmark_list", json!({ "url": "a" })).await, "Unknown argument: url");

    server.ok("bookmark_update", json!({ "id": "bm-2", "title": "Bee" })).await;
    assert_eq!(server.ok("bookmark_get_by_id", json!({ "id": "bm-2" })).await["title"], "Bee");
    server.ok("bookmark_delete", json!({ "id": "bm-2" })).await;
    assert_eq!(server.err("bookmark_get_by_id", json!({ "id": "bm-2" })).await, "Bookmark not found: bm-2");
}

#[tokio::test]
async fn crud_tools_over_the_store() {
    let server = Server::new();
    server.ok("task_create", json!({ "title": "Write docs", "status": "open", "body": "" })).await;
    let task = server.ok("task_get_by_id", json!({ "id": "write-docs" })).await;
    assert_eq!(task["status"], "open");

    server.ok("task_update", json!({ "id": "write-docs", "status": "done" })).await;
    assert_eq!(server.ok("task_get_by_id", json!({ "id": "write-docs" })).await["status"], "done");
    let e = server.err("task_get_by_id", json!({ "id": 7 })).await;
    assert_eq!(e, "Invalid parameter id: invalid type: integer `7`, expected a string");
    assert_eq!(server.err("task_get_by_id", json!({})).await, "Missing required parameter: id");

    server.ok("task_delete", json!({ "id": "write-docs" })).await;
    assert_eq!(server.ok("task_list", json!({})).await["total"], 0);
}

/// A custom GET reads each argument as its fn declares it: a `&str`, an
/// `Option<bool>` and an `Option<u32>`.
#[tokio::test]
async fn a_custom_get_reads_typed_arguments() {
    let server = Server::new();
    for title in ["One", "Two", "Three"] {
        server.ok("task_create", json!({ "title": title, "status": "open", "body": "" })).await;
    }

    let summary = server.ok("task_get_summary", json!({ "status": "open" })).await;
    assert_eq!(summary, json!({ "status": "open", "count": 3 }));
    let summary = server.ok("task_get_summary", json!({ "status": "open", "verbose": true, "limit": 2 })).await;
    assert_eq!(summary["titles"].as_array().map(Vec::len), Some(2));

    let e = server.err("task_get_summary", json!({ "status": "open", "verbose": "yes" })).await;
    assert!(e.starts_with("Invalid parameter verbose: invalid type: string \"yes\""), "{e}");
}

/// A body beside another argument is one argument among them, named as its
/// parameter, so the body's `status` and the op's own `status` stay apart.
#[tokio::test]
async fn a_custom_post_reads_its_body_beside_another_argument() {
    let server = Server::new();
    let input = json!({ "title": "Ship it", "status": "open", "body": "" });
    let task = server.ok("task_capture", json!({ "input": input, "status": "done" })).await;
    assert_eq!((task["id"].as_str(), task["status"].as_str()), (Some("ship-it"), Some("done")));
    let task = server.ok("task_capture", json!({ "input": { "title": "Plan", "status": "open", "body": "" } })).await;
    assert_eq!(task["status"], "open");
    assert_eq!(server.err("task_capture", json!({ "status": "done" })).await, "Missing required parameter: input");
}

/// Arguments named `state` and `store` reach the op as its own, beside the
/// handler's bindings.
#[tokio::test]
async fn an_argument_may_share_a_name_with_a_handler_binding() {
    let server = Server::new();
    server.ok("task_create", json!({ "title": "Ship it", "status": "open", "body": "" })).await;

    let task =
        server.ok("task_set_state", json!({ "id": "ship-it", "state": "blocked", "store": "Ship it later" })).await;
    assert_eq!((task["status"].as_str(), task["title"].as_str()), (Some("blocked"), Some("Ship it later")));
    let task = server.ok("task_set_state", json!({ "id": "ship-it", "state": "open" })).await;
    assert_eq!((task["status"].as_str(), task["title"].as_str()), (Some("open"), Some("Ship it later")));
}

#[tokio::test]
async fn unit_and_argumentless_custom_posts() {
    let server = Server::new();
    for title in ["One", "Two"] {
        server.ok("task_create", json!({ "title": title, "status": "open", "body": "" })).await;
    }
    assert_eq!(server.ok("task_complete", json!({ "id": "one" })).await, json!({ "success": true }));
    assert_eq!(server.ok("task_purge_done", json!({})).await, json!(1));
    assert_eq!(server.ok("task_list", json!({})).await["total"], 1);
}

#[tokio::test]
async fn junction_tools() {
    let server = Server::new();
    server.ok("task_create", json!({ "title": "Write docs", "status": "open", "body": "" })).await;
    for title in ["Docs", "Urgent", "Later"] {
        server.ok("tag_create", json!({ "title": title })).await;
    }
    for tag in ["docs", "urgent", "later"] {
        server.ok("task_add_tag", json!({ "id": "write-docs", "tag_id": tag })).await;
    }

    // A junction list pages in memory.
    let page = server.ok("task_list_tags", json!({ "id": "write-docs" })).await;
    assert_eq!(ids(&page), ["docs", "urgent"]);
    assert_eq!(page["total"], 3);
    server.ok("task_remove_tag", json!({ "id": "write-docs", "tag_id": "urgent" })).await;
    let page = server.ok("task_list_tags", json!({ "id": "write-docs", "offset": 1 })).await;
    assert_eq!(ids(&page), ["later"]);
    assert_eq!(page["total"], 2);
}

/// Event ops have no MCP tool: a tool call answers once.
#[test]
fn event_ops_have_no_tool() {
    let registry = generated_tool_registry();
    assert!(registry.iter().all(|t| !t.name.contains("feed") && !t.name.contains("watch")));
}

/// The synchronous dispatcher drives a tool on a runtime of its own.
#[test]
fn handle_tool_call_dispatches_by_name() {
    let server = Server::new();
    let args = |v: Value| v.as_object().cloned().expect("object");
    handle_tool_call(&server.state, "task_create", &args(json!({ "title": "One", "status": "open", "body": "" })))
        .expect("task_create");
    let summary = handle_tool_call(&server.state, "task_get_summary", &args(json!({ "status": "open" }))).unwrap();
    assert_eq!(summary["count"], 1);
    assert_eq!(handle_tool_call(&server.state, "nope", &args(json!({}))).unwrap_err(), "Unknown tool: nope");
}
