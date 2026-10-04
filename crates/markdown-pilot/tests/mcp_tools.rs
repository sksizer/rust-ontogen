//! Proof that the generated MCP tool registry compiles and behaves: each
//! kind of tool the pilot's API has (CRUD, custom GET and POST, a lone
//! `list_X`, junction ops listing entities or ids, in a resource module or
//! not, CRUD in a module without an entity, and filtered and sorted lists)
//! is called through its generated handler over a real temp vault.
//!
//! The pilot paginates every module with `default_limit: 2, max_limit: 3`.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use markdown_pilot::api::transport::http::generated::entity_routes;
use markdown_pilot::api::transport::mcp::generated::{generated_tool_registry, handle_tool_call, tool_definitions};
use markdown_pilot::persistence::markdown::generated::open_vault;
use markdown_pilot::schema::Section;
use markdown_pilot::{AppState, Store};
use serde_json::{Value, json};
use tower::util::ServiceExt;

/// A vault in a tempdir behind the state the tools run against.
struct Server {
    _dir: tempfile::TempDir,
    state: Arc<AppState>,
}

impl Server {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::new(open_vault(dir.path()));
        Server { _dir: dir, state: Arc::new(AppState::new(store)) }
    }

    /// The JSON:API document the generated HTTP router answers for `uri`,
    /// over the same state.
    async fn http_get(&self, uri: &str) -> Value {
        let request = Request::get(uri).header(header::ACCEPT, "application/vnd.api+json").body(Body::empty()).unwrap();
        let response = entity_routes().with_state(Arc::clone(&self.state)).oneshot(request).await.expect("infallible");
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("body");
        serde_json::from_slice(&bytes).expect("JSON body")
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
        names(&["parent_id", "title_contains", "min_children", "max_children", "sort", "limit", "offset"])
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
        ("task_add_label", json!({ "id": "write-docs", "tag_id": "docs" })),
        ("task_remove_label", json!({ "id": "write-docs", "tag_id": "docs" })),
        ("task_list_labels", json!({ "id": "write-docs" })),
        ("task_list_by_status", json!({ "status": "open" })),
        ("note_list_tags", json!({ "id": "write-docs" })),
        ("note_add_tag", json!({ "id": "write-docs", "tag_id": "docs" })),
        ("board_add_task", json!({ "tag_id": "docs", "task_id": "write-docs" })),
    ] {
        assert_eq!(server.err(tool, with(args, "extra")).await, "Unknown argument: extra", "{tool}");
    }

    // Nothing was written: the task is as created, with no tag, and no
    // other task or bookmark exists.
    let stored = server.ok("task_get_by_id", json!({ "id": "write-docs" })).await;
    assert_eq!((stored["title"].as_str(), stored["status"].as_str()), (Some("Write docs"), Some("open")));
    assert_eq!(server.ok("task_list", json!({})).await["total"], 1);
    assert_eq!(server.ok("task_list_labels", json!({ "id": "write-docs" })).await["total"], 0);
    assert_eq!(server.ok("note_list_tags", json!({ "id": "write-docs" })).await["total"], 0);
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
        server.ok("task_add_label", json!({ "id": "write-docs", "tag_id": tag })).await;
    }

    // A junction list pages in memory.
    let page = server.ok("task_list_labels", json!({ "id": "write-docs" })).await;
    assert_eq!(ids(&page), ["docs", "urgent"]);
    assert_eq!(page["total"], 3);
    server.ok("task_remove_label", json!({ "id": "write-docs", "tag_id": "urgent" })).await;
    let page = server.ok("task_list_labels", json!({ "id": "write-docs", "offset": 1 })).await;
    assert_eq!(ids(&page), ["later"]);
    assert_eq!(page["total"], 2);
}

/// A junction list of ids pages its strings in memory. The tools call the
/// ops as written, with no membership read: `note_add_tag` lists a tag
/// twice when called twice.
#[tokio::test]
async fn junction_tools_listing_ids() {
    let server = Server::new();
    for tag in ["a", "b", "c", "a"] {
        assert_eq!(
            server.ok("note_add_tag", json!({ "id": "alpha", "tag_id": tag })).await,
            json!({ "success": true })
        );
    }
    let page = server.ok("note_list_tags", json!({ "id": "alpha" })).await;
    assert_eq!(page, json!({ "items": ["a", "b"], "total": 4, "limit": 2, "offset": 0 }));
    server.ok("note_remove_tag", json!({ "id": "alpha", "tag_id": "a" })).await;
    let page = server.ok("note_list_tags", json!({ "id": "alpha", "limit": 5 })).await;
    assert_eq!(page, json!({ "items": ["b", "c"], "total": 2, "limit": 3, "offset": 0 }));
}

/// Junction ops in a module with no entity page their list as any other.
#[tokio::test]
async fn junction_tools_outside_a_resource_module() {
    let server = Server::new();
    server.ok("tag_create", json!({ "title": "Docs" })).await;
    for title in ["One", "Two", "Three"] {
        server.ok("task_create", json!({ "title": title, "status": "open", "body": "" })).await;
        server.ok("board_add_task", json!({ "tag_id": "docs", "task_id": title.to_lowercase() })).await;
    }
    server.ok("board_remove_task", json!({ "tag_id": "docs", "task_id": "two" })).await;
    let page = server.ok("board_list_tasks", json!({ "tag_id": "docs" })).await;
    assert_eq!(ids(&page), ["one", "three"]);
    assert_eq!(page["total"], 2);
    assert_eq!(server.err("board_list_tasks", json!({ "tag_id": "nope" })).await, "Tag not found: nope");
}

/// A lone `list_X` is a plain read: its whole `Vec`, with no page.
#[tokio::test]
async fn a_lone_list_answers_its_plain_array() {
    let server = Server::new();
    for (title, status) in [("One", "open"), ("Two", "done"), ("Three", "open"), ("Four", "open")] {
        server.ok("task_create", json!({ "title": title, "status": status, "body": "" })).await;
    }
    let tasks = server.ok("task_list_by_status", json!({ "status": "open" })).await;
    assert!(tasks.is_array(), "{tasks}");
    assert_eq!(ids(&tasks), ["four", "one", "three"]);
    assert_eq!(properties("task_list_by_status"), names(&["status"]));
    let e = server.err("task_list_by_status", json!({ "status": "open", "limit": 1 })).await;
    assert_eq!(e, "Unknown argument: limit");
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

// ── Sort ──
//
// The generated CRUD lists and the hand-written `section_list` take an
// order; `tag_list` does not, so its schema has no `sort`.

/// The `sort` property of a list tool's input schema.
fn sort_property(tool: &str) -> Value {
    let definitions = tool_definitions();
    let tool = definitions.iter().find(|t| t.name == tool).unwrap_or_else(|| panic!("no tool {tool}"));
    tool.input_schema["properties"]["sort"].clone()
}

#[test]
fn a_sorted_list_tool_lists_its_sort_keys() {
    for (tool, keys) in [
        ("task_list", json!(["id", "-id", "title", "-title", "status", "-status"])),
        ("note_list", json!(["id", "-id", "title", "-title"])),
        ("section_list", json!(["id", "-id", "title", "-title"])),
    ] {
        let sort = sort_property(tool);
        assert_eq!(sort["type"], "array", "{tool}: {sort}");
        assert_eq!(sort["items"], json!({ "type": "string", "enum": keys }), "{tool}");
        assert!(sort["description"].as_str().is_some_and(|d| !d.is_empty()), "{tool}");
    }
    // Optional.
    let definitions = tool_definitions();
    let section_list = definitions.iter().find(|t| t.name == "section_list").unwrap();
    assert_eq!(section_list.input_schema["required"], json!(["parent_id"]));
    let task_list = definitions.iter().find(|t| t.name == "task_list").unwrap();
    assert!(task_list.input_schema.get("required").is_none_or(|r| !r.to_string().contains("sort")));
    // A list that takes no order lists no `sort`, and refuses one.
    assert!(!properties("tag_list").contains("sort"));
}

/// Every page of a list tool, walked with `limit`.
async fn walk_tool(server: &Server, tool: &str, args: Value, limit: u64) -> Vec<String> {
    let mut seen = Vec::new();
    loop {
        let mut page_args = args.clone();
        page_args["limit"] = json!(limit);
        page_args["offset"] = json!(seen.len());
        let page = server.ok(tool, page_args).await;
        seen.extend(ids(&page).into_iter().map(str::to_owned));
        if seen.len() as u64 >= page["total"].as_u64().expect("total") {
            return seen;
        }
    }
}

/// Every page of the HTTP list at `uri`, following `next`.
async fn walk_http(server: &Server, uri: &str) -> Vec<String> {
    let mut document = server.http_get(uri).await;
    let mut seen = Vec::new();
    loop {
        let data = document["data"].as_array().expect("data");
        seen.extend(data.iter().map(|r| r["id"].as_str().expect("id").to_owned()));
        let Some(next) = document["links"]["next"].as_str() else { return seen };
        document = server.http_get(next).await;
    }
}

#[tokio::test]
async fn a_list_tool_sorts_as_the_http_list_does() {
    let server = Server::new();
    for (title, status) in [
        ("Alpha", "open"),
        ("Bravo", "done"),
        ("Charlie", "open"),
        ("Delta", "blocked"),
        ("Echo", "done"),
        ("Foxtrot", "open"),
        ("Golf", "blocked"),
    ] {
        server.ok("task_create", json!({ "title": title, "status": status, "body": "" })).await;
    }

    for (sort, expected) in [
        (json!(null), ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf"]),
        (json!([]), ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf"]),
        (json!(["-title"]), ["golf", "foxtrot", "echo", "delta", "charlie", "bravo", "alpha"]),
        (json!(["status"]), ["delta", "golf", "bravo", "echo", "alpha", "charlie", "foxtrot"]),
        (json!(["-status"]), ["alpha", "charlie", "foxtrot", "bravo", "echo", "delta", "golf"]),
        (json!(["-status", "-id"]), ["foxtrot", "charlie", "alpha", "echo", "bravo", "golf", "delta"]),
        (json!(["status", "-title"]), ["golf", "delta", "echo", "bravo", "foxtrot", "charlie", "alpha"]),
    ] {
        for limit in [1, 2, 3] {
            assert_eq!(walk_tool(&server, "task_list", json!({ "sort": sort }), limit).await, expected, "{sort}");
        }
        let keys: Vec<&str> =
            sort.as_array().map(|k| k.iter().map(|k| k.as_str().unwrap()).collect()).unwrap_or_default();
        let uri = if keys.is_empty() { "/api/tasks".to_owned() } else { format!("/api/tasks?sort={}", keys.join(",")) };
        assert_eq!(walk_http(&server, &uri).await, expected, "{uri}");
    }
}

#[tokio::test]
async fn a_hand_written_list_tool_sorts_its_filtered_set() {
    let server = Server::new();
    server.seed_sections().await;
    server.section("again", "Usage", "root").await;

    let args = json!({ "parent_id": "root", "sort": ["-title"] });
    let expected = ["again", "usage", "root", "intro"];
    for limit in [1, 2, 3] {
        assert_eq!(walk_tool(&server, "section_list", args.clone(), limit).await, expected);
    }
    assert_eq!(walk_http(&server, "/api/sections?filter[parent_id]=root&sort=-title").await, expected);

    let page = server.ok("section_list", json!({ "parent_id": "root", "min_children": 1, "sort": ["-id"] })).await;
    assert_eq!(page, json!({ "items": page["items"], "total": 2, "limit": 2, "offset": 0 }));
    assert_eq!(ids(&page), ["usage", "root"]);
    let args = json!({ "parent_id": "root", "sort": ["title", "-id"], "limit": 3, "offset": 1 });
    assert_eq!(ids(&server.ok("section_list", args).await), ["root", "usage", "again"]);
}

#[tokio::test]
async fn a_bad_sort_is_the_tool_error() {
    let server = Server::new();
    server.seed_sections().await;

    // The parser's own error text, as IPC reports it.
    for (sort, error) in [
        (json!(["priority"]), "unknown sort field `priority`"),
        (json!(["-parent"]), "unknown sort field `parent`"),
        (json!(["parent.title"]), "unknown sort field `parent.title`"),
        (json!(["title", "-title"]), "sort field `title` is named twice"),
        (json!([""]), "empty sort key"),
        (json!(["-"]), "empty sort key"),
    ] {
        assert_eq!(server.err("task_list", json!({ "sort": sort })).await, error, "{sort}");
    }
    let e = server.err("section_list", json!({ "parent_id": "root", "sort": ["children"] })).await;
    assert_eq!(e, "unknown sort field `children`");

    // `sort` is read strictly: an array of strings or nothing.
    for (sort, error) in [
        (json!("title"), r#"Invalid sort: expected an array of strings, got "title""#),
        (json!("title,-id"), r#"Invalid sort: expected an array of strings, got "title,-id""#),
        (json!({ "title": "asc" }), r#"Invalid sort: expected an array of strings, got {"title":"asc"}"#),
        (json!(["title", 1]), "Invalid sort: expected a string, got 1"),
        (json!([null]), "Invalid sort: expected a string, got null"),
    ] {
        assert_eq!(server.err("task_list", json!({ "sort": sort })).await, error, "{sort}");
    }

    // A list that takes no order refuses `sort` as any unread argument.
    assert_eq!(server.err("tag_list", json!({ "sort": ["title"] })).await, "Unknown argument: sort");
}
