//! Tests for the servers module: types, classify, parse, and generators.
//!
//! Follows the same pattern as `store/tests.rs` and `api/tests.rs`:
//! real schema files from `src-tauri/src/schema/` are used where applicable,
//! with synthetic API files for parse-level tests.

use std::collections::HashMap;
use std::path::PathBuf;

use ontogen_core::ir::OpKind;

use crate::clients::ClientGenerator;
use crate::clients::config::Config as ClientsInternalConfig;
use crate::servers::classify::classify_op;
use crate::servers::config::{Config, PrefixParam, RoutePrefix, ServerGenerator};
use crate::servers::parse::{ApiFn, ApiModule, EventFn, ForcedMethod, Param};
use crate::servers::types::{
    NamingConfig, capitalize, collect_ts_import, collect_type_import, event_name, extract_input_type, forward_arg_expr,
    inner_type, ipc_arg_key, normalize_spaces, param_to_owned_type, rust_type_to_ts, snake_to_camel, strip_ref,
    to_pascal_case, ts_key, ts_param,
};

// ─── Helper ──────────────────────────────────────────────────────────────────

/// Build a minimal server-side `Config` for testing the server generators
/// (HTTP, IPC, MCP).
fn test_config(api_dir: PathBuf) -> Config {
    Config {
        api_dir,
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming: NamingConfig::default(),
        generators: vec![],
        sse_route_overrides: HashMap::new(),
        route_prefix: None,
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        pagination: None,
        extra_surfaces: Vec::new(),
        resources: Default::default(),
        enums: Vec::new(),
        error_map: None,
    }
}

/// Build a minimal client-side internal `Config` for testing the client
/// generators (TS transport, TS HTTP client, admin registry). Mirrors
/// [`test_config`] but uses [`crate::clients::config::Config`].
fn client_test_config(api_dir: PathBuf) -> ClientsInternalConfig {
    ClientsInternalConfig {
        api_dir,
        required_query_structs: Default::default(),
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming: NamingConfig::default(),
        generators: vec![],
        ts_formatter: crate::TsFormatter::None,
        sse_route_overrides: HashMap::new(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        entities: Vec::new(),
        resources: Default::default(),
        schema_enums: Vec::new(),
        label_overrides: HashMap::new(),
        pagination: None,
        pool_extra_roots: Vec::new(),
        pool_exclude_paths: Vec::new(),
        extra_surfaces: Vec::new(),
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

/// Build a client test config with route_prefix (project scoping).
/// Mirrors [`test_config_with_prefix`] but for the client-side internal config.
fn client_test_config_with_prefix(api_dir: PathBuf) -> ClientsInternalConfig {
    let mut config = client_test_config(api_dir);
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

/// Build a `Param` from a name and a type string. Panics if the type fails to
/// parse as a `syn::Type`.
fn param(name: &str, ty: &str) -> Param {
    let ty_ast: syn::Type = syn::parse_str(ty).expect("test param type must parse as syn::Type");
    Param { name: name.to_string(), ty: ty.to_string(), ty_ast }
}

/// Parse a type string into a `syn::Type` for `ApiFn::return_type_ast` in tests.
fn ty_ast(ty: &str) -> syn::Type {
    syn::parse_str(ty).expect("test return type must parse as syn::Type")
}

/// Build a simple CRUD ApiModule for testing generators.
fn make_crud_module(name: &str, is_store_based: bool) -> ApiModule {
    let list_ret = format!("Vec<{}>", capitalize(name));
    let single_ret = capitalize(name);
    let create_input = format!("Create{}Input", capitalize(name));
    let update_input = format!("Update{}Input", capitalize(name));
    ApiModule {
        name: name.to_string(),
        functions: vec![
            ApiFn {
                name: "list".to_string(),
                is_async: true,
                doc: format!("List all {}s.", name),
                return_type: list_ret.clone(),
                return_type_ast: ty_ast(&list_ret),
                first_param_is_store: is_store_based,
                ..Default::default()
            },
            ApiFn {
                name: "get_by_id".to_string(),
                is_async: true,
                doc: format!("Get a {} by ID.", name),
                params: vec![param("id", "&str")],
                return_type: single_ret.clone(),
                return_type_ast: ty_ast(&single_ret),
                first_param_is_store: is_store_based,
                ..Default::default()
            },
            ApiFn {
                name: "create".to_string(),
                is_async: true,
                doc: format!("Create a new {}.", name),
                params: vec![param("input", &create_input)],
                return_type: single_ret.clone(),
                return_type_ast: ty_ast(&single_ret),
                first_param_is_store: is_store_based,
                ..Default::default()
            },
            ApiFn {
                name: "update".to_string(),
                is_async: true,
                doc: format!("Update a {}.", name),
                params: vec![param("id", "&str"), param("input", &update_input)],
                return_type: single_ret.clone(),
                return_type_ast: ty_ast(&single_ret),
                first_param_is_store: is_store_based,
                ..Default::default()
            },
            ApiFn {
                name: "delete".to_string(),
                is_async: true,
                doc: format!("Delete a {}.", name),
                params: vec![param("id", "&str")],
                first_param_is_store: is_store_based,
                ..Default::default()
            },
        ],
        events: vec![],
        is_singleton: false,
        has_count: false,
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
                params: vec![param("parent_id", "Option<&str>")],
                return_type: "GraphSnapshot".to_string(),
                return_type_ast: ty_ast("GraphSnapshot"),
                ..Default::default()
            },
            ApiFn {
                name: "get_node_detail".to_string(),
                is_async: true,
                doc: "Get node detail.".to_string(),
                params: vec![param("node_id", "&str")],
                return_type: "NodeDetail".to_string(),
                return_type_ast: ty_ast("NodeDetail"),
                ..Default::default()
            },
        ],
        events: vec![],
        is_singleton: false,
        has_count: false,
    }
}

/// Build a module with events.
fn make_event_module() -> ApiModule {
    ApiModule {
        name: "events".to_string(),
        functions: vec![],
        events: vec![
            EventFn {
                name: "graph_updated".to_string(),
                item_type: "GraphDelta".to_string(),
                item_type_ast: syn::parse_quote!(GraphDelta),
                ..Default::default()
            },
            EventFn {
                name: "entity_changed".to_string(),
                item_type: "EntityChange".to_string(),
                item_type_ast: syn::parse_quote!(EntityChange),
                ..Default::default()
            },
        ],
        is_singleton: false,
        has_count: false,
    }
}

/// A param'd, resumable, fallible async event op:
/// `async fn vault_note_changes(state, vault_id: String, classes: Option<String>, resume: Option<String>)
///     -> Result<Receiver<LoggedChange>, AppError>`.
fn make_param_event_module() -> ApiModule {
    let param = |name: &str, ty: syn::Type| Param {
        name: name.to_string(),
        ty: crate::servers::types::norm_type(&ty),
        ty_ast: ty,
    };
    ApiModule {
        name: "vault_notes".to_string(),
        functions: vec![],
        events: vec![EventFn {
            name: "vault_note_changes".to_string(),
            is_async: true,
            params: vec![
                param("vault_id", syn::parse_quote!(String)),
                param("classes", syn::parse_quote!(Option<String>)),
                param("resume", syn::parse_quote!(Option<String>)),
            ],
            item_type: "LoggedChange".to_string(),
            item_type_ast: syn::parse_quote!(LoggedChange),
            returns_result: true,
            ..Default::default()
        }],
        is_singleton: false,
        has_count: false,
    }
}

/// Build a multi-word module with junction + custom operations, for testing
/// that HTTP routes, IPC command names, and TS transport calls stay in sync.
///
/// Models the real `destination_skills` module: a two-word module with
/// junction ops (add_skill/remove_skill/list_skills), a reverse list
/// (`list_destinations`) that has no add or remove beside it and so is a
/// custom GET, and a custom post (`publish`).
fn make_junction_module() -> ApiModule {
    ApiModule {
        name: "destination_skills".to_string(),
        functions: vec![
            // Junction add: add_skill(destination_id, skill_id)
            ApiFn {
                name: "add_skill".to_string(),
                doc: "Link a skill to a destination.".to_string(),
                params: vec![param("destination_id", "&str"), param("skill_id", "&str")],
                ..Default::default()
            },
            // Junction remove: remove_skill(destination_id, skill_id)
            ApiFn {
                name: "remove_skill".to_string(),
                doc: "Unlink a skill from a destination.".to_string(),
                params: vec![param("destination_id", "&str"), param("skill_id", "&str")],
                ..Default::default()
            },
            // Junction list (children-of-parent): list_skills(destination_id)
            ApiFn {
                name: "list_skills".to_string(),
                doc: "List skills linked to a destination.".to_string(),
                params: vec![param("destination_id", "&str")],
                return_type: "Vec<Skill>".to_string(),
                return_type_ast: ty_ast("Vec<Skill>"),
                ..Default::default()
            },
            // A lone list (reverse): list_destinations(skill_id), a custom GET
            ApiFn {
                name: "list_destinations".to_string(),
                doc: "List destinations linked to a skill.".to_string(),
                params: vec![param("skill_id", "&str")],
                return_type: "Vec<DestinationSkill>".to_string(),
                return_type_ast: ty_ast("Vec<DestinationSkill>"),
                ..Default::default()
            },
            // Custom post: publish(destination_id, skill_id, version)
            ApiFn {
                name: "publish".to_string(),
                doc: "Publish a skill to a destination.".to_string(),
                params: vec![param("destination_id", "&str"), param("skill_id", "&str"), param("version", "i64")],
                return_type: "PublishResult".to_string(),
                return_type_ast: ty_ast("PublishResult"),
                ..Default::default()
            },
        ],
        events: vec![],
        is_singleton: false,
        has_count: false,
    }
}

/// Write a synthetic API source file for parse tests.
pub(crate) fn write_synthetic_api(dir: &std::path::Path, filename: &str, content: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(filename), content).unwrap();
}

/// A store-scoped CRUD module source for `entity` (PascalCase `Entity`),
/// taking `&{store_type}` as its first parameter.
pub(crate) fn crud_module_source(entity: &str, store_type: &str) -> String {
    let pascal = capitalize(entity);
    format!(
        "pub async fn list(store: &{st}) -> Result<Vec<{p}>, anyhow::Error> {{ todo!() }}
pub async fn get_by_id(store: &{st}, id: &str) -> Result<{p}, anyhow::Error> {{ todo!() }}
pub async fn create(store: &{st}, input: Create{p}Input) -> Result<{p}, anyhow::Error> {{ todo!() }}
pub async fn update(store: &{st}, id: &str, input: Update{p}Input) -> Result<{p}, anyhow::Error> {{ todo!() }}
pub async fn delete(store: &{st}, id: &str) -> Result<(), anyhow::Error> {{ todo!() }}
",
        st = store_type,
        p = pascal,
    )
}

/// Two API surfaces under `root`: the primary one at `root/primary` with a
/// store-scoped `athlete` module and a state-scoped `workout` module of custom
/// fns, and a second one at `root/fitness` (accessor `fitness_store`, store
/// type `FitnessStore`) with CRUD `workout` and `exercise` modules. `workout`
/// exists in both, and both reference a type named `Workout`.
pub(crate) fn two_surface_fixture(root: &std::path::Path) -> Vec<crate::servers::ApiSurface> {
    let primary = root.join("primary");
    write_synthetic_api(
        &primary,
        "athlete.rs",
        "pub async fn list(store: &Store) -> Result<Vec<Athlete>, anyhow::Error> { todo!() }\n",
    );
    write_synthetic_api(
        &primary,
        "workout.rs",
        "pub async fn start(state: &AppState, input: StartWorkoutInput) -> Result<Workout, anyhow::Error> { todo!() }
pub async fn get_summary(state: &AppState, id: &str) -> Result<WorkoutSummary, anyhow::Error> { todo!() }
",
    );
    let fitness = root.join("fitness");
    write_synthetic_api(&fitness, "workout.rs", &crud_module_source("workout", "FitnessStore"));
    // `exercise` is the module the pagination tests flag, so it carries the page.
    write_synthetic_api(&fitness, "exercise.rs", &paged_crud_module_source("exercise", "FitnessStore"));

    vec![
        crate::servers::ApiSurface {
            api_dir: primary,
            service_import_path: "crate::api::v1".to_string(),
            types_import_path: "crate::schema".to_string(),
            store_accessor: None,
            store_type: Some("Store".to_string()),
            pagination: None,
            paginated_modules: Vec::new(),
            schema_dir: None,
        },
        crate::servers::ApiSurface {
            api_dir: fitness,
            service_import_path: "fitness::api".to_string(),
            types_import_path: "fitness::schema".to_string(),
            store_accessor: Some("fitness_store".to_string()),
            store_type: Some("FitnessStore".to_string()),
            pagination: None,
            paginated_modules: Vec::new(),
            schema_dir: None,
        },
    ]
}

/// A server `Config` whose primary surface is `surfaces[0]` and whose
/// `extra_surfaces` are the rest.
pub(crate) fn two_surface_config(surfaces: Vec<crate::servers::ApiSurface>) -> Config {
    let mut surfaces = surfaces.into_iter();
    let primary = surfaces.next().expect("at least one surface");
    let mut config = test_config(primary.api_dir);
    config.service_import_path = primary.service_import_path;
    config.types_import_path = primary.types_import_path;
    config.store_type = primary.store_type;
    config.pagination = primary.pagination;
    config.extra_surfaces = surfaces.collect();
    config
}

/// Every name a file's `use` items bring into scope (the leaf ident, or the
/// alias after `as`). Two entries with the same name would not compile.
pub(crate) fn imported_names(source: &str) -> Vec<String> {
    fn walk(tree: &syn::UseTree, out: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(p) => walk(&p.tree, out),
            syn::UseTree::Name(n) => out.push(n.ident.to_string()),
            syn::UseTree::Rename(r) => out.push(r.rename.to_string()),
            syn::UseTree::Group(g) => g.items.iter().for_each(|t| walk(t, out)),
            syn::UseTree::Glob(_) => {}
        }
    }
    let file = syn::parse_file(source).expect("generated file must parse");
    let mut names = Vec::new();
    for item in &file.items {
        if let syn::Item::Use(u) = item {
            walk(&u.tree, &mut names);
        }
    }
    names
}

/// `source` without whitespace or trailing commas, so an assertion on
/// generated code does not depend on where rustfmt breaks a line.
pub(crate) fn compact(source: &str) -> String {
    source.split_whitespace().collect::<String>().replace(",]", "]").replace(",)", ")")
}

// ═══════════════════════════════════════════════════════════════════════════════
// types.rs - Pure function tests
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
    collect_type_import(&ty_ast("Vec<Node>"), &mut imports);
    assert_eq!(imports, vec!["Node"]);

    collect_type_import(&ty_ast("()"), &mut imports);
    assert_eq!(imports.len(), 1, "() should not add imports");

    collect_type_import(&ty_ast("relation::Model"), &mut imports);
    assert_eq!(imports.len(), 1, "entity-qualified types should not add imports");

    collect_type_import(&ty_ast("Node"), &mut imports);
    assert_eq!(imports.len(), 1, "duplicates should not be added");

    collect_type_import(&ty_ast("Requirement"), &mut imports);
    assert_eq!(imports, vec!["Node", "Requirement"]);
}

/// Comprehensive matrix for `collect_type_import` covering OF-008 and OF-010.
///
/// Each row is `(input type string, expected imports)`. The walker must:
/// - skip primitives and prelude scalars,
/// - peel single-arg wrappers (Option, Vec, Box, Arc, Rc, Cow) and recurse,
/// - recurse into multi-arg generics (HashMap, BTreeMap, Result, …) without
///   importing the head,
/// - skip qualified paths (`crate::schema::Foo`, `relation::Model`),
/// - peel `&T` / `&mut T` references,
/// - handle nested combinations of all of the above.
#[test]
fn test_collect_type_import_matrix() {
    let cases: &[(&str, &[&str])] = &[
        ("()", &[]),
        ("String", &[]),
        ("&str", &[]),
        ("bool", &[]),
        ("i64", &[]),
        ("u128", &[]),
        ("f32", &[]),
        ("MyType", &["MyType"]),
        ("&MyType", &["MyType"]),
        ("&mut MyType", &["MyType"]),
        ("Vec<MyType>", &["MyType"]),
        ("Option<MyType>", &["MyType"]),
        ("Option<String>", &[]),
        ("Vec<Option<MyType>>", &["MyType"]),
        ("Option<Vec<MyType>>", &["MyType"]),
        ("Box<MyType>", &["MyType"]),
        ("Arc<MyType>", &["MyType"]),
        ("Rc<MyType>", &["MyType"]),
        ("HashMap<String, MyType>", &["MyType"]),
        ("HashMap<MyKey, MyValue>", &["MyKey", "MyValue"]),
        ("HashMap<String, Vec<MyType>>", &["MyType"]),
        ("BTreeMap<String, MyType>", &["MyType"]),
        ("HashSet<MyType>", &["MyType"]),
        ("BTreeSet<MyType>", &["MyType"]),
        ("Option<HashMap<String, Vec<MyType>>>", &["MyType"]),
        // A container's name without args is the consumer's type of that name.
        ("Arc", &["Arc"]),
        ("Vec<HashMap>", &["HashMap"]),
        ("crate::schema::Foo", &[]),
        ("Vec<crate::schema::Foo>", &[]),
        ("Option<crate::schema::Foo>", &[]),
    ];

    for (input, expected) in cases {
        let mut imports = Vec::new();
        collect_type_import(&ty_ast(input), &mut imports);
        let got: Vec<&str> = imports.iter().map(String::as_str).collect();
        assert_eq!(got, *expected, "input was `{}`", input);
    }
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

/// Matrix covering `param_to_owned_type` (OF-013).
///
/// The helper decides the *declared* owned form of a handler arg / struct
/// field, given the user's service-fn parameter type. It must stay in lockstep
/// with `forward_arg_expr`'s `.as_deref()` allowlist: every `&T` that the
/// forwarding side calls `.as_deref()` on must have a sized owned companion
/// here so the dereference target lines up. The DST → sized-companion mappings
/// (`&str → String`, `&[T] → Vec<T>`, `&Path → PathBuf`, `&CStr → CString`,
/// `&OsStr → OsString`) cover that allowlist.
#[test]
fn test_param_to_owned_type_matrix() {
    let cases: &[(&str, &str)] = &[
        // Plain owned types — render as-is.
        ("String", "String"),
        ("i32", "i32"),
        ("u8", "u8"),
        ("bool", "bool"),
        ("MyStruct", "MyStruct"),
        ("CreateNodeInput", "CreateNodeInput"),
        // Sized refs — just lose the `&`.
        ("&MyStruct", "MyStruct"),
        ("&crate::schema::Foo", "crate::schema::Foo"),
        // Unsized-DST refs — map to sized owned companions.
        ("&str", "String"),
        ("&[u8]", "Vec<u8>"),
        ("&[MyStruct]", "Vec<MyStruct>"),
        ("&Path", "PathBuf"),
        ("&CStr", "CString"),
        ("&OsStr", "OsString"),
        // Option<U> — recurse into U.
        ("Option<String>", "Option<String>"),
        ("Option<u8>", "Option<u8>"),
        ("Option<MyStruct>", "Option<MyStruct>"),
        ("Option<&str>", "Option<String>"),
        ("Option<&[u8]>", "Option<Vec<u8>>"),
        ("Option<&Path>", "Option<PathBuf>"),
        ("Option<&CStr>", "Option<CString>"),
        ("Option<&OsStr>", "Option<OsString>"),
        ("Option<&MyStruct>", "Option<MyStruct>"),
        // A qualified `Path`-like ident that isn't the prelude `Path` is not
        // a recognized DST — strip the `&`, keep the user's path verbatim.
        ("&my::Path", "my::Path"),
        ("Option<&my::Path>", "Option<my::Path>"),
        // Vec<&T> is left alone — owned-form recursion into container args is
        // out of scope (see OF-013 open questions). `Vec<String>` etc. work
        // correctly as IPC payloads via serde without any rewrite here.
        ("Vec<&str>", "Vec<&str>"),
    ];

    for (input, expected) in cases {
        let ty: syn::Type = syn::parse_str(input).expect("test input must parse as syn::Type");
        assert_eq!(param_to_owned_type(&ty), *expected, "input was `{}`", input);
    }
}

/// Matrix covering `forward_arg_expr` (OF-011).
///
/// The helper decides how a handler with an owned binding `name` forwards
/// the value to the user-written service fn. The big regression was
/// `Option<u8>` → unconditional `.as_deref()`; that case is explicitly
/// covered alongside owned-vs-reference and Option<&Deref>/Option<&UserType>
/// permutations.
#[test]
fn test_forward_arg_expr_matrix() {
    let cases: &[(&str, &str)] = &[
        // Plain owned types -> by value
        ("String", "name"),
        ("bool", "name"),
        ("u8", "name"),
        ("i64", "name"),
        ("MyStruct", "name"),
        ("crate::schema::Foo", "name"),
        // References -> by reference (deref-coerces in callee)
        ("&str", "&name"),
        ("&MyStruct", "&name"),
        ("&crate::schema::Foo", "&name"),
        ("&[u8]", "&name"),
        ("&Path", "&name"),
        // Option<T> owned inner -> by value (no .as_deref() bug)
        ("Option<u8>", "name"),
        ("Option<i64>", "name"),
        ("Option<bool>", "name"),
        ("Option<String>", "name"),
        ("Option<MyStruct>", "name"),
        ("Option<Vec<String>>", "name"),
        ("Option<crate::schema::Foo>", "name"),
        // Option<&Deref-target> -> .as_deref() via owned's Deref impl
        ("Option<&str>", "name.as_deref()"),
        ("Option<&[u8]>", "name.as_deref()"),
        ("Option<&[MyStruct]>", "name.as_deref()"),
        ("Option<&Path>", "name.as_deref()"),
        ("Option<&CStr>", "name.as_deref()"),
        ("Option<&OsStr>", "name.as_deref()"),
        // Option<&UserType> -> .as_ref() (no Deref required)
        ("Option<&MyStruct>", "name.as_ref()"),
        // Qualified `Path`-like idents that aren't the prelude `Path` must NOT
        // be treated as Deref targets — they are unknown user types.
        ("Option<&my::Path>", "name.as_ref()"),
    ];

    for (input, expected) in cases {
        let ty: syn::Type = syn::parse_str(input).expect("test input must parse as syn::Type");
        assert_eq!(forward_arg_expr("name", &ty), *expected, "input was `{}`", input);
    }
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
fn rust_type_to_ts_parenthesizes_unions_before_postfix() {
    // `T | null[]` parses as `T | (null[])` — the array-ness binds to the
    // wrong operand, so the emitted type claimed "a T, or an array of null".
    assert_eq!(rust_type_to_ts("Vec<Option<String>>"), "(string | null)[]");
    assert_eq!(rust_type_to_ts("Vec<Option<Node>>"), "(Node | null)[]");
    // Same hazard one level up: `| null` must apply to the whole inner union.
    assert_eq!(rust_type_to_ts("Option<Option<i32>>"), "(number | null) | null");
    // A non-union element needs no parens.
    assert_eq!(rust_type_to_ts("Vec<Vec<String>>"), "string[][]");
}

#[test]
fn rust_type_to_ts_renders_maps_as_record() {
    // The Rust spelling was returned verbatim, putting `HashMap<String, Foo>`
    // in TS type position — a type this emitter never declares.
    assert_eq!(rust_type_to_ts("HashMap<String, i32>"), "Record<string, number>");
    assert_eq!(rust_type_to_ts("BTreeMap<String, Node>"), "Record<string, Node>");
    assert_eq!(rust_type_to_ts("std::collections::HashMap<String, bool>"), "Record<string, boolean>");
    // Nested generics survive the depth-aware argument split.
    assert_eq!(rust_type_to_ts("HashMap<String, Vec<Node>>"), "Record<string, Node[]>");
}

#[test]
fn rust_type_to_ts_handles_sets_and_smart_pointers() {
    // Sets share Vec's wire shape.
    assert_eq!(rust_type_to_ts("HashSet<String>"), "string[]");
    assert_eq!(rust_type_to_ts("BTreeSet<Node>"), "Node[]");
    // Smart pointers are transparent to serde.
    assert_eq!(rust_type_to_ts("Box<Node>"), "Node");
    assert_eq!(rust_type_to_ts("Arc<Vec<String>>"), "string[]");
    assert_eq!(rust_type_to_ts("Cow<'a, str>"), "string");
}

#[test]
fn rust_type_to_ts_strips_args_from_unrecognized_generics() {
    // Every rendered name is used downstream as a bare TS identifier — in an
    // `import { … }` list or a `type X = …` placeholder. Neither accepts
    // generic syntax, so a generic must not survive as a name.
    assert_eq!(rust_type_to_ts("MyWrapper<Node>"), "MyWrapper");
    assert_eq!(rust_type_to_ts("relation::Model<T>"), "RelationModel");
}

#[test]
fn rust_type_to_ts_resolves_external_types() {
    // These reach the shared external-types table now that this emitter
    // delegates to ontogen-ts. Each used to fall through to a bare ident and
    // get stubbed as `type DateTime<Utc> = Record<string, unknown>;` — a
    // syntax error in the consumer's build for the generic ones, and a
    // silently untyped value for the rest.
    assert_eq!(rust_type_to_ts("chrono::DateTime<Utc>"), "string");
    assert_eq!(rust_type_to_ts("chrono::NaiveDate"), "string");
    assert_eq!(rust_type_to_ts("uuid::Uuid"), "string");
    assert_eq!(rust_type_to_ts("url::Url"), "string");
    assert_eq!(rust_type_to_ts("serde_json::Value"), "unknown");
    assert_eq!(rust_type_to_ts("std::path::PathBuf"), "string");
}

#[test]
fn rust_type_to_ts_covers_every_numeric_width() {
    // Only the six "common" widths were mapped; the rest shipped as bare Rust
    // idents and were stubbed `Record<string, unknown>` downstream, so a
    // `u8` field arrived in TypeScript as an object.
    for ty in ["u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize", "f32", "f64"] {
        assert_eq!(rust_type_to_ts(ty), "number", "`{ty}` should render as number");
    }
    // `char` serializes as a single-codepoint JSON string.
    assert_eq!(rust_type_to_ts("char"), "string");
}

#[test]
fn rust_type_to_ts_degrades_instead_of_failing_on_unrenderable_types() {
    // ontogen-ts is hard-error-only; this path is lenient by design, because
    // failing the Rust build over a signature type it can't render would be a
    // worse trade than shipping an untyped one. A rejected shape falls back to
    // the terminal ident, and anything that isn't a usable identifier renders
    // `unknown` rather than a token that breaks the consumer's tsc run.
    assert_eq!(rust_type_to_ts("Mutex<Node>"), "Mutex");
    assert_eq!(rust_type_to_ts("(String, i32)"), "unknown");
    assert_eq!(rust_type_to_ts("impl Iterator<Item = u8>"), "unknown");
}

#[test]
fn rust_type_to_ts_tolerates_token_stream_spacing() {
    // Types can arrive rendered from a token stream, with spaces around the
    // angle brackets and commas. The old fixed-offset slicing missed these
    // entirely and fell through to the verbatim branch.
    assert_eq!(rust_type_to_ts("Vec < String >"), "string[]");
    assert_eq!(rust_type_to_ts("HashMap < String , i32 >"), "Record<string, number>");
    assert_eq!(rust_type_to_ts("Option < Node >"), "Node | null");
}

/// Render `rust_ty` through the AST-based emitter by putting it in a struct
/// field and pulling the field's rendered type back out.
fn via_ontogen_ts(rust_ty: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("lib.rs"), format!("pub struct Probe {{ pub field: {rust_ty} }}"))
        .expect("write probe");
    let pool = ontogen_ts::scan_src_dir(dir.path()).expect("scan");
    // Pool keys name the root they came from, so a crate-root type is
    // `["crate", "Probe"]`.
    let root = ontogen_ts::TypePath::new(vec![ontogen_ts::LOCAL_CRATE_ROOT.to_string(), "Probe".to_string()])
        .expect("non-empty");
    let ts = ontogen_ts::emit(&[root], &pool, &ontogen_ts::EmitConfig::default())
        .unwrap_or_else(|errs| panic!("ontogen-ts emit failed for `{rust_ty}`: {errs:?}"));
    ts.lines()
        .find_map(|line| line.trim().strip_prefix("field: "))
        .map(|t| t.trim_end_matches(';').to_string())
        .unwrap_or_else(|| panic!("no `field:` line for `{rust_ty}` in:\n{ts}"))
}

#[test]
fn the_two_ts_emitters_agree_on_shared_shapes() {
    // ontogen renders TypeScript from two places: `rust_type_to_ts` (API
    // signatures) and ontogen-ts (the long-tail closure). Their output lands
    // in the same generated file, so a disagreement puts contradictory types
    // in front of one caller.
    //
    // `rust_type_to_ts` delegates to ontogen-ts now, so this can't drift the
    // way it did when the two carried separate type models. It stays as a
    // guard on that delegation: anything short-circuited ahead of the
    // delegate, or bolted on after it, has to keep agreeing here.
    //
    // The `via_ontogen_ts` side goes the long way round — through a real
    // scan/resolve/emit of a struct field — so this also pins that rendering
    // a type standalone matches rendering it in a declaration.
    //
    // `relation::Model` is the one deliberate exclusion: an ontogen
    // entity-naming convention ontogen-ts has no way to know about.
    let shared = [
        // Primitives, every width — not just the six the string matcher knew.
        "String",
        "bool",
        "char",
        "u8",
        "u16",
        "u32",
        "u64",
        "usize",
        "i8",
        "i16",
        "i32",
        "i64",
        "isize",
        "f32",
        "f64",
        // Containers.
        "Vec<String>",
        "Vec<Node>",
        "Option<String>",
        "Option<Node>",
        "Vec<Option<String>>",
        "Option<Option<i32>>",
        "Vec<Vec<String>>",
        "HashMap<String, i32>",
        "BTreeMap<String, Node>",
        "HashMap<String, Vec<Node>>",
        "HashSet<String>",
        "BTreeSet<Node>",
        "VecDeque<Node>",
        // Smart pointers, peeled by both.
        "Box<Node>",
        "Arc<Vec<String>>",
        "Cow<'a, str>",
        // External types — previously the documented disagreement.
        "chrono::DateTime<Utc>",
        "chrono::NaiveDate",
        "uuid::Uuid",
        "url::Url",
        "serde_json::Value",
        "std::path::PathBuf",
        // Unknown user types render as their terminal ident on both sides.
        "Node",
        "some::nested::Node",
    ];
    for rust_ty in shared {
        assert_eq!(rust_type_to_ts(rust_ty), via_ontogen_ts(rust_ty), "emitters disagree on `{rust_ty}`");
    }
}

#[test]
fn collect_ts_import_unwraps_structural_syntax() {
    // Whatever `rust_type_to_ts` renders has to reduce to bare identifiers
    // here, or it gets spliced into an `import { … }` list verbatim.
    let mut imports = Vec::new();
    collect_ts_import("(Node | null)[]", &mut imports);
    assert_eq!(imports, vec!["Node"]);

    let mut imports = Vec::new();
    collect_ts_import("Record<string, Node>", &mut imports);
    assert_eq!(imports, vec!["Node"], "Record itself is built in; its value type is not");

    let mut imports = Vec::new();
    collect_ts_import("Record<string, unknown>", &mut imports);
    assert!(imports.is_empty(), "no importable name in {imports:?}");

    let mut imports = Vec::new();
    collect_ts_import("(number | null) | null", &mut imports);
    assert!(imports.is_empty(), "no importable name in {imports:?}");

    let mut imports = Vec::new();
    collect_ts_import("Record<string, Node[]>", &mut imports);
    assert_eq!(imports, vec!["Node"]);
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
fn test_url_for_module_singleton_uses_singular() {
    let naming = NamingConfig::default();
    let m = ApiModule {
        name: "database".to_string(),
        functions: vec![],
        events: vec![],
        is_singleton: true,
        has_count: false,
    };
    assert_eq!(naming.url_for_module(&m), "database", "singleton modules must NOT be pluralized");
}

#[test]
fn test_url_for_module_entity_uses_plural() {
    let naming = NamingConfig::default();
    let m = ApiModule {
        name: "workout".to_string(),
        functions: vec![],
        events: vec![],
        is_singleton: false,
        has_count: false,
    };
    assert_eq!(naming.url_for_module(&m), "workouts", "non-singleton modules go through url_plural");
}

#[test]
fn test_url_for_module_singleton_with_underscore() {
    let naming = NamingConfig::default();
    let m = ApiModule {
        name: "auto_start".to_string(),
        functions: vec![],
        events: vec![],
        is_singleton: true,
        has_count: false,
    };
    assert_eq!(naming.url_for_module(&m), "auto-start", "singleton URL must be kebab-cased but NOT pluralized");
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
// classify.rs - Operation classification
// ═══════════════════════════════════════════════════════════════════════════════

/// Classify `f` as the only fn of its module, as a fn with no junction
/// siblings is classified.
fn classify_alone(f: &ApiFn) -> OpKind {
    let module = ApiModule {
        name: "solo".to_string(),
        functions: vec![f.clone()],
        events: vec![],
        is_singleton: false,
        has_count: false,
    };
    classify_op(&module, f)
}

#[test]
fn test_classify_crud_operations() {
    let module = make_crud_module("node", true);

    for f in &module.functions {
        let op = classify_op(&module, f);
        match f.name.as_str() {
            "list" => assert!(matches!(op, OpKind::List)),
            "get_by_id" => assert!(matches!(op, OpKind::GetById)),
            "create" => assert!(matches!(op, OpKind::Create)),
            "update" => assert!(matches!(op, OpKind::Update)),
            "delete" => assert!(matches!(op, OpKind::Delete)),
            _ => panic!("unexpected function name: {}", f.name),
        }
    }
}

/// A one-parameter `list_X` is a junction list only beside an unforced
/// `add_Y` or `remove_Y` with the same child segment; alone it is a custom
/// GET, in every transport, since each classifies with `classify_op`.
#[test]
fn a_list_is_a_junction_list_only_beside_its_add_or_remove() {
    let f = |name: &str, params: &[&str]| ApiFn {
        name: name.to_string(),
        params: params.iter().map(|p| param(p, "&str")).collect(),
        ..Default::default()
    };
    let classify = |functions: Vec<ApiFn>| {
        let m = ApiModule { name: "task".into(), functions, events: vec![], is_singleton: false, has_count: false };
        m.functions.iter().map(|g| classify_op(&m, g)).collect::<Vec<_>>()
    };
    let list = |segment: &str| OpKind::JunctionList { child_segment: segment.to_string() };

    assert_eq!(classify(vec![f("list_tags", &["id"])]), [OpKind::CustomGet]);
    assert_eq!(classify(vec![f("list_by_status", &["status"])]), [OpKind::CustomGet]);
    assert_eq!(
        classify(vec![f("list_tags", &["id"]), f("add_tag", &["id", "tag_id"])]),
        [list("tags"), OpKind::JunctionAdd { child_segment: "tags".into() }]
    );
    assert_eq!(
        classify(vec![f("remove_tag", &["id", "tag_id"]), f("list_tags", &["id"])]),
        [OpKind::JunctionRemove { child_segment: "tags".into() }, list("tags")]
    );
    assert_eq!(classify(vec![f("list_sub_tasks", &["id"]), f("add_sub_task", &["id", "t"])])[0], list("sub-tasks"));
    // A partner for another segment, or a forced one, does not count; an add
    // or remove keeps its kind either way.
    assert_eq!(
        classify(vec![f("list_tags", &["id"]), f("add_label", &["id", "l"])]),
        [OpKind::CustomGet, OpKind::JunctionAdd { child_segment: "labels".into() }]
    );
    let forced = ApiFn { force_method: Some(ForcedMethod::Post), ..f("add_tag", &["id", "tag_id"]) };
    assert_eq!(classify(vec![f("list_tags", &["id"]), forced]), [OpKind::CustomGet, OpKind::CustomPost]);
}

#[test]
fn test_classify_custom_operations() {
    let custom = make_custom_module();

    let snapshot = &custom.functions[0]; // get_graph_snapshot
    assert!(matches!(classify_op(&custom, snapshot), OpKind::CustomGet));

    let detail = &custom.functions[1]; // get_node_detail
    assert!(matches!(classify_op(&custom, detail), OpKind::CustomGet));

    // A non-get function with params should be CustomPost
    let post_fn = ApiFn {
        name: "switch_project".to_string(),
        is_async: true,
        params: vec![param("path", "&str")],
        ..Default::default()
    };
    assert!(matches!(classify_alone(&post_fn), OpKind::CustomPost));
}

/// Zero-user-param functions now classify based on the name-prefix
/// allowlist (see `KNOWN_READ_PREFIXES`): names matching a known-read
/// prefix (`get_`, `list_`, `count_`, `exists_`, `find_`, `is_`, `has_`)
/// classify as `CustomGet`; everything else defaults to `CustomPost`
/// (RFC 7231 §4.2.1 — GET is for retrieval, not action).
///
/// Earlier behaviour: any zero-user-param fn unconditionally classified as
/// `CustomGet`, which silently routed mutating action verbs (`pause`,
/// `backup`, `reset_all`) as cacheable, retryable GETs. The default
/// reversed in the task that introduced this test.
#[test]
fn test_classify_no_params_defaults_to_post() {
    // Non-read-prefixed name → CustomPost.
    let action_fn = ApiFn {
        name: "detect_installed_openers".to_string(),
        is_async: true,
        return_type: "Vec<String>".to_string(),
        return_type_ast: ty_ast("Vec<String>"),
        ..Default::default()
    };
    assert!(matches!(classify_alone(&action_fn), OpKind::CustomPost));

    // Read-prefixed name → CustomGet.
    let read_fn = ApiFn {
        name: "list_installed_openers".to_string(),
        is_async: true,
        return_type: "Vec<String>".to_string(),
        return_type_ast: ty_ast("Vec<String>"),
        ..Default::default()
    };
    assert!(matches!(classify_alone(&read_fn), OpKind::CustomGet));
}

/// Cover the full known-read-prefix allowlist for zero-user-param fns,
/// plus a sampling of action-verb names that must now route as POST.
///
/// AC-1 of the task that introduced this test: `fn pause(state)` → POST;
/// `fn get_state(state)` → GET; `fn list_items()` → GET; `fn backup(state)`
/// → POST.
#[test]
fn test_classify_zero_param_prefix_matrix() {
    fn zero_param(name: &str) -> ApiFn {
        ApiFn { name: name.to_string(), is_async: true, is_stateless: true, ..Default::default() }
    }

    // Each row: (name, expect_get).
    let cases: &[(&str, bool)] = &[
        // Every known-read prefix → CustomGet.
        ("get_state", true),
        ("list_items", true),
        ("count_widgets", true),
        ("exists_user", true),
        ("find_neighbors", true),
        ("is_ready", true),
        ("has_pending", true),
        // Bare-prefix matches (`get_` alone is unusual but ought to match).
        // We don't construct that case here because the name would be empty
        // after the prefix; the production caller never sees such a fn.
        // Action verbs / mutating handlers → CustomPost (the bug this
        // task closes — previously these silently routed as GET).
        ("pause", false),
        ("resume", false),
        ("backup", false),
        ("reset_all", false),
        ("rebuild_index", false),
        ("publish", false),
        ("snapshot", false), // noun, not a read prefix
        ("workout", false),  // noun stats handler — should rename if it's a read
    ];

    for (name, expect_get) in cases {
        let f = zero_param(name);
        let op = classify_alone(&f);
        let got_get = matches!(op, OpKind::CustomGet);
        assert_eq!(
            got_get,
            *expect_get,
            "name=`{name}` got op={op:?} (expected {})",
            if *expect_get { "CustomGet" } else { "CustomPost" }
        );
    }
}

/// Regression test for OF-016.
///
/// A `get_*` function whose first user-facing param is a body-carrying
/// custom struct must classify as `CustomPost`, not `CustomGet`. Pre-fix,
/// the classifier looked only at the name; the HTTP generator then tried
/// to extract the struct as a `Path<String>` URL segment and produced
/// uncompileable handler code. Post-fix, the classifier consults the
/// first param's AST and routes body-carrying shapes through POST.
///
/// The id-like primitive allowlist (`String`, integers, `Uuid`, etc.) and
/// `Option<…>` (query slot) keep their old behavior — the change only
/// affects custom-struct shapes.
#[test]
fn test_of016_classify_get_with_first_param_ast() {
    fn fn_with_first_param(name: &str, ty: &str) -> ApiFn {
        ApiFn {
            name: name.to_string(),
            is_async: true,
            params: vec![param("arg", ty)],
            return_type: "String".to_string(),
            return_type_ast: ty_ast("String"),
            ..Default::default()
        }
    }

    // Each row: (fn name, first param type, expected OpKind matches CustomGet).
    let cases: &[(&str, &str, bool)] = &[
        // id-like primitives — path-param shape, stays CustomGet.
        ("get_session", "String", true),
        ("get_session", "&str", true),
        ("get_session", "i32", true),
        ("get_session", "i64", true),
        ("get_session", "u64", true),
        ("get_session", "Uuid", true),
        // Option<…> — query-slot shape, stays CustomGet.
        ("get_things", "Option<String>", true),
        ("get_things", "Option<&str>", true),
        ("get_things", "Option<i64>", true),
        // Custom structs — body-carrying, must flip to CustomPost.
        ("get_filtered_sessions", "&ExportFilterRequest", false),
        ("get_summary", "&ExportRequest", false),
        ("get_filtered", "ExportFilterRequest", false),
        ("get_thing", "&crate::schema::ExportRequest", false), // qualified path
        // Generic containers in body position — body-carrying.
        ("get_aggregated", "Vec<Workout>", false),
        ("get_aggregated", "HashMap<String, Stat>", false),
    ];

    for (name, ty_str, expect_get) in cases {
        let f = fn_with_first_param(name, ty_str);
        let op = classify_alone(&f);
        let got_get = matches!(op, OpKind::CustomGet);
        assert_eq!(got_get, *expect_get, "OF-016: name=`{name}` first_param=`{ty_str}` got op={op:?}");
    }

    // Zero-param `get_*` stays CustomGet — no body to extract.
    let zero = ApiFn {
        name: "get_summary".to_string(),
        is_async: true,
        return_type: "Summary".to_string(),
        return_type_ast: ty_ast("Summary"),
        ..Default::default()
    };
    assert!(matches!(classify_alone(&zero), OpKind::CustomGet));
}

/// `force_method: Some(ForcedMethod::Post)` short-circuits the classifier
/// and returns `OpKind::CustomPost` regardless of name/param shape.
///
/// Historical context: the `#[ontogen::http::post]` escape hatch landed in
/// the same milestone that flipped the zero-user-param classifier default
/// from `CustomGet` to `CustomPost`. The attribute was the conservative
/// opt-in that shipped first, letting consumers annotate mutating
/// action-verb handlers (`pause(state)`, `reset_all(state)`, …) on the
/// OLD default. With the default now flipped, the attribute is only
/// needed to force POST on a name that DOES match a known-read prefix —
/// the zero-param-without-read-prefix case the attribute originally
/// targeted now POSTs by default.
#[test]
fn test_force_method_post_overrides_classifier() {
    fn make_fn(name: &str, params: Vec<Param>, force_method: Option<ForcedMethod>) -> ApiFn {
        ApiFn { name: name.to_string(), is_async: true, params, is_stateless: true, force_method, ..Default::default() }
    }

    // Zero-param non-read-prefix: default classifier and forced both produce CustomPost.
    let pause_unforced = make_fn("pause", vec![], None);
    assert!(matches!(classify_alone(&pause_unforced), OpKind::CustomPost));
    let pause_forced = make_fn("pause", vec![], Some(ForcedMethod::Post));
    assert!(matches!(classify_alone(&pause_forced), OpKind::CustomPost));

    // Zero-param read-prefix: default classifier says CustomGet;
    // ForcedMethod::Post flips it to CustomPost. This is the path where
    // the attribute remains load-bearing.
    let get_unforced = make_fn("get_state", vec![], None);
    assert!(matches!(classify_alone(&get_unforced), OpKind::CustomGet));
    let get_forced = make_fn("get_state", vec![], Some(ForcedMethod::Post));
    assert!(matches!(classify_alone(&get_forced), OpKind::CustomPost));

    // The override beats the named-CRUD branch too: `list` would normally
    // route as OpKind::List, but ForcedMethod::Post short-circuits before
    // name matching runs.
    let list_forced = make_fn("list", vec![], Some(ForcedMethod::Post));
    assert!(matches!(classify_alone(&list_forced), OpKind::CustomPost));

    // Unrelated case: function that would already classify as CustomPost
    // (non-read-prefix name with id-like params) — ForcedMethod::Post is a
    // no-op because the result is the same.
    let switch_forced = make_fn("switch_project", vec![param("path", "&str")], Some(ForcedMethod::Post));
    assert!(matches!(classify_alone(&switch_forced), OpKind::CustomPost));
}

/// `force_method: Some(ForcedMethod::Get)` short-circuits the classifier and
/// returns `OpKind::CustomGet` regardless of name/param shape.
///
/// The case it exists for is the asymmetry in the name heuristic:
/// `KNOWN_READ_PREFIXES` is consulted only in the `params.is_empty()` branch,
/// so among functions that take arguments only `get_` routes as a read. Six of
/// the seven read prefixes are inert the moment a handler gains a parameter,
/// which is surprising precisely because the allowlist names them.
#[test]
fn test_force_method_get_overrides_classifier() {
    fn make_fn(name: &str, params: Vec<Param>, force_method: Option<ForcedMethod>) -> ApiFn {
        ApiFn { name: name.to_string(), is_async: true, params, is_stateless: true, force_method, ..Default::default() }
    }

    // The motivating shape: a read that carries params and is named with a
    // read prefix that is not `get_`. Unforced it POSTs despite `count_`
    // being in the allowlist, because the allowlist is zero-param-only.
    let count_params = vec![param("collection_path", "Option<String>"), param("glob_pattern", "Option<String>")];
    let count_unforced = make_fn("count_matching_files", count_params.clone(), None);
    assert!(matches!(classify_alone(&count_unforced), OpKind::CustomPost));
    let count_forced = make_fn("count_matching_files", count_params, Some(ForcedMethod::Get));
    assert!(matches!(classify_alone(&count_forced), OpKind::CustomGet));

    // The same holds for the other four inert prefixes.
    for name in ["exists_note", "find_by_tag", "is_indexed", "has_children"] {
        let unforced = make_fn(name, vec![param("id", "String")], None);
        assert!(matches!(classify_alone(&unforced), OpKind::CustomPost), "{name} unforced");
        let forced = make_fn(name, vec![param("id", "String")], Some(ForcedMethod::Get));
        assert!(matches!(classify_alone(&forced), OpKind::CustomGet), "{name} forced");
    }

    // The override beats the named-CRUD branch, mirroring `Post`: `create`
    // would normally route as OpKind::Create.
    let create_forced = make_fn("create", vec![param("body", "NoteDoc")], Some(ForcedMethod::Get));
    assert!(matches!(classify_alone(&create_forced), OpKind::CustomGet));

    // It also beats the body-carrying-first-param check that demotes a
    // `get_*` to CustomPost. The override does not re-run that check — the
    // author is asserting the shape suits a GET.
    let get_body_unforced = make_fn("get_report", vec![param("req", "ReportRequest")], None);
    assert!(matches!(classify_alone(&get_body_unforced), OpKind::CustomPost));
    let get_body_forced = make_fn("get_report", vec![param("req", "ReportRequest")], Some(ForcedMethod::Get));
    assert!(matches!(classify_alone(&get_body_forced), OpKind::CustomGet));

    // No-op where the result already matches.
    let already = make_fn("get_state", vec![], Some(ForcedMethod::Get));
    assert!(matches!(classify_alone(&already), OpKind::CustomGet));
}

/// The two overrides are independent and each wins outright, so a handler
/// annotated one way never drifts into the other's classification.
#[test]
fn test_force_method_get_and_post_are_symmetric() {
    fn make_fn(name: &str, force_method: Option<ForcedMethod>) -> ApiFn {
        ApiFn {
            name: name.to_string(),
            is_async: true,
            params: vec![param("id", "String")],
            is_stateless: true,
            force_method,
            ..Default::default()
        }
    }
    let name = "count_things";
    assert!(matches!(classify_alone(&make_fn(name, None)), OpKind::CustomPost));
    assert!(matches!(classify_alone(&make_fn(name, Some(ForcedMethod::Get))), OpKind::CustomGet));
    assert!(matches!(classify_alone(&make_fn(name, Some(ForcedMethod::Post))), OpKind::CustomPost));
}

// ═══════════════════════════════════════════════════════════════════════════════
// parse.rs - API module parsing
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

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
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

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
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

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
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

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1, "should only find node.rs");
    assert_eq!(modules[0].name, "node");
}

// ═══════════════════════════════════════════════════════════════════════════════
// parse.rs - File-level skip marker (OF-012)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_parse_skip_marker_suppresses_module() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "helpers.rs",
        r#"// ontogen:skip
use crate::AppState;

pub fn list(state: &AppState) -> Result<Vec<String>, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(
        scan.modules.iter().all(|m| m.name != "helpers"),
        "marker should remove the module from the scan: got {:?}",
        scan.modules.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
}

#[test]
fn test_parse_skip_marker_suppresses_skip_records() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "helpers.rs",
        r#"// ontogen:skip

pub fn helper(x: u32) -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(scan.skips.is_empty(), "marker should silence per-fn skip records: got {:?}", scan.skips);
}

#[test]
fn test_parse_doc_comment_skip_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "helpers.rs",
        r#"//! ontogen:skip
use crate::AppState;

pub fn list(state: &AppState) -> Result<Vec<String>, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(
        scan.modules.iter().all(|m| m.name != "helpers"),
        "//! ontogen:skip should be honoured the same as // ontogen:skip"
    );
}

#[test]
fn test_parse_skip_marker_after_real_items_not_honored() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    // A `use` followed by a `pub fn` come first; the marker is buried after
    // them and must NOT take effect.
    write_synthetic_api(
        &api_dir,
        "helpers.rs",
        r#"use crate::AppState;

pub fn list(state: &AppState) -> Result<Vec<String>, anyhow::Error> { todo!() }

// ontogen:skip
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    let module = scan.modules.iter().find(|m| m.name == "helpers").expect("helpers module must still be parsed");
    assert!(
        module.functions.iter().any(|f| f.name == "list"),
        "list fn must still be parsed when the marker is buried after items"
    );
}

#[test]
fn test_parse_skip_marker_inside_doc_comment_block() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "helpers.rs",
        r#"//! Helper module for foo.
//!
//! ontogen:skip
//!
//! Internal helpers, not transport-eligible.

use crate::AppState;

pub async fn helper(state: &AppState) -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(
        scan.modules.iter().all(|m| m.name != "helpers"),
        "marker embedded in a multi-line //! block should still be honoured"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// parse.rs - File-level singleton marker (OF-002 / OF-004)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_parse_singleton_marker_sets_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "database.rs",
        r#"// ontogen:singleton
use crate::AppState;

pub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    let module = scan.modules.iter().find(|m| m.name == "database").expect("singleton module must still be scanned");
    assert!(module.is_singleton, "// ontogen:singleton should set ApiModule::is_singleton");
}

#[test]
fn test_parse_doc_comment_singleton_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "database.rs",
        r#"//! ontogen:singleton
use crate::AppState;

pub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    let module = scan.modules.iter().find(|m| m.name == "database").expect("singleton module must still be scanned");
    assert!(module.is_singleton, "//! ontogen:singleton should set ApiModule::is_singleton");
}

#[test]
fn test_parse_no_singleton_marker_defaults_false() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "workout.rs",
        r#"use crate::AppState;

pub fn list(state: &AppState) -> Result<Vec<String>, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    let module = scan.modules.iter().find(|m| m.name == "workout").expect("workout module must be scanned");
    assert!(!module.is_singleton, "modules without the marker default to is_singleton == false");
}

#[test]
fn test_parse_singleton_marker_after_real_items_not_honored() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    // A `use` plus a real `pub fn` come first; the marker is buried after them
    // and must NOT take effect, mirroring the OF-012 placement rule.
    write_synthetic_api(
        &api_dir,
        "database.rs",
        r#"use crate::AppState;

pub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }

// ontogen:singleton
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    let module = scan.modules.iter().find(|m| m.name == "database").expect("database module must still be parsed");
    assert!(!module.is_singleton, "marker buried after items must not flip is_singleton");
}

#[test]
fn test_parse_singleton_and_skip_markers_independent() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    // Both markers in the leading block. `skip` wins outright — the module is
    // dropped, so it is also not reported as a singleton anywhere.
    write_synthetic_api(
        &api_dir,
        "database.rs",
        r#"// ontogen:skip
// ontogen:singleton
use crate::AppState;

pub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(
        scan.modules.iter().all(|m| m.name != "database"),
        "// ontogen:skip should drop the file even when // ontogen:singleton is also present"
    );
}

#[test]
fn test_singleton_config_overlay_sets_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "database.rs",
        r#"use crate::AppState;

pub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let mut naming = NamingConfig::default();
    naming.singleton_modules.insert("database".to_string());

    let mut modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    crate::servers::parse::apply_singleton_overlay(&mut modules, &naming);

    let module = modules.iter().find(|m| m.name == "database").expect("database module must exist");
    assert!(module.is_singleton, "config overlay should set is_singleton even without source marker");
}

#[test]
fn test_singleton_config_overlay_idempotent_with_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "database.rs",
        r#"// ontogen:singleton
use crate::AppState;

pub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let mut naming = NamingConfig::default();
    naming.singleton_modules.insert("database".to_string());

    let mut modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    crate::servers::parse::apply_singleton_overlay(&mut modules, &naming);

    let module = modules.iter().find(|m| m.name == "database").expect("database module must exist");
    assert!(module.is_singleton, "marker + config should still produce is_singleton == true (logical OR)");
}

#[test]
fn test_singleton_config_overlay_no_effect_for_other_modules() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "database.rs",
        r#"use crate::AppState;

pub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }
"#,
    );
    write_synthetic_api(
        &api_dir,
        "workout.rs",
        r#"use crate::AppState;

pub fn list(state: &AppState) -> Result<Vec<String>, anyhow::Error> { todo!() }
"#,
    );

    let mut naming = NamingConfig::default();
    naming.singleton_modules.insert("database".to_string());

    let mut modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    crate::servers::parse::apply_singleton_overlay(&mut modules, &naming);

    let workout = modules.iter().find(|m| m.name == "workout").expect("workout module must exist");
    assert!(!workout.is_singleton, "overlay must only flip modules listed in singleton_modules");
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

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
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

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let f = &modules[0].functions[0];
    assert_eq!(f.doc, "List all roles in the system.");
}

// ═══════════════════════════════════════════════════════════════════════════════
// parse.rs - SkipRecord diagnostics (OF-001)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_skip_record_first_param_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "foo.rs",
        r#"
use tauri::State;
use std::sync::Mutex;

/// First param uses Tauri State with a wrapper that does not contain AppState.
pub fn rename(state: State<'_, Mutex<OtherKind>>, name: String) -> Result<(), anyhow::Error> {
    todo!()
}
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));

    assert!(scan.modules.is_empty(), "no fns should be kept");
    assert_eq!(scan.skips.len(), 1, "exactly one skip record");
    let rec = &scan.skips[0];
    assert_eq!(rec.fn_name, "rename");
    assert!(matches!(rec.reason, crate::servers::parse::SkipReason::FirstParamMismatch { .. }));
    if let crate::servers::parse::SkipReason::FirstParamMismatch { first_param_ty, state_type, store_type } =
        &rec.reason
    {
        assert!(first_param_ty.contains("OtherKind"), "captured the actual first-param type");
        assert_eq!(state_type, "AppState");
        assert_eq!(store_type.as_deref(), Some("Store"));
    }
}

#[test]
fn test_skip_record_self_receiver() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "foo.rs",
        r#"
/// A method-shaped fn snuck into a free-fn module.
pub fn helper(&self, id: String) -> Result<(), anyhow::Error> {
    todo!()
}
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));

    assert!(scan.modules.is_empty());
    assert_eq!(scan.skips.len(), 1);
    let rec = &scan.skips[0];
    assert_eq!(rec.fn_name, "helper");
    assert!(matches!(rec.reason, crate::servers::parse::SkipReason::SelfReceiver));
}

#[test]
fn test_skip_record_no_params() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "foo.rs",
        r#"
/// Stateless utility with no params.
pub fn cache_clear() -> Result<(), anyhow::Error> {
    todo!()
}
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));

    assert!(scan.modules.is_empty());
    assert_eq!(scan.skips.len(), 1);
    let rec = &scan.skips[0];
    assert_eq!(rec.fn_name, "cache_clear");
    assert!(matches!(rec.reason, crate::servers::parse::SkipReason::NoParams));
}

#[test]
fn test_skip_record_includes_source_file_path() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "ghost.rs", "pub fn no_params_here() -> Result<(), anyhow::Error> { todo!() }\n");

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));

    assert_eq!(scan.skips.len(), 1);
    assert!(
        scan.skips[0].file.ends_with("ghost.rs"),
        "skip record carries the source file: got {:?}",
        scan.skips[0].file
    );
}

#[test]
fn test_no_skip_record_for_private_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "foo.rs",
        r#"
fn private_helper(state: &OtherKind) -> Result<(), anyhow::Error> { todo!() }

pub async fn list(store: &Store) -> Result<Vec<String>, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));

    assert_eq!(scan.modules.len(), 1, "the pub fn keeps the module alive");
    assert_eq!(scan.modules[0].functions.len(), 1);
    assert!(scan.skips.is_empty(), "private fn is not a skip — got {:?}", scan.skips);
}

#[test]
fn test_no_skip_record_for_event_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "events.rs",
        r#"
use tokio::sync::broadcast;

pub fn graph_updated(state: &AppState) -> broadcast::Receiver<String> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));

    assert_eq!(scan.modules.len(), 1);
    assert_eq!(scan.modules[0].events.len(), 1, "event fn classified as event");
    assert!(scan.skips.is_empty(), "events are not skips");
}

#[test]
fn test_skip_record_display_formats() {
    use crate::servers::parse::{SkipReason, SkipRecord};
    use std::path::PathBuf;

    let mismatch = SkipRecord {
        file: PathBuf::from("src/api/v1/foo.rs"),
        fn_name: "rename".to_string(),
        reason: SkipReason::FirstParamMismatch {
            first_param_ty: "tauri::State<'_,Mutex<AppState>>".to_string(),
            state_type: "PumiceState".to_string(),
            store_type: Some("Store".to_string()),
        },
    };
    let s = mismatch.to_string();
    assert!(s.contains("ontogen: skipped fn `rename`"));
    assert!(s.contains("src/api/v1/foo.rs"));
    assert!(s.contains("tauri::State<'_,Mutex<AppState>>"));
    assert!(s.contains("'PumiceState'"));
    assert!(s.contains("'Store'"));

    let self_recv = SkipRecord {
        file: PathBuf::from("src/api/v1/foo.rs"),
        fn_name: "helper".to_string(),
        reason: SkipReason::SelfReceiver,
    };
    assert!(self_recv.to_string().contains("first param is `self`/`&self`"));

    let no_params = SkipRecord {
        file: PathBuf::from("src/api/v1/foo.rs"),
        fn_name: "cache_clear".to_string(),
        reason: SkipReason::NoParams,
    };
    assert!(no_params.to_string().contains("fn has no parameters"));

    // store_type=None should not emit the " or store_type '...'" suffix.
    let no_store = SkipRecord {
        file: PathBuf::from("src/api/v1/foo.rs"),
        fn_name: "x".to_string(),
        reason: SkipReason::FirstParamMismatch {
            first_param_ty: "Other".to_string(),
            state_type: "AppState".to_string(),
            store_type: None,
        },
    };
    assert!(!no_store.to_string().contains("store_type"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// parse.rs - OF-005 acceptance-table verification
//
// Each row of the "Accepted Signatures" docs table maps to one assertion below
// so the table cannot drift from runtime behaviour silently.
// ═══════════════════════════════════════════════════════════════════════════════

/// Helper: write a single fn with a given first-param signature, scan with
/// state_type="PumiceState" and store_type=Some("Store"), and return whether
/// the fn was kept (true) or skipped (false).
fn accepts(first_param: &str) -> bool {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "row.rs",
        &format!("pub fn probe({first_param}) -> Result<(), anyhow::Error> {{ todo!() }}\n"),
    );
    let scan = crate::servers::parse::scan_api_dir(&api_dir, "PumiceState", Some("Store"));
    !scan.modules.is_empty() && !scan.modules[0].functions.is_empty()
}

#[test]
fn test_of005_table_accepted_rows() {
    assert!(accepts("state: &PumiceState"), "row: &PumiceState should accept");
    assert!(accepts("state: &Arc<PumiceState>"), "row: &Arc<PumiceState> should accept");
    assert!(
        accepts("state: State<'_, Arc<PumiceState>>"),
        "row: Tauri State<'_, Arc<PumiceState>> — substring matches PumiceState, accepted (downstream may then fail to compile; see OF-005 docs)"
    );
    assert!(accepts("store: &Store"), "row: &Store should accept (store_type match)");
}

#[test]
fn test_of005_table_rejected_rows() {
    assert!(
        !accepts("state: tauri::State<'_, Mutex<AppState>>"),
        "row: State<'_, Mutex<AppState>> — no PumiceState/Store substring, must reject"
    );
}

#[test]
fn test_of005_table_store_substring_false_positive() {
    // OF-005 documents that `&StoreContext` is silently accepted because
    // "Store" is a substring of "StoreContext". This test pins that behaviour
    // so the docs and runtime can't drift.
    assert!(
        accepts("store: &StoreContext"),
        "footgun: &StoreContext matches store_type='Store' as a substring — currently accepted"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Generator integration tests - verify generated output structure
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_http_generator_crud_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config_with_prefix(tmp.path().to_path_buf());

    let modules = vec![make_crud_module("node", true)];
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // Store-based modules generate scoped handlers only, named for their
    // IPC command.
    for handler in ["node_list", "node_get_by_id", "node_create", "node_update", "node_delete"] {
        assert!(content.contains(&format!("async fn {handler}_scoped(")), "a scoped {handler}:\n{content}");
        assert!(!content.contains(&format!("async fn {handler}(")), "no unscoped {handler}:\n{content}");
    }

    // Should have entity_routes function
    assert!(content.contains("pub fn entity_routes()"));
    assert!(content.contains("Router::new()"));

    // Should have scoped route paths (axum 0.8 `{param}` syntax)
    assert!(content.contains("/api/projects/{project_id}/nodes"));

    // Should have store construction
    assert!(content.contains("state.store_for(&ontogen_scope)"));

    // Standard Axum imports. A handler opens the store through the state and
    // never names its type, so the store type is not imported.
    assert!(content.contains("use axum::"));
    assert!(content.contains("use crate::AppState"));
    assert!(!content.contains("use crate::store::Store"), "{content}");
}

#[test]
fn test_http_generator_state_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config_with_prefix(tmp.path().to_path_buf());

    let modules = vec![make_crud_module("project", false)];
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // State-based modules get unscoped routes (entity-first handler names)
    assert!(content.contains("project_list"));
    assert!(content.contains("/api/projects"));
    assert!(content.contains("project::list(&ontogen_state)"), "should pass &state directly");
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
    assert!(content.contains("OntogenSse<impl futures::Stream"));
    // No entity is in the schema, so each item is sent as `meta.result`.
    assert!(content.contains(
        "ontogen_sse_stream(\"graph-updated\", ontogen_rx, ontogen_core::events::no_id, ontogen_result_frame)"
    ));
    assert!(content.contains("event.json_data(OntogenResultFrame::new(item))"));
    assert!(content.contains("/api/events/graph"), "should use SSE route override");
    assert!(!content.contains(".ok())"), "no generated path drops a lag error");
}

#[test]
fn test_parse_parameterized_fallible_async_event_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "vault_notes.rs",
        r#"
use crate::AppState;
use tokio::sync::broadcast;

/// One vault's note changes.
pub async fn vault_note_changes(
    state: &AppState,
    vault_id: String,
    classes: Option<Vec<FileClass>>,
    resume: Option<String>,
) -> Result<broadcast::Receiver<LoggedChange>, AppError> {
    todo!()
}

/// Every job status change.
pub fn job_status_changes(state: &AppState, app: Option<String>) -> broadcast::Receiver<JobEvent> {
    todo!()
}
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let m = &modules[0];
    assert!(m.functions.is_empty(), "a Result<Receiver<T>, E> fn is an event, not a function");
    assert_eq!(m.events.len(), 2);

    let ev = &m.events[0];
    assert_eq!(ev.name, "vault_note_changes");
    assert_eq!(ev.doc, "One vault's note changes.");
    assert!(ev.is_async);
    assert!(ev.returns_result);
    assert_eq!(ev.item_type, "LoggedChange");
    let names: Vec<&str> = ev.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["vault_id", "classes", "resume"]);
    assert!(ev.is_resumable());
    assert!(!ev.is_legacy());
    assert_eq!(ev.path_params().len(), 1);
    assert_eq!(ev.query_params().len(), 2);

    let job = &m.events[1];
    assert!(!job.is_async && !job.returns_result && !job.is_resumable());
    assert_eq!(job.item_type, "JobEvent");
    assert!(job.path_params().is_empty(), "an all-optional event op has no path params");
}

#[test]
fn test_http_generator_parameterized_event() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::http::generate(&output, &[make_param_event_module()], &config);
    let content = std::fs::read_to_string(&output).unwrap();

    assert!(compact(&content).contains(&compact(
        ".route(\"/api/events/vault-note-changes/{vault_id}\", \
         axum::routing::get(vault_note_changes_sse).fallback(ontogen_allow([OntogenMethod::GET])))"
    )));
    assert!(content.contains("OntogenPath(vault_id): OntogenPath<String>"), "required param rides the path");
    assert!(content.contains("struct OntogenVaultNotesVaultNoteChangesEventQuery"), "optional params ride the query");
    assert!(content.contains("ontogen_headers.get(\"last-event-id\")"), "Last-OntogenEvent-ID feeds resume");
    assert!(content.contains(".or(ontogen_query.resume)"), "resume query param is the fallback");
    assert!(
        content.contains(
            "vault_notes::vault_note_changes(&ontogen_state, vault_id, ontogen_query.classes, ontogen_resume)"
        )
    );
    assert!(content.contains(".await"), "async event fn is awaited");
    assert!(content.contains("Result<OntogenSse<"), "fallible subscribe returns an error response");
    assert!(content.contains("ontogen_core::events::seq_id"), "resumable op writes ids");
    assert!(content.contains("event(\"lag\")"), "lag becomes an explicit frame");
}

#[test]
fn test_http_generator_scoped_parameterized_event() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config_with_prefix(tmp.path().to_path_buf());
    crate::servers::generators::http::generate(&output, &[make_param_event_module()], &config);
    let content = std::fs::read_to_string(&output).unwrap();

    assert!(content.contains("OntogenPath((ontogen_scope, vault_id)): OntogenPath<(uuid::Uuid, String)>"));
    assert!(content.contains(
        ".subscribe_vault_note_changes_for(&ontogen_scope, vault_id, ontogen_query.classes, ontogen_resume)"
    ));
    assert!(content.contains("/api/projects/{project_id}/events/vault-note-changes/{vault_id}"));
}

#[test]
fn test_ipc_generator_event_subscriptions() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::ipc::generate(&output, &[make_param_event_module()], &config);
    let content = std::fs::read_to_string(&output).unwrap();

    assert!(content.contains("pub async fn vault_note_changes_subscribe("));
    assert!(content.contains("channel: ::tauri::ipc::Channel<::ontogen_core::events::EventFrame<LoggedChange>>"));
    let flat = compact(&content);
    assert!(
        flat.contains(&compact("vault_notes::vault_note_changes(&ontogen_state, vault_id, classes, resume).await"))
    );
    assert!(
        flat.contains(&compact(".spawn(::ontogen_core::events::forward(ontogen_rx, ::ontogen_core::events::seq_id"))
    );
    assert!(content.contains("channel.send(ontogen_frame)"));
    assert!(content.contains("pub fn vault_note_changes_unsubscribe(id: u64) -> bool"));
    assert!(content.contains("EVENT_SUBSCRIPTIONS.cancel(id)"));
    assert!(content.contains("generate_handler![vault_note_changes_subscribe, vault_note_changes_unsubscribe,]"));
    assert!(!content.contains(".emit("), "a parameterized event op never emits globally");
    assert!(!content.contains("start_event_forwarding"), "no legacy op, no global forwarding");
}

#[test]
fn test_ipc_generator_keeps_global_forwarding_for_legacy_events() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::ipc::generate(&output, &[make_event_module()], &config);
    let content = std::fs::read_to_string(&output).unwrap();

    assert!(content.contains("pub fn start_event_forwarding("));
    assert!(content.contains("::tauri::Emitter::emit(&handle, \"graph-updated\", &delta)"));
    assert!(content.contains("RecvError::Lagged(skipped)"), "lag is logged and forwarding goes on");
    assert!(content.contains("pub async fn graph_updated_subscribe("), "legacy ops get subscriptions too");
    assert!(content.contains("::ontogen_core::events::no_id"));
}

#[test]
fn test_server_metadata_lists_event_routes_and_commands() {
    let config = test_config(PathBuf::from("/tmp"));
    let out = super::extract_server_metadata(&[make_param_event_module()], &config);
    assert!(out.http_routes.iter().any(|r| r.path == "/api/events/vault-note-changes/{vault_id}" && r.method == "GET"));
    let names: Vec<&str> = out.ipc_commands.iter().map(|c| c.command_name.as_str()).collect();
    assert_eq!(names, ["vault_note_changes_subscribe", "vault_note_changes_unsubscribe"]);
    assert!(out.mcp_tools.is_empty(), "MCP skips event ops");
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

    // Should generate unscoped handlers (not scoped, entity-first names)
    assert!(content.contains("node_list"), "should generate list handler");
    assert!(content.contains("node_get_by_id"), "should generate get handler");
    assert!(content.contains("node_create"), "should generate create handler");
    assert!(content.contains("node_update"), "should generate update handler");
    assert!(content.contains("node_delete"), "should generate delete handler");

    // Should construct store from state
    assert!(content.contains("state.store().await"), "should construct store from state");

    // Should borrow the constructed store (owned `Store`) to the `&Store`-taking
    // service functions.
    assert!(content.contains("node::list(&ontogen_store)"), "should pass &store to list");
    assert!(content.contains("node::get_by_id(&ontogen_store, &id)"), "should pass &store to get_by_id");
    assert!(content.contains("node::create(&ontogen_store, input)"), "should pass &store to create");
    assert!(content.contains("node::update(&ontogen_store, &id, input)"), "should pass &store to update");
    assert!(content.contains("node::delete(&ontogen_store, &id)"), "should pass &store to delete");

    // Should have CRUD routes
    assert!(content.contains("/api/nodes"), "should have list route");
    assert!(content.contains("/api/nodes/{id}"), "should have detail route");

    // Should NOT have scoped routes (no route_prefix)
    assert!(!content.contains("_scoped"), "should not have scoped handlers");
}

#[test]
fn test_axum_path_converts_colon_params_to_braces() {
    use crate::servers::generators::http::axum_path;

    // Config-facing `:name` segments become axum 0.8 `{name}` segments.
    assert_eq!(axum_path("projects/:project_id"), "projects/{project_id}");
    assert_eq!(axum_path("/api/projects/:project_id/nodes/:id"), "/api/projects/{project_id}/nodes/{id}");
    // Paths without params pass through untouched.
    assert_eq!(axum_path("/api/events/graph-updated"), "/api/events/graph-updated");
    // Already-brace-style paths pass through untouched.
    assert_eq!(axum_path("/api/nodes/{id}"), "/api/nodes/{id}");
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
fn test_http_generator_singleton_module_uses_singular_url() {
    // End-to-end: a synthetic `database.rs` with `// ontogen:singleton` plus an
    // accepted custom GET fn should produce a route at `/api/database/path`
    // (singular kebab) rather than `/api/databases/path` (the previous
    // plural-only behaviour).
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    let output = tmp.path().join("http_generated.rs");
    let config = test_config(tmp.path().to_path_buf());

    write_synthetic_api(
        &api_dir,
        "database.rs",
        r#"// ontogen:singleton
use crate::AppState;

pub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert!(
        modules.iter().any(|m| m.name == "database" && m.is_singleton),
        "scan must produce a singleton database module"
    );
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();
    assert!(content.contains("/api/database/path"), "singleton module must produce singular URL");
    assert!(
        !content.contains("/api/databases/path"),
        "singleton module must NOT produce a pluralized URL: got\n{}",
        content
    );
}

#[test]
fn test_ipc_generator_crud_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("ipc_generated.rs");
    let config = test_config_with_prefix(tmp.path().to_path_buf());

    let modules = vec![make_crud_module("node", true)];
    crate::servers::generators::ipc::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // IPC command names (entity-first: {entity}_{fn})
    assert!(content.contains("pub async fn node_list("), "list → node_list");
    assert!(content.contains("pub async fn node_get_by_id("));
    assert!(content.contains("pub async fn node_create("));
    assert!(content.contains("pub async fn node_update("));
    assert!(content.contains("pub async fn node_delete("));

    // Tauri attributes
    assert!(content.contains("#[::tauri::command]"));
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

    // MCP tool names (entity-first: {entity}_{fn})
    assert!(content.contains(r#"name: "node_list""#));
    assert!(content.contains(r#"name: "node_get_by_id""#));
    assert!(content.contains(r#"name: "node_create""#));
    assert!(content.contains(r#"name: "node_update""#));
    assert!(content.contains(r#"name: "node_delete""#));

    // Registry function
    assert!(content.contains("pub fn generated_tool_registry()"));
    assert!(content.contains("Vec<McpToolDef>"));

    // Schema helpers
    assert!(content.contains("schema_for::<EmptyInput>"));
    assert!(content.contains("schema_for::<GetByIdInput>"));
    assert!(content.contains("with_project_id_schema"));

    // Store construction
    assert!(content.contains("ontogen_state.store_for("));

    // An input struct is read from the arguments the tool does not read
    // itself, so one that refuses unknown fields still reads.
    let flat = compact(&content);
    assert!(
        flat.contains(&compact(r#"serde_json::from_value(args_without(ontogen_args, &["project_id"]))"#)),
        "{content}"
    );
    assert!(
        flat.contains(&compact(r#"serde_json::from_value(args_without(ontogen_args, &["id", "project_id"]))"#)),
        "{content}"
    );

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

    let config = client_test_config_with_prefix(tmp.path().to_path_buf());
    let modules = vec![make_crud_module("node", true)];

    crate::clients::generators::transport::generate(&output, &bindings, &modules, &config);
    let content = std::fs::read_to_string(&output).unwrap();

    // Transport interface (entity-first camelCase method names)
    assert!(content.contains("export interface Transport"));
    assert!(content.contains("nodeList("));
    assert!(content.contains("nodeGetById("));
    assert!(content.contains("nodeCreate("));
    assert!(content.contains("nodeUpdate("));
    assert!(content.contains("nodeDelete("));

    // HTTP transport
    assert!(content.contains("export function createHttpTransport(): Transport"));
    // `node` has no entity behind it, so its CRUD ops are custom ops.
    assert!(content.contains("callOp<Node>('GET', scopedPath(projectId, `/nodes/${encodeURIComponent(id)}`))"));
    assert!(content.contains("callOp<Node>('POST', scopedPath(projectId, '/nodes'), { input })"));
    assert!(
        content.contains("callOp<Node>('PATCH', scopedPath(projectId, `/nodes/${encodeURIComponent(id)}`), { input })")
    );
    assert!(content.contains("callOp<null>('DELETE', scopedPath(projectId, `/nodes/${encodeURIComponent(id)}`))"));
    assert!(!content.contains("httpPut"), "no generated route takes a PUT");

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

// ═══════════════════════════════════════════════════════════════════════════════
// transport.rs / ts_client.rs - bindings.ts fallback diagnostics (OF-006)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_transport_returns_fallback_record_for_missing_type() {
    // When `bindings.ts` doesn't export a type the generated TS surface
    // references, the emitter falls back to `Record<string, unknown>` and
    // returns a `FallbackRecord` so the build can `cargo:warning=` it.
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("generated.ts");
    let bindings = tmp.path().join("bindings.ts");

    // Bindings file is empty - every referenced type will fall back.
    std::fs::write(&bindings, "").unwrap();

    let config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_crud_module("workout", true)];

    let fallbacks = crate::clients::generators::transport::generate(&output, &bindings, &modules, &config);

    let names: Vec<&str> = fallbacks.iter().map(|r| r.type_name.as_str()).collect();
    assert!(names.contains(&"Workout"), "expected Workout fallback, got {names:?}");
    assert!(names.contains(&"CreateWorkoutInput"), "expected CreateWorkoutInput fallback, got {names:?}");
    assert!(names.contains(&"UpdateWorkoutInput"), "expected UpdateWorkoutInput fallback, got {names:?}");

    for record in &fallbacks {
        assert_eq!(record.output, output, "FallbackRecord.output should be the generator output path");
        assert_eq!(
            record.bindings_path, bindings,
            "FallbackRecord.bindings_path should be the consulted bindings file"
        );
    }

    // The placeholder is still emitted in the generated TS so callers can compile.
    let content = std::fs::read_to_string(&output).unwrap();
    assert!(content.contains("type Workout = Record<string, unknown>"));
}

/// A POST whose parameters are all `Option<T>` still sends them: the client
/// puts every argument, optional ones included, in `meta.args`, where the
/// server reads them, and sends no query string. If the two disagreed, every
/// argument would arrive `None` and the call would quietly do nothing.
#[test]
fn test_transport_post_with_only_optional_params_sends_them_as_meta_args() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("generated.ts");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "").unwrap();

    // `count_matching_files` is a read, but it carries params and is not named
    // `get_*`, so it classifies CustomPost — the read-prefix allowlist is only
    // consulted for zero-param fns.
    let module = ApiModule {
        name: "doc".to_string(),
        events: vec![],
        is_singleton: false,
        has_count: false,
        functions: vec![ApiFn {
            name: "count_matching_files".to_string(),
            is_async: true,
            params: vec![param("collection_path", "Option<String>"), param("glob_pattern", "Option<String>")],
            return_type: "u64".to_string(),
            return_type_ast: ty_ast("u64"),
            is_stateless: true,
            ..Default::default()
        }],
    };
    assert!(
        matches!(classify_op(&module, &module.functions[0]), OpKind::CustomPost),
        "precondition: this shape classifies POST, which is the whole problem"
    );

    let config = client_test_config(tmp.path().to_path_buf());
    crate::clients::generators::transport::generate(&output, &bindings, &[module], &config);
    let content = std::fs::read_to_string(&output).unwrap();

    assert!(
        content.contains(
            "callOp<number>('POST', '/docs/count-matching-files', { collection_path: collectionPath, glob_pattern: \
             globPattern });"
        ),
        "both optional params must reach `meta.args`, with no query string; emitted:\n{content}"
    );
}

#[test]
fn test_transport_no_fallback_when_all_types_exported() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("generated.ts");
    let bindings = tmp.path().join("bindings.ts");

    std::fs::write(
        &bindings,
        "export type Workout = { id: string };\n\
         export type CreateWorkoutInput = { name: string };\n\
         export type UpdateWorkoutInput = { name?: string };\n",
    )
    .unwrap();

    let config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_crud_module("workout", true)];

    let fallbacks = crate::clients::generators::transport::generate(&output, &bindings, &modules, &config);
    assert!(fallbacks.is_empty(), "expected no fallbacks when bindings exports every type, got {fallbacks:?}");
}

#[test]
fn test_ts_client_returns_fallback_record_for_missing_type() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("client.ts");
    let bindings = tmp.path().join("bindings.ts");

    std::fs::write(&bindings, "").unwrap();

    let config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_crud_module("workout", true)];

    let fallbacks = crate::clients::generators::ts_client::generate(&output, &bindings, &modules, &config);

    let names: Vec<&str> = fallbacks.iter().map(|r| r.type_name.as_str()).collect();
    assert!(names.contains(&"Workout"), "expected Workout fallback, got {names:?}");
    assert!(names.contains(&"CreateWorkoutInput"), "expected CreateWorkoutInput fallback, got {names:?}");
    assert!(names.contains(&"UpdateWorkoutInput"), "expected UpdateWorkoutInput fallback, got {names:?}");
}

/// A module whose parameters exercise the widths the old two-way guess got
/// wrong: everything that wasn't literally `i32` was typed `string`.
fn make_widths_module() -> ApiModule {
    ApiModule {
        name: "widths".to_string(),
        functions: vec![
            ApiFn {
                name: "get_by_offset".to_string(),
                is_async: true,
                doc: "Read at an offset.".to_string(),
                params: vec![param("offset", "u64"), param("depth", "u8")],
                return_type: "Node".to_string(),
                return_type_ast: ty_ast("Node"),
                ..Default::default()
            },
            ApiFn {
                name: "set_flag".to_string(),
                is_async: true,
                doc: "Set a flag.".to_string(),
                params: vec![param("enabled", "bool"), param("weight", "f64")],
                return_type: "Node".to_string(),
                return_type_ast: ty_ast("Node"),
                ..Default::default()
            },
        ],
        events: vec![],
        is_singleton: false,
        has_count: false,
    }
}

/// Pull `name: type` pairs out of a generated method signature.
fn signature_params(ts: &str, method: &str) -> Vec<String> {
    let line = ts
        .lines()
        .find(|l| l.contains(&format!("{method}(")))
        .unwrap_or_else(|| panic!("no `{method}` method in:\n{ts}"));
    let open = line.find('(').expect("method signature has an open paren");
    let close = line[open..].find(')').expect("method signature has a close paren") + open;
    line[open + 1..close].split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

#[test]
fn the_two_client_generators_type_a_parameter_the_same_way() {
    // `ts_client.rs` reduced every parameter to `if ty == "i32" { number }
    // else { string }`, while `transport.rs` ran the same parameter through
    // `rust_type_to_ts`. Both files are generated from one API definition and
    // both are importable by the same caller, so one endpoint had two
    // different signatures depending on which file you imported from — and
    // for anything wider than `i32` at least one of them was wrong.
    let tmp = tempfile::tempdir().unwrap();
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type Node = { id: string };\n").unwrap();

    let config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_widths_module()];

    let client_path = tmp.path().join("client.ts");
    let transport_path = tmp.path().join("transport.ts");
    crate::clients::generators::ts_client::generate(&client_path, &bindings, &modules, &config);
    crate::clients::generators::transport::generate(&transport_path, &bindings, &modules, &config);

    let client_ts = std::fs::read_to_string(&client_path).unwrap();
    let transport_ts = std::fs::read_to_string(&transport_path).unwrap();

    for method in ["widthGetByOffset", "widthSetFlag"] {
        let from_client = signature_params(&client_ts, method);
        let from_transport = signature_params(&transport_ts, method);
        assert_eq!(from_client, from_transport, "generators disagree on `{method}` parameters");
    }

    // And the shared answer is the correct one, not the old `string` guess.
    let offset = signature_params(&client_ts, "widthGetByOffset");
    assert_eq!(offset, vec!["offset: number", "depth: number"], "numeric path params should be numbers");
    let flag = signature_params(&client_ts, "widthSetFlag");
    assert_eq!(flag, vec!["enabled: boolean", "weight: number"], "body fields should keep their real types");
}

#[test]
fn test_fallback_record_display_format() {
    use crate::clients::generators::FallbackRecord;
    use std::path::PathBuf;

    let record = FallbackRecord {
        output: PathBuf::from("src/generated/transport.ts"),
        bindings_path: PathBuf::from("src/generated/bindings.ts"),
        type_name: "Workout".to_string(),
    };
    let msg = format!("{record}");
    // The Display impl is what the build emits as `cargo:warning=<msg>`,
    // so its shape is part of the user-visible contract.
    assert!(msg.contains("ontogen:"), "warning should be ontogen-prefixed, got: {msg}");
    assert!(msg.contains("'Workout'"), "warning should quote the missing type name, got: {msg}");
    assert!(msg.contains("Record<string, unknown>"), "warning should mention the placeholder, got: {msg}");
    assert!(msg.contains("bindings.ts"), "warning should reference the bindings file, got: {msg}");
    assert!(msg.contains("transport.ts"), "warning should reference the output file, got: {msg}");
}

#[test]
fn test_admin_registry_generator() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("admin-registry.ts");
    let config = client_test_config(tmp.path().to_path_buf());

    let modules = vec![
        make_crud_module("node", true),
        make_crud_module("agent", true),
        make_custom_module(), // non-CRUD should be excluded
    ];

    crate::clients::generators::admin::generate(&output, &modules, &config, &config.entities, &config.schema_enums);
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

    // Transport method names (entity-first camelCase)
    assert!(content.contains("listMethod: 'nodeList'"));
    assert!(content.contains("getMethod: 'nodeGetById'"));
    assert!(content.contains("createMethod: 'nodeCreate'"));
    assert!(content.contains("updateMethod: 'nodeUpdate'"));
    assert!(content.contains("deleteMethod: 'nodeDelete'"));

    // Type references
    assert!(content.contains("returnType: 'Node'"));
    assert!(content.contains("createInputType: 'CreateNodeInput'"));
    assert!(content.contains("updateInputType: 'UpdateNodeInput'"));

    // Lookup maps
    assert!(content.contains("export const adminEntityMap"));
    assert!(content.contains("export const adminEntityByPlural"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Junction cross-transport integration tests
// ═══════════════════════════════════════════════════════════════════════════════
//
// These tests guard the contract that HTTP routes, IPC command names, and TS
// transport call sites stay in sync for junction and custom operations on
// multi-word modules. Historically these drifted because ipc.rs used a plural
// module prefix (`destination_skills_add_skill`) while http.rs and transport.rs
// used the singular entity-first form (`destination_skill_add_skill`), causing
// Tauri invoke calls to fail with "unknown command".
//
// Each test generates a single output and asserts on substring presence.
// `test_junction_cross_transport_consistency` then ties them together by
// asserting the same identifier appears across ipc.rs, transport.rs (IPC
// branch), and the Rust HTTP route for http.rs.

#[test]
fn test_ipc_generator_junction_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());

    let modules = vec![make_junction_module()];
    crate::servers::generators::ipc::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // Junction functions must use the singular entity-first form
    // (`destination_skill_*`), matching what `command_name()` produces.
    // They must NOT use the plural module name prefix
    // (`destination_skills_*`) that the TS IPC transport will not invoke.
    assert!(
        content.contains("pub async fn destination_skill_add_skill("),
        "JunctionAdd should use singular entity prefix: destination_skill_add_skill"
    );
    assert!(
        content.contains("pub async fn destination_skill_remove_skill("),
        "JunctionRemove should use singular entity prefix: destination_skill_remove_skill"
    );
    assert!(
        content.contains("pub async fn destination_skill_list_skills("),
        "JunctionList (forward) should use singular entity prefix: destination_skill_list_skills"
    );
    assert!(
        content.contains("pub async fn destination_skill_list_destinations("),
        "JunctionList (reverse) should use singular entity prefix: destination_skill_list_destinations"
    );
    assert!(
        content.contains("pub async fn destination_skill_publish("),
        "CustomPost should use singular entity prefix: destination_skill_publish"
    );

    // Negative: no plural module-prefixed forms should leak through.
    assert!(
        !content.contains("destination_skills_add_skill"),
        "regression: plural-prefixed junction name leaked into IPC output"
    );
    assert!(
        !content.contains("destination_skills_publish"),
        "regression: plural-prefixed custom method name leaked into IPC output"
    );

    // The generate_handlers! registration block must list the same names so
    // Tauri can dispatch the invoke calls.
    assert!(content.contains("destination_skill_add_skill,"));
    assert!(content.contains("destination_skill_remove_skill,"));
    assert!(content.contains("destination_skill_publish,"));
}

#[test]
fn test_http_generator_junction_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http_generated.rs");
    let config = test_config(tmp.path().to_path_buf());

    let modules = vec![make_junction_module()];
    crate::servers::generators::http::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();

    // Junction routes must use the kebab-case URL plural (`destination-skills`)
    // and the child segment (`skills`), NOT the snake_case module name or
    // action-style URLs (`/destination_skills/add-skill`).
    assert!(
        content.contains("/api/destination-skills/{parent_id}/skills"),
        "junction URL should use kebab-case plural and child segment"
    );
    assert!(
        content.contains("/api/destination-skills/list-destinations/{skill_id}"),
        "a list with no add or remove beside it is a custom GET at its action route:\n{content}"
    );
    assert!(
        content.contains("/api/destination-skills/{parent_id}/skills/{child_id}"),
        "junction remove URL should include child_id path segment"
    );

    // Handler functions should use singular entity prefix.
    assert!(content.contains("async fn destination_skill_add_skill("));
    assert!(content.contains("async fn destination_skill_list_skills("));

    // Served as custom ops (§10.4): the child id is `meta.args.{child param}`,
    // and add and remove answer 204.
    let flat = compact(&content);
    assert!(
        flat.contains(&compact(
            "let skill_id = ontogen_jsonapi::request::op_arg::<String>(&ontogen_args, \"skill_id\", true)?;"
        )),
        "the child id is read from meta.args:\n{content}"
    );
    assert!(flat.contains(&compact("ontogen_jsonapi::request::check_op_arg_names(&ontogen_args, &[\"skill_id\"])?;")));
    assert!(
        flat.contains(&compact(
            "OntogenPath((destination_id, skill_id)): OntogenPath<(String, String)>, _: OntogenQuery<OntogenNoParams>"
        )),
        "remove takes both ids from the path and no body:\n{content}"
    );
    assert_eq!(
        content.matches("Ok(ontogen_jsonapi::response::no_content())").count(),
        2,
        "add and remove are 204s:\n{content}"
    );
    assert!(
        content.contains("Ok(ontogen_jsonapi::response::ok(&OntogenDocument::meta_only(OntogenResultMeta { result: ontogen_result })))"),
        "lists are meta.result"
    );
    assert!(!content.contains("Json"), "no flat bodies remain:\n{content}");

    // Regression guards - no old-style custom URLs.
    assert!(
        !content.contains("/api/destination_skills/add-skill"),
        "a junction add is served at its nested route `{{base}}/{{parent_id}}/{{segment}}`, never at an action route:\n{content}"
    );
    assert!(
        !content.contains("/api/destination_skills/"),
        "regression: snake_case plural leaked into HTTP routes (should be kebab-case)"
    );
}
/// Build a module with a custom POST whose optional param is declared *after*
/// a required one. Models SDF's `setup_project(project_id, install_harness)`.
fn make_post_trailing_optional_module() -> ApiModule {
    ApiModule {
        name: "project".to_string(),
        functions: vec![ApiFn {
            name: "setup".to_string(),
            is_async: true,
            doc: "Set a project up.".to_string(),
            params: vec![param("project_id", "&str"), param("install_harness", "Option<&str>")],
            return_type: "ProjectSetupResult".to_string(),
            return_type_ast: ty_ast("ProjectSetupResult"),
            ..Default::default()
        }],
        events: vec![],
        is_singleton: false,
        has_count: false,
    }
}

#[test]
fn test_custom_post_keeps_declaration_order_across_transports() {
    // Pre-fix the TS emitters grouped the `Option<_>` query params ahead of the
    // body ones, so this fn's client read `(installHarness, projectId)` while
    // the IPC impl — which forwards `f.params` verbatim and can emit nothing
    // but declaration order — read `(projectId, installHarness)`. The Transport
    // interface sided with the HTTP impl, so the IPC impl failed to typecheck
    // and no Rust signature satisfied every transport at once.
    let tmp = tempfile::tempdir().unwrap();
    let transport_out = tmp.path().join("transport.ts");
    let client_out = tmp.path().join("client.ts");
    let mcp_out = tmp.path().join("mcp.rs");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type ProjectSetupResult = { ok: boolean };\n").unwrap();

    let config = test_config(tmp.path().to_path_buf());
    let client_config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_post_trailing_optional_module()];

    crate::clients::generators::transport::generate(&transport_out, &bindings, &modules, &client_config);
    crate::clients::generators::ts_client::generate(&client_out, &bindings, &modules, &client_config);
    crate::servers::generators::mcp::generate(&mcp_out, &modules, &config);

    let transport = std::fs::read_to_string(&transport_out).unwrap();
    let client = std::fs::read_to_string(&client_out).unwrap();
    let mcp = std::fs::read_to_string(&mcp_out).unwrap();

    // Formatter-agnostic: collapse all whitespace.
    let declared = "projectSetup(projectId:string,installHarness:string|null)";
    let hoisted = "projectSetup(installHarness:string|null,projectId:string)";
    for (name, content) in [("transport.ts", &transport), ("ts_client.ts", &client)] {
        let compact: String = content.split_whitespace().collect();
        assert!(
            !compact.contains(hoisted),
            "regression: {name} hoisted the optional param ahead of the required one:\n{content}"
        );
        assert!(compact.contains(declared), "{name} must emit the params in Rust declaration order, got:\n{content}");
    }

    // transport.ts carries three signatures for the one fn — the Transport
    // interface, the HTTP impl and the IPC impl — and all three must agree.
    let transport_compact: String = transport.split_whitespace().collect();
    assert_eq!(
        transport_compact.matches(declared).count(),
        3,
        "interface, HTTP impl and IPC impl must each carry the declared order:\n{transport}"
    );

    // The MCP handler calls the Rust fn positionally, so it pins the order the
    // TS surfaces are being held to.
    assert!(
        mcp.contains("project_id, install_harness.as_deref()"),
        "MCP handler must call the service fn in declaration order:\n{mcp}"
    );
}

#[test]
fn test_http_generator_junction_routes_deterministic_across_runs() {
    // Junction routes are collected into a map keyed by URL path, then
    // iterated to emit `.route(...)` lines. If that map is a HashMap, the
    // iteration order changes between runs (per-instance RandomState), which
    // makes consumers of `write_if_changed` rewrite the output every cargo
    // invocation — and on `tauri dev`, that causes an infinite rebuild
    // loop because the file watcher sees the new mtime.
    //
    // Generate twice (two separate map instances inside one process) and
    // assert the byte-for-byte output is identical. Pre-fix this fails on
    // every other run; post-fix (BTreeMap) it holds.
    let tmp = tempfile::tempdir().unwrap();
    let out_a = tmp.path().join("a.rs");
    let out_b = tmp.path().join("b.rs");
    let config = test_config(tmp.path().to_path_buf());
    let modules = vec![make_junction_module()];

    crate::servers::generators::http::generate(&out_a, &modules, &config);
    crate::servers::generators::http::generate(&out_b, &modules, &config);

    let a = std::fs::read_to_string(&out_a).unwrap();
    let b = std::fs::read_to_string(&out_b).unwrap();
    assert_eq!(a, b, "http.rs generator output must be byte-identical across runs with identical input");
}

#[test]
fn test_ts_transport_junction_module() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("generated.ts");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(
        &bindings,
        "export type Skill = { id: string };\n\
         export type DestinationSkill = { destination_id: string; skill_id: string };\n\
         export type PublishResult = { ok: boolean };\n",
    )
    .unwrap();

    let config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_junction_module()];

    crate::clients::generators::transport::generate(&output, &bindings, &modules, &config);
    let content = std::fs::read_to_string(&output).unwrap();
    // Assertions are formatter-agnostic: collapse ALL whitespace so they hold
    // regardless of which TsFormatter (if any) shaped the output.
    let compact: String = content.split_whitespace().collect();

    // Transport interface method names (entity-first camelCase) + first param.
    assert!(compact.contains("destinationSkillAddSkill(destinationId:string,"));
    assert!(compact.contains("destinationSkillRemoveSkill(destinationId:string,"));
    assert!(compact.contains("destinationSkillListSkills(destinationId:string)"));
    assert!(compact.contains("destinationSkillListDestinations(skillId:string"));

    // HTTP transport must use junction URL pattern matching http.rs, NOT the
    // old action-style URLs. Formatter quote style varies, so test the core
    // path template (a backtick template literal, quote-agnostic).
    assert!(
        content.contains("/destination-skills/${encodeURIComponent(destinationId)}/skills`"),
        "HTTP junction add should POST to /destination-skills/:id/skills (kebab-case plural, encoded parent)"
    );
    assert!(
        content.contains("skill_id: skillId"),
        "HTTP junction add body should match http.rs expected field name (snake_case)"
    );
    assert!(
        content.contains(
            "/destination-skills/${encodeURIComponent(destinationId)}/skills/${encodeURIComponent(skillId)}`"
        ),
        "HTTP junction remove should DELETE the nested child URL"
    );
    assert!(
        content.contains("/destination-skills/list-destinations/${encodeURIComponent(skillId)}`"),
        "a list with no add or remove beside it is a custom GET at its action route:\n{content}"
    );

    // IPC transport must invoke the singular entity-first command names that
    // ipc.rs actually registers. Formatter quote style varies, so accept both.
    for cmd in [
        "destination_skill_add_skill",
        "destination_skill_remove_skill",
        "destination_skill_list_skills",
        "destination_skill_list_destinations",
        "destination_skill_publish",
    ] {
        let single = format!("invoke('{}'", cmd);
        let double = format!("invoke(\"{}\"", cmd);
        assert!(
            content.contains(&single) || content.contains(&double),
            "TS transport missing invoke({}) - ipc.rs would never be reached",
            cmd
        );
    }

    // Regression guards - no plural-prefixed invoke names or snake_case URLs.
    assert!(
        !content.contains("invoke('destination_skills_add_skill'")
            && !content.contains("invoke(\"destination_skills_add_skill\""),
        "regression: plural-prefixed invoke name leaked into TS IPC transport"
    );
    assert!(
        !content.contains("`/destination_skills/"),
        "regression: snake_case plural leaked into TS HTTP transport URL"
    );
    assert!(
        !content.contains("'/destination_skills/add-skill'") && !content.contains("\"/destination_skills/add-skill\""),
        "a junction add is called at its nested route `/{{base}}/{{parentId}}/{{segment}}`, never at an action route:\n{content}"
    );
}

#[test]
fn test_junction_cross_transport_consistency() {
    // Tie the three generators together: for every junction/custom method on
    // the junction module, the IPC command name registered in ipc.rs must
    // exactly match the `invoke('...')` call emitted by transport.rs, and
    // the HTTP route must match the URL fetched by transport.rs.
    //
    // This is the test that would have caught the destination_skills bug
    // at build time.

    let tmp = tempfile::tempdir().unwrap();
    let ipc_out = tmp.path().join("ipc.rs");
    let http_out = tmp.path().join("http.rs");
    let ts_out = tmp.path().join("generated.ts");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(
        &bindings,
        "export type Skill = { id: string };\n\
         export type DestinationSkill = { destination_id: string; skill_id: string };\n\
         export type PublishResult = { ok: boolean };\n",
    )
    .unwrap();

    let config = test_config(tmp.path().to_path_buf());
    let client_config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_junction_module()];

    crate::servers::generators::ipc::generate(&ipc_out, &modules, &config);
    crate::servers::generators::http::generate(&http_out, &modules, &config);
    crate::clients::generators::transport::generate(&ts_out, &bindings, &modules, &client_config);

    let ipc = std::fs::read_to_string(&ipc_out).unwrap();
    let http = std::fs::read_to_string(&http_out).unwrap();
    let ts = std::fs::read_to_string(&ts_out).unwrap();

    // Every IPC invoke in the TS transport must correspond to a registered
    // Rust `#[tauri::command]` function with the same name.
    let expected_ipc_commands = [
        "destination_skill_add_skill",
        "destination_skill_remove_skill",
        "destination_skill_list_skills",
        "destination_skill_list_destinations",
        "destination_skill_publish",
    ];
    for cmd in &expected_ipc_commands {
        let rust_decl = format!("pub async fn {}(", cmd);
        assert!(
            ipc.contains(&rust_decl),
            "IPC output missing Rust handler `{}` - TS transport would fail to invoke",
            cmd
        );
        // The formatter may quote the invoke arg with ' or " - accept either.
        let ts_single = format!("invoke('{}'", cmd);
        let ts_double = format!("invoke(\"{}\"", cmd);
        assert!(
            ts.contains(&ts_single) || ts.contains(&ts_double),
            "TS transport missing invoke({}) - IPC handler would never be called",
            cmd
        );
    }

    // The HTTP junction routes registered in http.rs must be the same URLs
    // the TS HTTP transport calls against (modulo the `/api` prefix which
    // http.rs adds but the TS helper BASE constant prepends on the TS side).
    let expected_http_junctions = [
        (
            "/api/destination-skills/{parent_id}/skills",
            "/destination-skills/${encodeURIComponent(destinationId)}/skills",
        ),
        (
            "/api/destination-skills/list-destinations/{skill_id}",
            "/destination-skills/list-destinations/${encodeURIComponent(skillId)}",
        ),
    ];
    for (rust_route, ts_url) in &expected_http_junctions {
        assert!(http.contains(rust_route), "HTTP output missing route `{}`", rust_route);
        assert!(ts.contains(ts_url), "TS HTTP transport missing URL `{}`", ts_url);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Custom GET mixing a path param with query params
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a module with a custom GET that mixes a required path param with an
/// optional numeric query param. Models `jobs::get_log(app, lines)`: http.rs
/// registers `/api/jobs/log/{app}` with `lines` in the query string, so the TS
/// clients must emit the path segment AND type `lines` from the Option's
/// inner type (`number | null`), matching what the IPC handler deserializes.
fn make_mixed_path_query_module() -> ApiModule {
    ApiModule {
        name: "jobs".to_string(),
        functions: vec![ApiFn {
            name: "get_log".to_string(),
            is_async: true,
            doc: "Tail the captured log for an app.".to_string(),
            params: vec![param("app", "&str"), param("lines", "Option<u64>")],
            return_type: "JobLog".to_string(),
            return_type_ast: ty_ast("JobLog"),
            ..Default::default()
        }],
        events: vec![],
        is_singleton: false,
        has_count: false,
    }
}

#[test]
fn test_custom_get_mixed_path_and_query_cross_transport() {
    // Pre-fix, the TS emitters dropped the `app` path segment whenever a
    // custom GET also had query params, fetching `/jobs/log?lines=N` against
    // a server route registered as `/api/jobs/log/{app}`.
    let tmp = tempfile::tempdir().unwrap();
    let http_out = tmp.path().join("http.rs");
    let transport_out = tmp.path().join("transport.ts");
    let client_out = tmp.path().join("client.ts");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type JobLog = { lines: string[] };\n").unwrap();

    let config = test_config(tmp.path().to_path_buf());
    let client_config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_mixed_path_query_module()];

    crate::servers::generators::http::generate(&http_out, &modules, &config);
    crate::clients::generators::transport::generate(&transport_out, &bindings, &modules, &client_config);
    crate::clients::generators::ts_client::generate(&client_out, &bindings, &modules, &client_config);

    let http = std::fs::read_to_string(&http_out).unwrap();
    let transport = std::fs::read_to_string(&transport_out).unwrap();
    let client = std::fs::read_to_string(&client_out).unwrap();

    // Server side: path param after the action segment, query param extracted
    // as the Option's inner type.
    assert!(http.contains("\"/api/jobs/log/{app}\""), "http.rs should register the path param after the action");
    assert!(
        http.contains("let lines = ontogen_query.op_arg::<u64>(\"lines\")?;"),
        "http.rs reads the opArg as the Option's numeric inner type:\n{http}"
    );
    assert!(http.contains("op_args: &[\"lines\"]"), "and accepts only that opArg:\n{http}");

    // Both TS emitters must keep the path segment ahead of the query string,
    // where the optional param is an `opArg`.
    for (name, content) in [("transport.ts", &transport), ("ts_client.ts", &client)] {
        assert!(
            content.contains("`/jobs/log/${encodeURIComponent(app)}${toQueryString({ opArg: { lines } })}`"),
            "{name} must fetch the path segment the server routes on, got neither in:\n{content}"
        );
        assert!(
            !content.contains("`/jobs/log${"),
            "regression: {name} dropped the `:app` path segment from the custom GET URL"
        );
    }

    // Query param typing must derive from the Option's inner type everywhere
    // (Transport interface, HTTP impl, IPC impl, HTTP-only client) - the IPC
    // handler deserializes `Option<u64>`, so `string | null` breaks at runtime.
    for (name, content) in [("transport.ts", &transport), ("ts_client.ts", &client)] {
        assert!(content.contains("lines: number | null"), "{name} should type the numeric query param as number");
        assert!(
            !content.contains("lines: string | null"),
            "regression: {name} typed the Option<u64> query param as string"
        );
    }
    assert!(!transport.contains("null | null"), "IPC transport should not double-append `| null` to Option params");

    // `0` is a valid value for a numeric query param - the query builder must
    // skip only null and undefined, not every falsy value.
    for (name, content) in [("transport.ts", &transport), ("ts_client.ts", &client)] {
        assert!(
            content.contains("if (value == null) return;"),
            "{name} should null-check the numeric query param so `0` still serializes"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// E2E pipeline - scan real API modules → generate all outputs
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

    let route_prefix = Some(RoutePrefix {
        segments: "projects/:project_id".to_string(),
        state_accessor: "store_for".to_string(),
        params: vec![PrefixParam {
            name: "project_id".to_string(),
            rust_type: "uuid::Uuid".to_string(),
            ts_type: "string".to_string(),
        }],
    });

    let server_config = Config {
        api_dir: api_dir.clone(),
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming: naming.clone(),
        generators: vec![
            ServerGenerator::HttpAxum { output: http_out.clone() },
            ServerGenerator::TauriIpc { output: ipc_out.clone() },
            ServerGenerator::Mcp { output: mcp_out.clone() },
        ],
        sse_route_overrides: HashMap::new(),
        route_prefix: route_prefix.clone(),
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        pagination: None,
        extra_surfaces: Vec::new(),
        resources: Default::default(),
        enums: Vec::new(),
        error_map: None,
    };

    let modules = crate::servers::generate_transport(&server_config).expect("generate_transport failed");

    // Also run the client-side generators so the rest of the assertions
    // (TS transport + admin registry outputs) still apply post-split.
    let client_config = ClientsInternalConfig {
        api_dir: api_dir.clone(),
        required_query_structs: Default::default(),
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming,
        generators: vec![
            ClientGenerator::HttpTauriIpcSplit { output: ts_out.clone(), bindings_path: bindings.clone() },
            ClientGenerator::AdminRegistry { output: admin_out.clone() },
        ],
        ts_formatter: crate::TsFormatter::None,
        sse_route_overrides: HashMap::new(),
        ts_skip_commands: vec![],
        route_prefix,
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        pagination: None,
        entities: Vec::new(),
        resources: Default::default(),
        schema_enums: Vec::new(),
        label_overrides: HashMap::new(),
        pool_extra_roots: Vec::new(),
        pool_exclude_paths: Vec::new(),
        extra_surfaces: Vec::new(),
    };
    crate::clients::generators::transport::generate(&ts_out, &bindings, &modules, &client_config);
    crate::clients::generators::admin::generate(
        &admin_out,
        &modules,
        &client_config,
        &client_config.entities,
        &client_config.schema_enums,
    );

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

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;

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
            // Full CRUD entity - should have all 5
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

// ═══════════════════════════════════════════════════════════════════════════════
// extract_server_metadata - ServersOutput IR population
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_extract_metadata_crud_routes() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf());
    let modules = vec![make_crud_module("node", true)];

    let output = crate::servers::extract_server_metadata(&modules, &config);

    let nodes: Vec<_> = output.http_routes.iter().filter(|r| r.module_name == "node").collect();
    assert_eq!(nodes.len(), 5, "Expected 5 CRUD routes, got {nodes:?}");

    let by_method = |m: &str| nodes.iter().find(|r| r.method == m).expect(m);
    assert_eq!(by_method("GET").path, "/api/nodes");
    assert_eq!(by_method("POST").path, "/api/nodes");
    assert_eq!(by_method("PATCH").path, "/api/nodes/{id}", "no route is PUT");
    assert_eq!(by_method("DELETE").path, "/api/nodes/{id}");
    let gets: Vec<_> = nodes.iter().filter(|r| r.method == "GET").collect();
    assert!(gets.iter().any(|r| r.path == "/api/nodes/{id}"), "Expected get_by_id route");
}

#[test]
fn test_extract_metadata_ipc_and_mcp_populated() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf());
    let modules = vec![make_crud_module("node", true)];

    let output = crate::servers::extract_server_metadata(&modules, &config);

    assert_eq!(output.ipc_commands.len(), 5);
    assert_eq!(output.mcp_tools.len(), 5);

    let create_cmd = output.ipc_commands.iter().find(|c| c.command_name == "node_create").expect("node_create");
    assert_eq!(create_cmd.params.len(), 1);
    assert_eq!(create_cmd.params[0].name, "input");
    assert_eq!(create_cmd.return_type, "Node");

    let update_tool = output.mcp_tools.iter().find(|t| t.tool_name == "node_update").expect("node_update");
    assert_eq!(update_tool.params.len(), 2);
    assert!(update_tool.description.contains("Update"));
}

#[test]
fn test_extract_metadata_scopes_store_modules_with_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config_with_prefix(tmp.path().to_path_buf());
    let modules = vec![make_crud_module("node", true)];

    let output = crate::servers::extract_server_metadata(&modules, &config);

    for r in &output.http_routes {
        if r.module_name == "node" {
            assert!(r.path.starts_with("/api/projects/{project_id}/nodes"), "Expected scoped path, got {}", r.path);
        }
    }
}

#[test]
fn test_extract_metadata_emits_event_routes() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf());
    let modules = vec![make_event_module()];

    let output = crate::servers::extract_server_metadata(&modules, &config);

    // Each event op is an IPC subscribe/unsubscribe pair; MCP skips it.
    assert_eq!(output.ipc_commands.len(), 4);
    assert!(output.mcp_tools.is_empty());

    let paths: Vec<_> = output.http_routes.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.contains(&"/api/events/graph-updated"), "got {paths:?}");
    assert!(paths.contains(&"/api/events/entity-changed"), "got {paths:?}");
}

#[test]
fn test_extract_metadata_custom_get_includes_path_params() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf());
    let modules = vec![make_custom_module()];

    let output = crate::servers::extract_server_metadata(&modules, &config);

    let snapshot = output.http_routes.iter().find(|r| r.handler_name.contains("get_graph_snapshot")).expect("snapshot");
    assert_eq!(snapshot.method, "GET");
    // get_graph_snapshot has only an Option<&str> param → no path params
    assert!(!snapshot.path.contains("{parent_id}"), "Optional params should not be path-params: {}", snapshot.path);

    let detail = output.http_routes.iter().find(|r| r.handler_name.contains("get_node_detail")).expect("detail");
    assert_eq!(detail.method, "GET");
    assert!(detail.path.contains("{node_id}"), "Required param should be path-param: {}", detail.path);
}

/// Regression test for OF-008 and OF-010.
///
/// Parses a synthetic api module whose return types include `Option<T>`,
/// `Option<String>`, `HashMap<K, V>`, `Box<T>`, and a nested
/// `Option<HashMap<String, Vec<T>>>` — the exact patterns that previously
/// caused `collect_type_import` to emit invalid Rust into the `use
/// crate::schema::{ ... };` block. After the fix, the generated import block
/// must contain only the leaf identifiers and never wrappers like
/// `Option<...>` or `HashMap<...>`.
#[test]
fn test_collect_type_import_no_leak_of_wrappers() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "backup.rs",
        r#"
use crate::store::Store;
use std::collections::HashMap;

pub async fn backup_data(store: &Store) -> Result<Option<String>, anyhow::Error> { todo!() }
pub async fn restore_pick(store: &Store) -> Result<Option<RestoreCandidate>, anyhow::Error> { todo!() }
pub async fn get_prefs(store: &Store) -> Result<HashMap<String, NotificationPrefs>, anyhow::Error> { todo!() }
pub async fn get_keyed(store: &Store) -> Result<HashMap<MyKey, MyValue>, anyhow::Error> { todo!() }
pub async fn boxed(store: &Store) -> Result<Box<Schema>, anyhow::Error> { todo!() }
pub async fn deeply_nested(store: &Store) -> Result<Option<HashMap<String, Vec<NestedItem>>>, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1);

    let output_path = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::ipc::generate(&output_path, &modules, &config);

    let content = std::fs::read_to_string(&output_path).unwrap();

    // The `use crate::schema::{ ... };` block must contain the leaf types we
    // expect — and must NOT contain any wrapper-leak literals.
    for must_contain in ["RestoreCandidate", "NotificationPrefs", "MyKey", "MyValue", "Schema", "NestedItem"].iter() {
        assert!(content.contains(must_contain), "expected `{must_contain}` in generated output");
    }

    // No wrappers must ever appear inside a `use crate::schema::{ ... }`.
    for forbidden in ["Option<", "HashMap<", "Box<", "Vec<", "Result<"].iter() {
        let schema_block_starts = content.find("use crate::schema::{").expect("schema use block");
        let schema_block_ends =
            content[schema_block_starts..].find("};").map(|i| schema_block_starts + i).expect("schema block end");
        let schema_block = &content[schema_block_starts..schema_block_ends];
        assert!(!schema_block.contains(forbidden), "wrapper `{forbidden}` leaked into use block: {schema_block}");
    }
}

/// Regression test for OF-011.
///
/// Drives the generic IPC handler through every param-shape that previously
/// triggered wrong forwarding. The critical regression is `rating: Option<u8>`
/// — the old generator emitted `rating.as_deref()`, which fails to compile
/// because `u8: !Deref`. Several other shapes were also miscategorised by the
/// "type-name-contains-Input" substring heuristic.
///
/// The test parses a synthetic api module, generates IPC, and asserts on the
/// rendered forwarding expressions inside the service call. The function name
/// for each case is unique so we can grep precisely.
#[test]
fn test_ipc_handler_arg_forwarding_matrix() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "shapes.rs",
        r#"
use crate::store::Store;

pub async fn case_ref_str(store: &Store, text: &str) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_owned_string(store: &Store, id: String) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_owned_bool(store: &Store, enabled: bool) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_owned_u8(store: &Store, count: u8) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_ref_struct(store: &Store, profile_id: SelectedProfileId) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_input_struct(store: &Store, input: ProfileInput) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_owned_qualified(store: &Store, prefs: crate::schema::NotificationPrefs) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_ref_qualified(store: &Store, prefs: &crate::schema::NotificationPrefs) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_option_u8(store: &Store, rating: Option<u8>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_option_owned_string(store: &Store, name: Option<String>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_option_ref_str(store: &Store, name: Option<&str>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_option_ref_struct(store: &Store, profile: Option<&MyStruct>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_option_ref_slice(store: &Store, bytes: Option<&[u8]>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_option_ref_path(store: &Store, p: Option<&Path>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_option_vec(store: &Store, tags: Option<Vec<String>>) -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1);

    let output_path = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::ipc::generate(&output_path, &modules, &config);

    let content = std::fs::read_to_string(&output_path).unwrap();

    // Each case asserts the precise forwarding call. Function names in the
    // generated output get the entity prefix (`shape_`), so we match on the
    // bare service-fn portion: `shapes::case_..._args)`.
    let cases: &[(&str, &str)] = &[
        ("case_ref_str", "shapes::case_ref_str(&ontogen_store, &text)"),
        ("case_owned_string", "shapes::case_owned_string(&ontogen_store, id)"),
        ("case_owned_bool", "shapes::case_owned_bool(&ontogen_store, enabled)"),
        ("case_owned_u8", "shapes::case_owned_u8(&ontogen_store, count)"),
        ("case_ref_struct", "shapes::case_ref_struct(&ontogen_store, profile_id)"),
        ("case_input_struct", "shapes::case_input_struct(&ontogen_store, input)"),
        ("case_owned_qualified", "shapes::case_owned_qualified(&ontogen_store, prefs)"),
        ("case_ref_qualified", "shapes::case_ref_qualified(&ontogen_store, &prefs)"),
        // The OF-011 regression: Option<u8> must pass `rating`, NOT
        // `rating.as_deref()`.
        ("case_option_u8", "shapes::case_option_u8(&ontogen_store, rating)"),
        ("case_option_owned_string", "shapes::case_option_owned_string(&ontogen_store, name)"),
        ("case_option_ref_str", "shapes::case_option_ref_str(&ontogen_store, name.as_deref())"),
        ("case_option_ref_struct", "shapes::case_option_ref_struct(&ontogen_store, profile.as_ref())"),
        ("case_option_ref_slice", "shapes::case_option_ref_slice(&ontogen_store, bytes.as_deref())"),
        ("case_option_ref_path", "shapes::case_option_ref_path(&ontogen_store, p.as_deref())"),
        ("case_option_vec", "shapes::case_option_vec(&ontogen_store, tags)"),
    ];

    for (fn_name, expected_call) in cases {
        assert!(
            content.contains(expected_call),
            "OF-011 forwarding regression for `{fn_name}`: expected `{expected_call}` in generated IPC, got:\n{content}"
        );
    }

    // Negative: the old broken `rating.as_deref()` must not appear anywhere.
    assert!(
        !content.contains("rating.as_deref()"),
        "OF-011 regression: `rating.as_deref()` leaked back into IPC output"
    );
}

/// Regression test for OF-013.
///
/// The OF-011 fix made forwarding-side `.as_deref()` AST-aware (so
/// `Option<&[u8]>` correctly emits `payload.as_deref()`), but the
/// declaration side (`param_to_owned_type`) stayed string-based and
/// produced `Option<[u8]>` — an unsized type that won't compile in a handler
/// param list. OF-013 fixes the declaration side so the two stay in lockstep.
///
/// This test pins the symmetry: for each unsized-DST-under-Option shape, the
/// generated IPC handler must declare the sized owned companion AND forward
/// via `.as_deref()`. Plain `&[u8]` / `&Path` shapes are also checked to
/// ensure the rule applies outside `Option<...>` too.
#[test]
fn test_of013_unsized_dst_owned_form_in_ipc() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "dst_shapes.rs",
        r#"
use crate::store::Store;
use std::path::Path;
use std::ffi::{CStr, OsStr};

pub async fn case_opt_bytes(store: &Store, payload: Option<&[u8]>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_opt_path(store: &Store, p: Option<&Path>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_opt_cstr(store: &Store, s: Option<&CStr>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_opt_osstr(store: &Store, s: Option<&OsStr>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_opt_str(store: &Store, s: Option<&str>) -> Result<(), anyhow::Error> { todo!() }
pub async fn case_ref_bytes(store: &Store, payload: &[u8]) -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1);

    let output_path = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::ipc::generate(&output_path, &modules, &config);

    let content = std::fs::read_to_string(&output_path).unwrap();

    // Each row: (param-declaration to find, forwarding call to find). The
    // declaration uses `param_to_owned_type`; the forwarding uses
    // `forward_arg_expr`. Both must agree.
    let cases: &[(&str, &str)] = &[
        ("payload: Option<Vec<u8>>", "dst_shapes::case_opt_bytes(&ontogen_store, payload.as_deref())"),
        ("p: Option<PathBuf>", "dst_shapes::case_opt_path(&ontogen_store, p.as_deref())"),
        ("s: Option<CString>", "dst_shapes::case_opt_cstr(&ontogen_store, s.as_deref())"),
        ("s: Option<OsString>", "dst_shapes::case_opt_osstr(&ontogen_store, s.as_deref())"),
        ("s: Option<String>", "dst_shapes::case_opt_str(&ontogen_store, s.as_deref())"),
        ("payload: Vec<u8>", "dst_shapes::case_ref_bytes(&ontogen_store, &payload)"),
    ];

    for (decl, forward_call) in cases {
        assert!(content.contains(decl), "OF-013: expected declared param `{decl}` in generated IPC, got:\n{content}");
        assert!(
            content.contains(forward_call),
            "OF-013/OF-011 symmetry: expected forwarding `{forward_call}` in generated IPC, got:\n{content}"
        );
    }

    // Negative: the pre-fix unsized declarations must not appear.
    for forbidden in &["Option<[u8]>", "Option<Path>", "Option<CStr>", "Option<OsStr>", ": [u8]", ": str"] {
        assert!(
            !content.contains(forbidden),
            "OF-013 regression: unsized declaration `{forbidden}` leaked into IPC output:\n{content}"
        );
    }
}

/// Regression test for OF-017.
///
/// Pre-fix, IPC / HTTP / MCP generators wrapped `collect_type_import` on each
/// param in a substring filter that only invoked the walker when the rendered
/// param type name contained `Input` or `Query`. Wrapper structs whose names
/// didn't match that pattern (`ExportRequest`, `ExportFilterRequest`, …) were
/// silently dropped from the `use crate::schema::{...}` block, producing
/// uncompileable generated code.
///
/// Post-fix the gate is gone — the AST walker runs on every param. The
/// walker's existing rules (skip primitives, skip qualified paths, recurse
/// into containers) handle the no-op cases on their own.
#[test]
fn test_of017_param_imports_drop_substring_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "export.rs",
        r#"
use crate::store::Store;

pub async fn run(store: &Store, request: &ExportRequest) -> Result<ExportSummary, anyhow::Error> { todo!() }
pub async fn filtered_sessions(
    store: &Store,
    filter: &ExportFilterRequest,
) -> Result<Vec<Session>, anyhow::Error> { todo!() }
pub async fn rename(store: &Store, id: String, payload: &FilenameTemplateRequest) -> Result<(), anyhow::Error> { todo!() }
pub async fn already_matched(store: &Store, opts: &ExportOptionsInput) -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1);

    let config = test_config(tmp.path().to_path_buf());

    // Each generator must emit `use crate::schema::{ ... }` containing every
    // custom param/return type referenced above — including the ones that
    // don't match the legacy `Input`/`Query` substring. Parse the use-block
    // body so the assertion doesn't depend on rustfmt's line-wrapping
    // (rustfmt collapses short blocks onto a single line).
    let expected_in_imports = [
        "ExportRequest",           // bare custom struct, param position, no Input/Query in name
        "ExportFilterRequest",     // same shape, different name
        "FilenameTemplateRequest", // same shape, alongside a primitive param
        "ExportOptionsInput",      // legacy gate would have matched this; must still work
        "ExportSummary",           // return-position custom struct
        "Session",                 // return-position via Vec<Session>
    ];
    let forbidden_in_imports = ["String", "str", "Store"]; // primitive / state — never imported

    fn schema_imports(content: &str) -> Vec<String> {
        let Some(start) = content.find("use crate::schema::{") else { return Vec::new() };
        let body_start = start + "use crate::schema::{".len();
        let Some(rel_end) = content[body_start..].find('}') else { return Vec::new() };
        content[body_start..body_start + rel_end]
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn assert_imports(label: &str, content: &str, expected: &[&str], forbidden: &[&str]) {
        let imports = schema_imports(content);
        for ty in expected {
            assert!(
                imports.iter().any(|i| i == ty),
                "OF-017 {label}: expected `{ty}` in `use crate::schema::{{ ... }}` block. Parsed imports = {:?}\nFull output:\n{content}",
                imports
            );
        }
        for ty in forbidden {
            assert!(
                !imports.iter().any(|i| i == ty),
                "OF-017 {label}: `{ty}` must not appear in `use crate::schema::{{ ... }}` block. Parsed imports = {:?}\nFull output:\n{content}",
                imports
            );
        }
    }

    // IPC
    let ipc_out = tmp.path().join("ipc_generated.rs");
    crate::servers::generators::ipc::generate(&ipc_out, &modules, &config);
    let ipc = std::fs::read_to_string(&ipc_out).unwrap();
    assert_imports("IPC", &ipc, &expected_in_imports, &forbidden_in_imports);

    // HTTP: a handler names the types its arguments are read as, and no
    // result type (`meta.result` is serialized from the fn's own value), so
    // it imports exactly the param types.
    let http_out = tmp.path().join("http_generated.rs");
    crate::servers::generators::http::generate(&http_out, &modules, &config);
    let http = std::fs::read_to_string(&http_out).unwrap();
    let (params, results) = expected_in_imports.split_at(4);
    assert_imports("HTTP", &http, params, &[&forbidden_in_imports[..], results].concat());

    // MCP — covers both the param-side gate drop AND the new return-type walk
    // (mcp.rs previously skipped `f.return_type_ast` entirely).
    let mcp_out = tmp.path().join("mcp_generated.rs");
    crate::servers::generators::mcp::generate(&mcp_out, &modules, &config);
    let mcp = std::fs::read_to_string(&mcp_out).unwrap();
    assert_imports("MCP (covers return-type walk regression)", &mcp, &expected_in_imports, &forbidden_in_imports);
}

/// End-to-end regression test for OF-016.
///
/// A `get_*` function whose first user-facing param is a body-carrying
/// custom struct must classify as `CustomPost` and emit a `POST` HTTP
/// route with `Json(...)` body extraction -- not a `GET` route with
/// `Path<String>` extraction. Sibling `get_*` functions with id-like
/// primitive params keep their original `GET` + `Path<...>` shape.
///
/// This is the integration test that pins the user-visible behavior:
/// the classifier change plus the HTTP generator's existing
/// body/path/query partition end up producing valid handler code on
/// both sides of the heuristic.
#[test]
fn test_of016_get_with_body_param_routes_as_post() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "export.rs",
        r#"
use crate::store::Store;

pub async fn get_session(store: &Store, id: &str) -> Result<Session, anyhow::Error> { todo!() }
pub async fn get_filtered_sessions(
    store: &Store,
    filter: &ExportFilterRequest,
) -> Result<Vec<Session>, anyhow::Error> { todo!() }
pub async fn get_summary(store: &Store, request: &ExportRequest) -> Result<ExportSummary, anyhow::Error> { todo!() }
pub async fn get_recent(store: &Store, since: Option<String>) -> Result<Vec<Session>, anyhow::Error> { todo!() }
pub async fn get_count(store: &Store) -> Result<i64, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1);

    let output_path = tmp.path().join("http_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::http::generate(&output_path, &modules, &config);

    let content = std::fs::read_to_string(&output_path).unwrap();

    // Body-carrying get_* functions must route as POST with body extraction:
    // the struct is a `meta.args` member, not a path segment.
    for (route_post, body_extraction) in &[
        (
            "post(export_get_filtered_sessions)",
            "let filter = ontogen_jsonapi::request::op_arg::<ExportFilterRequest>(&ontogen_args, \"filter\", true)?;",
        ),
        (
            "post(export_get_summary)",
            "let request = ontogen_jsonapi::request::op_arg::<ExportRequest>(&ontogen_args, \"request\", true)?;",
        ),
    ] {
        assert!(
            content.contains(route_post),
            "OF-016: expected POST route fragment `{route_post}` in HTTP output, got:\n{content}"
        );
        assert!(
            content.contains(body_extraction),
            "OF-016: expected Json body extraction `{body_extraction}` in HTTP output, got:\n{content}"
        );
    }

    // The handler names the original custom struct type by its real name --
    // this is what the import collector (OF-017) had to start emitting for
    // these to compile.
    for call in
        &["export::get_filtered_sessions(&ontogen_store, &filter)", "export::get_summary(&ontogen_store, &request)"]
    {
        assert!(content.contains(call), "OF-016: expected the call `{call}`, got:\n{content}");
    }

    // Negative: the body-carrying handlers must NOT extract the struct as
    // a path segment. The classifier change is precisely what prevents
    // this.
    for forbidden in &[
        "OntogenPath(filter): OntogenPath<String>",
        "OntogenPath(request): OntogenPath<String>",
        "/{filter}",
        "/{request}",
        "get(export_get_filtered_sessions)",
        "get(export_get_summary)",
    ] {
        assert!(
            !content.contains(forbidden),
            "OF-016 regression: forbidden fragment `{forbidden}` leaked into HTTP output:\n{content}"
        );
    }

    // Positive: id-like primitive first param stays GET with Path<String>.
    assert!(
        content.contains("get(export_get_session)"),
        "OF-016: `get_session(id: &str)` must stay GET, got:\n{content}"
    );
    assert!(
        content.contains("OntogenPath(id): OntogenPath<String>"),
        "OF-016: `get_session` should still extract `id` as OntogenPath<String>, got:\n{content}"
    );

    // Positive: Option<String> first param stays GET with query
    // extraction, as an `opArg`.
    assert!(
        content.contains("get(export_get_recent)"),
        "OF-016: `get_recent(since: Option<String>)` should stay GET, got:\n{content}"
    );
    assert!(
        content.contains("let since = ontogen_query.op_arg::<String>(\"since\")?;"),
        "OF-016: `get_recent` should read `opArg[since]`, got:\n{content}"
    );

    // Positive: zero-param get_* stays GET (no body to carry).
    assert!(
        content.contains("get(export_get_count)"),
        "OF-016: zero-param `get_count` should stay GET, got:\n{content}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// parse.rs + generators - #[ontogen::stateless] (OF-007)
// ═══════════════════════════════════════════════════════════════════════════════

/// `#[ontogen::stateless]` on a fn with no params is accepted: the parser
/// skips the first-param state/store check and the NoParams guard, produces
/// an `ApiFn` with `is_stateless = true`, and emits no `SkipRecord`.
#[test]
fn test_of007_stateless_zero_params_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "clock.rs",
        r#"
#[ontogen::stateless]
pub fn now() -> Result<i64, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(scan.skips.is_empty(), "stateless fn must not produce SkipRecord: {:?}", scan.skips);
    let module = scan.modules.iter().find(|m| m.name == "clock").expect("clock module must be parsed");
    let f = module.functions.iter().find(|f| f.name == "now").expect("now fn must be present");
    assert!(f.is_stateless, "now() must be marked stateless");
    assert!(!f.first_param_is_store, "stateless fns must not be store-scoped");
    assert!(f.params.is_empty(), "zero-param stateless fn should have empty params");
}

/// `#[ontogen::stateless]` on a fn whose first parameter is unrelated to the
/// configured state/store type is accepted. The parameter is included in
/// `params` rather than being skipped as a state slot.
#[test]
fn test_of007_stateless_non_state_first_param_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "clipboard.rs",
        r#"
#[ontogen::stateless]
pub fn copy(text: &str) -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(scan.skips.is_empty(), "stateless fn must not produce SkipRecord: {:?}", scan.skips);
    let module = scan.modules.iter().find(|m| m.name == "clipboard").unwrap();
    let f = module.functions.iter().find(|f| f.name == "copy").unwrap();
    assert!(f.is_stateless);
    assert_eq!(f.params.len(), 1, "stateless fns keep every input as a param (no state to skip)");
    assert_eq!(f.params[0].name, "text");
    assert_eq!(f.params[0].ty, "&str");
}

/// The bare `#[stateless]` path form (after `use ontogen::stateless`) is also
/// accepted — the parser matches on the final path segment.
#[test]
fn test_of007_bare_stateless_path_form_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "util.rs",
        r#"
use ontogen::stateless;

#[stateless]
pub fn checksum(payload: &[u8]) -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(scan.skips.is_empty(), "bare #[stateless] must be accepted: {:?}", scan.skips);
    let module = scan.modules.iter().find(|m| m.name == "util").unwrap();
    assert!(module.functions.iter().any(|f| f.name == "checksum" && f.is_stateless));
}

/// `&self` is still rejected even with `#[ontogen::stateless]` — method
/// signatures don't fit free-function API modules regardless of state.
#[test]
fn test_of007_stateless_self_receiver_still_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "broken.rs",
        r#"
struct S;
impl S {
    #[ontogen::stateless]
    pub fn foo(&self, _x: u32) -> Result<(), anyhow::Error> { todo!() }
}
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    // The `&self` fn is inside an impl block, so scan_api_dir won't see it as
    // a free `pub fn` — but the more direct guarantee is that no module is
    // produced for `broken.rs` and no stateless fn lands. The negative shape
    // matters more than the SkipRecord here.
    assert!(
        scan.modules.iter().find(|m| m.name == "broken").is_none_or(|m| m.functions.is_empty()),
        "self-receiver methods must not be picked up by the parser"
    );
}

/// An unmarked stateless-shaped fn (no `#[stateless]`, no state/store first
/// param) still produces a `SkipRecord` with the hint mentioning the
/// `#[ontogen::stateless]` attribute.
#[test]
fn test_of007_unmarked_stateless_fn_emits_skip_with_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "lonely.rs",
        r#"
pub fn just_a_helper(x: u32) -> Result<(), anyhow::Error> { todo!() }
pub fn no_params() -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert_eq!(scan.skips.len(), 2, "two unmarked fns should each emit one SkipRecord");
    let texts: Vec<String> = scan.skips.iter().map(ToString::to_string).collect();
    assert!(
        texts.iter().all(|s| s.contains("#[ontogen::stateless]")),
        "OF-007 hint must appear in every state-shape skip warning, got:\n{texts:#?}"
    );
}

/// End-to-end IPC: a stateless fn renders without `state: State<...>` and
/// without a positional state forward. Path/owned-type behaviour is
/// unchanged.
#[test]
fn test_of007_stateless_ipc_handler_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "util.rs",
        r#"
#[ontogen::stateless]
pub fn copy(text: &str) -> Result<(), anyhow::Error> { todo!() }

#[ontogen::stateless]
pub fn now() -> Result<i64, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let output_path = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::ipc::generate(&output_path, &modules, &config);

    let content = std::fs::read_to_string(&output_path).unwrap();

    // The `copy` handler must declare `text: String`, take no `State<...>`
    // extractor, and forward without a leading state argument.
    assert!(content.contains("text: String"), "owned form for &str must still be String:\n{content}");
    assert!(
        content.contains("util::copy(text.as_deref())")
            || content.contains("util::copy(&text)")
            || content.contains("util::copy(text)"),
        "stateless forward must call util::copy(text) directly with no state prefix:\n{content}"
    );
    assert!(!content.contains("util::copy(&state, text"), "stateless fn must not forward `&state`:\n{content}");

    // The `now` handler is zero-param, so the body should be `util::now()`
    // with no state extractor or argument.
    assert!(content.contains("util::now()"), "zero-arg stateless call expected:\n{content}");
    assert!(
        !content.contains("fn util_now")
            || !content[content.find("fn util_now").unwrap()..].lines().take(8).any(|l| l.contains("state: State<")),
        "stateless fn handler must not declare `state: State<...>`:\n{content}"
    );
}

/// End-to-end HTTP: a stateless fn renders without `State(state): State<...>`
/// and forwards no state. Route nesting under `/api/<module>` is preserved.
#[test]
fn test_of007_stateless_http_handler_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "util.rs",
        r#"
#[ontogen::stateless]
pub fn ping() -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let output_path = tmp.path().join("http_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::http::generate(&output_path, &modules, &config);

    let content = std::fs::read_to_string(&output_path).unwrap();

    // Locate the `ping` handler block and assert its signature has no State<>.
    let ping_idx = content.find("fn util_ping").expect("util_ping handler must be emitted");
    let handler_block: String = content[ping_idx..].chars().take(600).collect();
    assert!(
        !handler_block.contains("OntogenState(state)"),
        "stateless HTTP handler must not declare OntogenState(state):\n{handler_block}"
    );
    assert!(
        handler_block.contains("util::ping()"),
        "stateless forward must call util::ping() with no leading state arg:\n{handler_block}"
    );
    // The module-nested route /api/utils/ping (nested by default) must appear.
    assert!(
        content.contains("/api/utils/ping") || content.contains("/api/util/ping"),
        "stateless route should be nested under the module url:\n{content}"
    );
}

/// End-to-end MCP: a stateless fn produces a tool whose handler body
/// invokes the service fn directly with no state/store argument.
#[test]
fn test_of007_stateless_mcp_tool_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "util.rs",
        r#"
#[ontogen::stateless]
pub fn echo(text: &str) -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let output_path = tmp.path().join("mcp_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::mcp::generate(&output_path, &modules, &config);

    let content = std::fs::read_to_string(&output_path).unwrap();

    // The tool's handler must not construct a Store or call validation,
    // and must invoke util::echo with no state argument.
    assert!(
        content.contains("util::echo(text)"),
        "stateless MCP tool must call util::echo(text) — no state prefix:\n{content}"
    );
    assert!(
        !content.contains("util::echo(ontogen_state, text") && !content.contains("util::echo(&ontogen_store, text"),
        "stateless MCP tool must not forward state or store:\n{content}"
    );
}

/// A custom op's MCP tool reads each argument as its fn declares it, bound
/// under its own name: the handler's bindings are `ontogen_`-prefixed, so
/// arguments named `state` and `store` reach the op, and an `Option<bool>`
/// is read as a `bool`. A body struct beside other arguments is one of them,
/// named as its parameter (as on IPC), so its fields cannot collide with
/// theirs; a body alone is the arguments, less the scope's.
#[test]
fn an_mcp_custom_tool_reads_each_argument_as_its_type_under_its_name() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "task.rs",
        r#"
pub async fn set_state(ctx: &Store, id: &str, state: String, store: Option<String>) -> Result<Task, AppError> { todo!() }
pub async fn get_summary(store: &Store, status: &str, verbose: Option<bool>, limit: Option<u32>) -> Result<TaskSummary, AppError> { todo!() }
pub async fn capture(store: &Store, input: CreateTaskInput, status: Option<String>) -> Result<Task, AppError> { todo!() }
pub async fn file(store: &Store, input: CreateTaskInput) -> Result<Task, AppError> { todo!() }
"#,
    );
    let config = test_config_with_prefix(api_dir.clone());
    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let out = tmp.path().join("mcp.rs");
    crate::servers::generators::mcp::generate(&out, &modules, &config);
    let code = std::fs::read_to_string(&out).unwrap();
    syn::parse_file(&code).unwrap_or_else(|e| panic!("does not parse: {e}\n{code}"));
    let flat = compact(&code);

    for line in [
        "let state: String = ::serde_json::from_value(ontogen_args.get(\"state\").cloned()\
         .ok_or(\"Missing required parameter: state\")?).map_err(|e| format!(\"Invalid parameter state: {e}\"))?;",
        "let store: Option<String> = ontogen_args.get(\"store\")",
        "task::set_state(&ontogen_store, id, state, store)",
        "let verbose: Option<bool> = ontogen_args.get(\"verbose\").filter(|v| !v.is_null()).cloned()\
         .map(::serde_json::from_value::<bool>)",
        "task::get_summary(&ontogen_store, status, verbose, limit)",
        "let input: CreateTaskInput = ::serde_json::from_value(ontogen_args.get(\"input\").cloned()\
         .ok_or(\"Missing required parameter: input\")?).map_err(|e| format!(\"Invalid parameter input: {e}\"))?;",
        "task::capture(&ontogen_store, input, status)",
        "#[derive(::schemars::JsonSchema)] pub struct OntogenTaskCaptureInput { pub input: CreateTaskInput, pub status: \
         Option<String>, }",
        // A body alone is the arguments, less the scope's.
        "let ontogen_input: CreateTaskInput = ::serde_json::from_value(args_without(ontogen_args, &[\"project_id\"]))\
         .map_err(|e| format!(\"Invalid input: {e}\"))?;",
        "task::file(&ontogen_store, ontogen_input)",
        "schema_fn: || with_project_id_schema(schema_for::<OntogenTaskCaptureInput>()),",
        "#[derive(::schemars::JsonSchema)] pub struct OntogenTaskSetStateInput { pub id: String, pub state: String, pub \
         store: Option<String>, }",
    ] {
        assert!(flat.contains(&compact(line)), "{line}:\n{code}");
    }
}

/// Every MCP tool refuses an argument its input schema does not name, as a
/// list does and as HTTP refuses an unknown member: custom `GET` and `POST`
/// (with plain arguments, a body alone, a body beside another argument, no
/// argument, stateless), CRUD, junction ops (paged and not) and lists, unscoped and
/// under a route prefix. The refusal reads the very schema the tool
/// advertises, scope and page included.
#[test]
fn every_mcp_tool_refuses_an_argument_its_schema_does_not_name() {
    let task = "\
pub async fn list(store: &Store, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Task>, AppError> { todo!() }
pub async fn count(store: &Store) -> Result<u64, AppError> { todo!() }
pub async fn get_by_id(store: &Store, id: &str) -> Result<Task, AppError> { todo!() }
pub async fn create(store: &Store, input: CreateTaskInput) -> Result<Task, AppError> { todo!() }
pub async fn update(store: &Store, id: &str, input: UpdateTaskInput) -> Result<Task, AppError> { todo!() }
pub async fn delete(store: &Store, id: &str) -> Result<(), AppError> { todo!() }
pub async fn get_summary(store: &Store, status: &str, verbose: Option<bool>) -> Result<TaskSummary, AppError> { todo!() }
pub async fn capture(store: &Store, input: CreateTaskInput, status: Option<String>) -> Result<Task, AppError> { todo!() }
pub async fn file(store: &Store, input: CreateTaskInput) -> Result<Task, AppError> { todo!() }
pub async fn purge_done(store: &Store) -> Result<u32, AppError> { todo!() }
pub async fn list_tags(store: &Store, id: &str) -> Result<Vec<Tag>, AppError> { todo!() }
pub async fn add_tag(store: &Store, id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }
pub async fn remove_tag(store: &Store, id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }
";
    let gadget = "\
pub async fn list(store: &Store, query: GadgetQuery, owner: &str, limit: Option<u64>, offset: Option<u64>) \
  -> Result<Vec<Gadget>, AppError> { todo!() }
pub async fn count(store: &Store, query: GadgetQuery, owner: &str) -> Result<u64, AppError> { todo!() }
";
    let util = "\
#[ontogen::stateless]
pub fn echo(text: &str) -> Result<String, anyhow::Error> { todo!() }
";
    let tools = [
        "task_list",
        "task_get_by_id",
        "task_create",
        "task_update",
        "task_delete",
        "task_get_summary",
        "task_capture",
        "task_file",
        "task_purge_done",
        "task_list_tags",
        "task_add_tag",
        "task_remove_tag",
        "gadget_list",
        "util_echo",
    ];
    for (scoped, paginated) in [(false, false), (false, true), (true, false), (true, true)] {
        let tmp = tempfile::tempdir().unwrap();
        let api_dir = tmp.path().join("api");
        for (file, source) in [("task.rs", task), ("gadget.rs", gadget), ("util.rs", util)] {
            write_synthetic_api(&api_dir, file, source);
        }
        let mut config = if scoped { test_config_with_prefix(api_dir) } else { test_config(api_dir) };
        if paginated {
            config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });
        }
        let output = tmp.path().join("mcp.rs");
        config.generators = vec![ServerGenerator::Mcp { output: output.clone() }];
        crate::servers::generate_transport(&config).expect("generate_transport failed");
        let code = std::fs::read_to_string(output).unwrap();
        syn::parse_file(&code).unwrap_or_else(|e| panic!("does not parse: {e}\n{code}"));
        let case = format!("scoped: {scoped}, paginated: {paginated}");

        let flat = compact(&code);
        let defs: Vec<&str> = flat.split("McpToolDef{name:\"").skip(1).collect();
        let mut named: Vec<&str> = defs.iter().map(|def| &def[..def.find('"').unwrap()]).collect();
        named.sort_unstable();
        let mut expected = tools.to_vec();
        if !paginated {
            expected.extend(["task_count", "gadget_count"]);
        }
        expected.sort_unstable();
        assert_eq!(named, expected, "{case}");
        for def in defs {
            let name = &def[..def.find('"').unwrap()];
            let schema_fn =
                &def[def.find(",schema_fn:").unwrap() + ",schema_fn:".len()..def.find(",handler:").unwrap()];
            let schema = schema_fn.strip_prefix("||").map_or_else(|| format!("{schema_fn}()"), str::to_string);
            let opening = format!(
                ",handler:|ontogen_state,ontogen_args|{{Box::pin(asyncmove{{refuse_unknown_args(ontogen_args,{schema})?;"
            );
            assert!(def.contains(&opening), "{case}: {name} first refuses what {schema} does not name:\n{code}");
        }
        if scoped {
            assert!(
                flat.contains(
                    "refuse_unknown_args(ontogen_args,with_project_id_schema(schema_for::<GetByIdInput>()))?;"
                ),
                "{case}: the scope is an argument the tool names:\n{code}"
            );
        }
        if paginated {
            assert!(
                flat.contains("refuse_unknown_args(ontogen_args,with_pagination_schema("),
                "{case}: the page is an argument the tool names:\n{code}"
            );
        }
    }
}

/// `required_str` tells a missing string argument from one of another
/// type, as every other argument read does, and words the second as a
/// `String` read does: `expected a string`.
#[test]
fn mcp_required_str_reports_a_wrong_type_as_invalid() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "workout.rs", &paged_crud_module_source("workout", "Store"));
    let config = test_config(api_dir.clone());
    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let out = tmp.path().join("mcp.rs");
    crate::servers::generators::mcp::generate(&out, &modules, &config);
    let code = std::fs::read_to_string(&out).unwrap();
    assert!(
        compact(&code).contains(&compact(
            "let value = args.get(key).ok_or_else(|| format!(\"Missing required parameter: {key}\"))?; \
             value.as_str().ok_or_else(|| { \
                 let e = <String as ::serde::Deserialize>::deserialize(value) \
                     .expect_err(\"a value that is no string is no String\"); \
                 format!(\"Invalid parameter {key}: {e}\") \
             })"
        )),
        "{code}"
    );
    assert!(!code.contains("<&str>::deserialize"), "a `&str` read words its error `expected a borrowed string`");
    // The wording is serde's own for a `String`, which the read reports.
    let e = <String as serde::Deserialize>::deserialize(&serde_json::json!(5)).unwrap_err();
    assert_eq!(e.to_string(), "invalid type: integer `5`, expected a string");
}

// ═══════════════════════════════════════════════════════════════════════════════
// IPC + MCP - a consumer's names never collide with the generated file's
// ═══════════════════════════════════════════════════════════════════════════════

/// API modules named after the items an IPC or MCP file once imported or
/// defined (`Value`, `State`, `Arc`, `Future`, `Log`) and after a Rust
/// keyword (`Match`, module `r#match`), with args named after keywords
/// (`r#type`, `r#in`), a JS reserved word (`class`) and a doc that quotes.
/// Paginated, with a parameterless and a parameterized event op, so every
/// shape of command and tool is emitted.
pub(crate) fn name_safety_config(root: &std::path::Path) -> Config {
    let api_dir = root.join("api");
    for entity in ["value", "state"] {
        write_synthetic_api(&api_dir, &format!("{entity}.rs"), &paged_crud_module_source(entity, "Store"));
    }
    write_synthetic_api(
        &api_dir,
        "match.rs",
        &(paged_crud_module_source("match", "Store")
            + "pub fn match_changes(state: &AppState) -> broadcast::Receiver<Match> { todo!() }\n"),
    );
    write_synthetic_api(
        &api_dir,
        "future.rs",
        "pub async fn list(store: &Store, r#type: Option<&str>, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Future>, anyhow::Error> { todo!() }
pub async fn count(store: &Store, r#type: Option<&str>) -> Result<u64, anyhow::Error> { todo!() }
",
    );
    write_synthetic_api(
        &api_dir,
        "arc.rs",
        "pub async fn get(store: &Store, r#in: &str) -> Result<Arc, anyhow::Error> { todo!() }
pub fn arc_changes(state: &AppState, r#in: Option<String>) -> broadcast::Receiver<Arc> { todo!() }
",
    );
    write_synthetic_api(
        &api_dir,
        "log.rs",
        "pub fn log_changes(state: &AppState) -> broadcast::Receiver<Log> { todo!() }\n",
    );
    write_synthetic_api(
        &api_dir,
        "lookup.rs",
        "/// Find the docs of a \"type\", in a \\ path.
pub async fn find_docs(store: &Store, r#type: Option<String>, r#in: &str, class: Option<u32>) -> Result<Vec<Value>, anyhow::Error> { todo!() }
",
    );
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });
    config
}

/// The `use` paths of a generated file, compacted (`crate::schema::{A,B}`).
fn use_paths(code: &str) -> Vec<String> {
    syn::parse_file(code)
        .expect("the generated file parses")
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Use(u) => Some(compact(&quote::ToTokens::to_token_stream(&u.tree).to_string())),
            _ => None,
        })
        .collect()
}

/// The names of the types a generated file defines itself.
fn defined_types(code: &str) -> Vec<String> {
    syn::parse_file(code)
        .expect("the generated file parses")
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(i) => Some(i.ident.to_string()),
            syn::Item::Enum(i) => Some(i.ident.to_string()),
            syn::Item::Type(i) => Some(i.ident.to_string()),
            syn::Item::Trait(i) => Some(i.ident.to_string()),
            syn::Item::Union(i) => Some(i.ident.to_string()),
            _ => None,
        })
        .collect()
}

/// Whether `line` writes a path into a runtime crate without the leading
/// `::` that keeps a same-named consumer module from shadowing it.
fn names_a_crate_rootless(line: &str) -> bool {
    const CRATES: [&str; 9] =
        ["std", "serde", "serde_json", "schemars", "tokio", "uuid", "ontogen_core", "tauri", "log"];
    CRATES.iter().any(|krate| {
        let path = format!("{krate}::");
        line.match_indices(&path)
            .any(|(at, _)| line[..at].chars().next_back().is_none_or(|c| c != ':' && c != '_' && !c.is_alphanumeric()))
    })
}

/// IPC and MCP bring into scope bare only the consumer's names: its API
/// modules, the types its fns name, its state and store. Runtime items are
/// written by `::`-rooted paths, so an entity `Value` (once
/// `serde_json::Value` on MCP), `State` (`tauri::State` on IPC), `Arc` or
/// `Future` collides with nothing and a module `log` shadows no crate. The
/// file's own types are `Ontogen`-prefixed but for those public in 0.8.0.
/// A module named after a keyword is written raw (`r#match`).
#[test]
fn ipc_and_mcp_import_only_the_consumers_names() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, generator, public) in [
        ("ipc", ipc_gen as fn(PathBuf) -> ServerGenerator, &["PaginatedResult"][..]),
        ("mcp", mcp_gen, &["McpToolDef", "GetByIdInput", "ByIntIdInput", "EmptyInput", "SimpleToolDef"][..]),
    ] {
        let config = name_safety_config(tmp.path());
        let code = generate_one(tmp.path(), config.clone(), generator);

        let mut names = imported_names(&code);
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "{name}: a name is imported twice:\n{code}");
        for entity in ["Arc", "Future", "Log", "Match", "State", "Value"] {
            assert!(names.contains(&entity.to_string()), "{name} does not import the entity `{entity}`:\n{code}");
        }

        let consumer = [
            format!("{}::", config.service_import_path),
            format!("{}::", config.types_import_path),
            config.state_import.clone(),
            config.store_import.clone().unwrap(),
        ];
        for path in use_paths(&code) {
            assert!(consumer.iter().any(|c| path.starts_with(c.as_str())), "{name}: `use {path}` is no consumer path");
        }
        let modules = use_paths(&code).into_iter().find(|p| p.starts_with("crate::api::v1::")).unwrap();
        assert!(modules.contains("r#match"), "{name}: {modules}");
        assert!(compact(&code).contains("r#match::list(&ontogen_store"), "{name}:\n{code}");
        if name == "ipc" {
            assert!(compact(&code).contains("crate::api::v1::r#match::match_changes(state)"), "{code}");
        }

        for ty in defined_types(&code) {
            assert!(ty.starts_with("Ontogen") || public.contains(&ty.as_str()), "{name} defines `{ty}`:\n{code}");
        }

        // The consumer's own `log` module is called bare, as every module is.
        let found: Vec<&str> = code
            .lines()
            .filter(|l| {
                !l.trim_start().starts_with("//") && names_a_crate_rootless(&l.replace("log::log_changes(", ""))
            })
            .collect();
        assert!(found.is_empty(), "{name}: a runtime path a module can shadow: {found:#?}");
    }
}

/// A fn argument named after a keyword is bound raw (`r#type`) but travels
/// under serde's name for it (`type`): the MCP input schema advertises
/// `type`, so the tool reads `type`, and its errors name `type`. An IPC
/// command takes the raw parameter, from which Tauri derives the key
/// `type` itself.
#[test]
fn a_raw_argument_travels_under_its_name_without_r_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let mcp = generate_one(tmp.path(), name_safety_config(tmp.path()), mcp_gen);
    assert!(!mcp.contains("\"r#"), "a wire key keeps its `r#`:\n{mcp}");
    let flat = compact(&mcp);
    for line in [
        // A custom op's arguments, as its schema struct names them.
        "pub struct OntogenLookupFindDocsInput { pub r#type: Option<String>, pub r#in: String, pub class: \
         Option<u32>, }",
        "let r#type: Option<String> = ontogen_args.get(\"type\").filter(|v| !v.is_null()).cloned()\
         .map(::serde_json::from_value::<String>).transpose()\
         .map_err(|e| format!(\"Invalid parameter type: {e}\"))?;",
        "let r#in = required_str(ontogen_args, \"in\")?;",
        "lookup::find_docs(&ontogen_store, r#type, r#in, class)",
        // A list's bare filter, read beside the page.
        "let r#type: Option<String> = ontogen_args.get(\"type\")",
        "future::list(&ontogen_store, r#type.as_deref(), Some(ontogen_limit), Some(ontogen_offset))",
        // The doc that quotes is an escaped literal.
        "description: \"Find the docs of a \\\"type\\\", in a \\\\ path.\",",
    ] {
        assert!(flat.contains(&compact(line)), "{line}:\n{mcp}");
    }

    let ipc = generate_one(tmp.path(), name_safety_config(tmp.path()), ipc_gen);
    let flat = compact(&ipc);
    for line in [
        "pub async fn lookup_find_docs(r#type: Option<String>, r#in: String, class: Option<u32>,",
        "lookup::find_docs(&ontogen_store, r#type, &r#in, class)",
        "pub async fn arc_changes_subscribe(r#in: Option<String>,",
        "pub async fn future_list(r#type: Option<String>, limit: Option<u32>, offset: Option<u32>,",
    ] {
        assert!(flat.contains(&compact(line)), "{line}:\n{ipc}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// parse.rs + generators - #[ontogen::http::post] (attribute-driven POST opt-in)
// ═══════════════════════════════════════════════════════════════════════════════

/// `#[ontogen::http::post]` on a zero-user-param stateless fn flips
/// classification to `OpKind::CustomPost`. The default classifier would
/// route the same fn as `OpKind::CustomGet` because it has no params to
/// carry a body.
#[test]
fn test_post_attr_zero_param_classifies_as_custom_post() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "engine.rs",
        r#"
#[ontogen::stateless]
#[ontogen::http::post]
pub fn pause() -> Result<(), anyhow::Error> { todo!() }

// Sibling fn without the attribute — uses a known-read prefix so it
// classifies as CustomGet via the prefix allowlist (post zero-param
// default flip; non-prefix names like `status` would default to POST).
#[ontogen::stateless]
pub fn get_status() -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(scan.skips.is_empty(), "post fn must not produce SkipRecord: {:?}", scan.skips);
    let module = scan.modules.iter().find(|m| m.name == "engine").expect("engine module must be parsed");

    let pause_fn = module.functions.iter().find(|f| f.name == "pause").expect("pause fn must be present");
    assert!(
        matches!(pause_fn.force_method, Some(ForcedMethod::Post)),
        "#[ontogen::http::post] must stamp force_method=Some(ForcedMethod::Post)"
    );
    assert!(matches!(classify_op(module, pause_fn), OpKind::CustomPost), "ForcedMethod::Post must produce CustomPost");

    let get_status_fn = module.functions.iter().find(|f| f.name == "get_status").unwrap();
    assert!(get_status_fn.force_method.is_none(), "unmarked fn must keep force_method=None");
    assert!(
        matches!(classify_op(module, get_status_fn), OpKind::CustomGet),
        "unmarked zero-param fn with known-read prefix must classify as CustomGet"
    );
}

/// The bare `#[post]` path form (after `use ontogen::http::post`) is also
/// accepted — parser matches on the final path segment, mirroring
/// `has_stateless_attr`.
#[test]
fn test_post_attr_bare_path_form_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "data.rs",
        r#"
use ontogen::http::post;
use ontogen::stateless;

#[stateless]
#[post]
pub fn backup() -> Result<(), anyhow::Error> { todo!() }
"#,
    );

    let scan = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store"));
    assert!(scan.skips.is_empty(), "bare #[post] must be accepted: {:?}", scan.skips);
    let module = scan.modules.iter().find(|m| m.name == "data").unwrap();
    let backup_fn = module.functions.iter().find(|f| f.name == "backup").unwrap();
    assert!(
        matches!(backup_fn.force_method, Some(ForcedMethod::Post)),
        "bare #[post] must stamp force_method=Some(ForcedMethod::Post)"
    );
    assert!(matches!(classify_op(module, backup_fn), OpKind::CustomPost));
}

/// End-to-end HTTP: a `#[ontogen::http::post]`-annotated zero-param
/// stateless fn emits a `post(...)` route in `entity_routes()`, not
/// `get(...)`. Without the attribute the same fn would route as GET
/// (zero-param custom).
#[test]
fn test_post_attr_emits_post_http_route() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "engine.rs",
        r#"
#[ontogen::stateless]
#[ontogen::http::post]
pub fn pause() -> Result<(), anyhow::Error> { todo!() }

#[ontogen::stateless]
pub fn get_status() -> Result<String, anyhow::Error> { todo!() }
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let output_path = tmp.path().join("http_generated.rs");
    let config = test_config(tmp.path().to_path_buf());
    crate::servers::generators::http::generate(&output_path, &modules, &config);

    let content = std::fs::read_to_string(&output_path).unwrap();

    // The `pause` route must use `post(`, never `get(`.
    let pause_idx = content.find("engine_pause").expect("engine_pause handler must be emitted");
    // Inspect the route line referencing engine_pause (search backward to find the
    // .route(...) call).
    assert!(
        content.contains("post(engine_pause)"),
        "pause route must use post(...), found:\n{}",
        &content[pause_idx.saturating_sub(200)..(pause_idx + 200).min(content.len())]
    );
    assert!(
        !content.contains("get(engine_pause)"),
        "pause route must not use get(...) — ForcedMethod::Post should win:\n{content}"
    );

    // The `get_status` sibling routes as GET via the known-read prefix
    // allowlist (without the attribute, non-prefix names would default to
    // POST post zero-param default flip — `get_` opts back into GET).
    assert!(
        content.contains("get(engine_get_status)"),
        "get_status fn must route as GET via known-read prefix:\n{content}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// OF-003 - Per-function command-name override
// ═══════════════════════════════════════════════════════════════════════════════

/// Build a one-function module with an explicit command override, mirroring
/// the canonical OF-003 example (`journal::get_tag_history` renamed to
/// `tag_get_history`).
fn make_renamed_module(override_value: Option<&str>) -> ApiModule {
    ApiModule {
        name: "journal".to_string(),
        functions: vec![ApiFn {
            name: "get_tag_history".to_string(),
            doc: "Get the tag history.".to_string(),
            params: vec![param("tag", "&str")],
            return_type: "Vec<HistoryEntry>".to_string(),
            return_type_ast: ty_ast("Vec<HistoryEntry>"),
            first_param_is_store: true,
            command_override: override_value.map(|s| s.to_string()),
            ..Default::default()
        }],
        events: vec![],
        is_singleton: false,
        has_count: false,
    }
}

#[test]
fn test_parse_rename_attribute() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "journal.rs",
        r#"
use crate::store::Store;

/// Get tag history.
#[ontogen(rename = "tag_get_history")]
pub fn get_tag_history(store: &Store, tag: &str) -> Result<Vec<HistoryEntry>, anyhow::Error> {
    todo!()
}
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1);
    let f = &modules[0].functions[0];
    assert_eq!(f.name, "get_tag_history");
    assert_eq!(f.command_override.as_deref(), Some("tag_get_history"));
}

#[test]
fn test_parse_no_rename_attribute_leaves_override_none() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "journal.rs",
        r#"
use crate::store::Store;

/// Get tag history.
pub fn get_tag_history(store: &Store, tag: &str) -> Result<Vec<HistoryEntry>, anyhow::Error> {
    todo!()
}
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    let f = &modules[0].functions[0];
    assert!(f.command_override.is_none(), "expected no override, got {:?}", f.command_override);
}

#[test]
fn test_parse_ontogen_attribute_without_rename_is_ignored() {
    // Unknown directives inside `#[ontogen(...)]` must not crash the parser
    // and must not populate `command_override` - the function should still
    // pass through with its default naming.
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "journal.rs",
        r#"
use crate::store::Store;

/// Get tag history.
#[ontogen(stateless)]
pub fn get_tag_history(store: &Store, tag: &str) -> Result<Vec<HistoryEntry>, anyhow::Error> {
    todo!()
}
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].functions.len(), 1, "function should still be parsed");
    let f = &modules[0].functions[0];
    assert!(f.command_override.is_none(), "unknown directive should leave override None");
}

#[test]
fn test_parse_rename_attribute_with_invalid_value_drops_fn() {
    // A non-string literal value (e.g., `rename = 42`) is malformed. The
    // function is dropped from the parsed module so the mistake is visible
    // at build time rather than silently falling back to the default name.
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");

    write_synthetic_api(
        &api_dir,
        "journal.rs",
        r#"
use crate::store::Store;

/// Valid function - kept.
pub fn list(store: &Store) -> Result<Vec<HistoryEntry>, anyhow::Error> { todo!() }

/// Malformed override - dropped.
#[ontogen(rename = 42)]
pub fn get_tag_history(store: &Store, tag: &str) -> Result<Vec<HistoryEntry>, anyhow::Error> {
    todo!()
}
"#,
    );

    let modules = crate::servers::parse::scan_api_dir(&api_dir, "AppState", Some("Store")).modules;
    assert_eq!(modules.len(), 1);
    let names: Vec<&str> = modules[0].functions.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"list"), "valid sibling fn should survive");
    assert!(!names.contains(&"get_tag_history"), "fn with malformed `rename` value should be dropped, got {:?}", names);
}

#[test]
fn test_apply_command_overrides_populates_field() {
    let mut modules = vec![make_renamed_module(None)];
    let mut naming = NamingConfig::default();
    naming.command_overrides.insert("journal::get_tag_history".to_string(), "tag_get_history".to_string());

    crate::servers::parse::apply_command_overrides(&mut modules, &naming);

    let f = &modules[0].functions[0];
    assert_eq!(f.command_override.as_deref(), Some("tag_get_history"));
}

#[test]
fn test_command_overrides_source_wins_over_config() {
    // Source-side attribute already set the override. The config map for
    // the same key must NOT overwrite it.
    let mut modules = vec![make_renamed_module(Some("from_source"))];
    let mut naming = NamingConfig::default();
    naming.command_overrides.insert("journal::get_tag_history".to_string(), "from_config".to_string());

    crate::servers::parse::apply_command_overrides(&mut modules, &naming);

    let f = &modules[0].functions[0];
    assert_eq!(f.command_override.as_deref(), Some("from_source"), "source-side override must win over config map");
}

#[test]
fn test_command_name_uses_override_when_set() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf());
    let module = make_renamed_module(Some("tag_get_history"));
    let f = &module.functions[0];

    let name = crate::servers::generators::ipc::command_name(&module.name, f, &config);
    assert_eq!(name, "tag_get_history", "override should be returned verbatim");
}

#[test]
fn test_command_name_falls_back_when_override_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(tmp.path().to_path_buf());
    let module = make_renamed_module(None);
    let f = &module.functions[0];

    let name = crate::servers::generators::ipc::command_name(&module.name, f, &config);
    assert_eq!(name, "journal_get_tag_history", "default scheme is `{{entity}}_{{fn}}`");
}

#[test]
fn test_ipc_handler_uses_override_function_name() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());

    let modules = vec![make_renamed_module(Some("tag_get_history"))];
    crate::servers::generators::ipc::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();
    assert!(
        content.contains("pub async fn tag_get_history("),
        "IPC handler should use override as the command name. Generated:\n{}",
        content
    );
    assert!(
        !content.contains("pub async fn journal_get_tag_history("),
        "default-scheme name should NOT appear when override is set"
    );
}

#[test]
fn test_ipc_handler_calls_unchanged_rust_fn() {
    // The override changes only the emitted command name, not the
    // underlying Rust path the handler delegates to.
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("ipc_generated.rs");
    let config = test_config(tmp.path().to_path_buf());

    let modules = vec![make_renamed_module(Some("tag_get_history"))];
    crate::servers::generators::ipc::generate(&output, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();
    assert!(
        content.contains("journal::get_tag_history("),
        "handler body must still call the original Rust function path. Generated:\n{}",
        content
    );
}

#[test]
fn test_ts_client_uses_override_camelcased() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("http-client.ts");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type HistoryEntry = { tag: string; ts: number; };\n").unwrap();

    let config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_renamed_module(Some("tag_get_history"))];

    crate::clients::generators::ts_client::generate(&output, &bindings, &modules, &config);

    let content = std::fs::read_to_string(&output).unwrap();
    assert!(
        content.contains("tagGetHistory"),
        "TS client method name should be camelCased override. Generated:\n{}",
        content
    );
    assert!(
        !content.contains("journalGetTagHistory"),
        "default-scheme camelCase should NOT appear when override is set"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// API surfaces - a second api_dir with its own accessor, merged into one transport
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_two_surfaces_emit_each_accessor() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = two_surface_config(two_surface_fixture(tmp.path()));
    let http_out = tmp.path().join("http.rs");
    let ipc_out = tmp.path().join("ipc.rs");
    let mcp_out = tmp.path().join("mcp.rs");
    config.generators = vec![
        ServerGenerator::HttpAxum { output: http_out.clone() },
        ServerGenerator::TauriIpc { output: ipc_out.clone() },
        ServerGenerator::Mcp { output: mcp_out.clone() },
    ];

    crate::servers::generate_transport(&config).expect("generate_transport failed");

    let http = std::fs::read_to_string(&http_out).unwrap();
    assert!(
        http.contains("let ontogen_store = ontogen_state.fitness_store().await.map_err(ontogen_internal_error)?;"),
        "second-surface handlers open the store through the surface accessor:\n{http}"
    );
    assert!(
        http.contains("let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;"),
        "primary-surface handlers keep the default accessor:\n{http}"
    );
    assert!(http.contains("athlete::list(&ontogen_store)"), "primary store module calls through its own name:\n{http}");

    let ipc = std::fs::read_to_string(&ipc_out).unwrap();
    assert!(ipc.contains("state.fitness_store().await"), "IPC uses the surface accessor:\n{ipc}");
    assert!(ipc.contains("state.store().await"), "IPC keeps the default accessor:\n{ipc}");

    let mcp = std::fs::read_to_string(&mcp_out).unwrap();
    assert!(mcp.contains("state.fitness_store().await"), "MCP uses the surface accessor:\n{mcp}");
    assert!(mcp.contains("state.store().await"), "MCP keeps the default accessor:\n{mcp}");
}

#[test]
fn test_two_surfaces_merge_same_named_module() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = two_surface_config(two_surface_fixture(tmp.path()));
    let http_out = tmp.path().join("http.rs");
    let ipc_out = tmp.path().join("ipc.rs");
    config.generators = vec![
        ServerGenerator::HttpAxum { output: http_out.clone() },
        ServerGenerator::TauriIpc { output: ipc_out.clone() },
    ];

    let modules = crate::servers::generate_transport(&config).expect("generate_transport failed");

    let names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["athlete", "workout", "exercise"], "primary order first, new modules appended");
    let workout = modules.iter().find(|m| m.name == "workout").unwrap();
    assert_eq!(workout.functions.len(), 7, "custom fns and CRUD five merge into one module");
    assert_eq!(workout.base_surface(), 0);
    assert_eq!(workout.service_ident(0), "workout");
    assert_eq!(workout.service_ident(1), "workout_1");

    let http = std::fs::read_to_string(&http_out).unwrap();
    for route in [
        ".route(\"/api/workouts\", axum::routing::get(workout_list).post(workout_create).fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::POST])))",
        ".route(\"/api/workouts/{id}\", axum::routing::get(workout_get_by_id).patch(workout_update).delete(workout_delete)\
         .fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::PATCH, OntogenMethod::DELETE])))",
        ".route(\"/api/workouts/start\", axum::routing::post(workout_start).fallback(ontogen_allow([OntogenMethod::POST])))",
        ".route(\"/api/workouts/summary/{id}\", axum::routing::get(workout_get_summary).fallback(ontogen_allow([OntogenMethod::GET])))",
        ".route(\"/api/exercises\", axum::routing::get(exercise_list).post(exercise_create).fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::POST])))",
    ] {
        assert!(compact(&http).contains(&compact(route)), "expected route {route} in:\n{http}");
    }
    assert!(http.contains("workout as workout_1"), "second surface's workout is aliased:\n{http}");
    assert!(http.contains("workout_1::list(&ontogen_store)"), "CRUD handlers call through the alias:\n{http}");
    assert!(http.contains("workout::start(&ontogen_state, input)"), "custom handlers call the primary module:\n{http}");
    // A meta-only document names no result type, so neither `Workout` is
    // imported into the HTTP handlers.
    assert!(!http.contains("Workout,") && !http.contains("fitness::schema::Workout"), "{http}");
    let mut names = imported_names(&http);
    let count = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), count, "no name is imported twice:\n{http}");

    let ipc = std::fs::read_to_string(&ipc_out).unwrap();
    for cmd in
        ["workout_list", "workout_get_by_id", "workout_create", "workout_update", "workout_delete", "workout_start"]
    {
        assert!(ipc.contains(&format!("        {cmd},\n")), "ipc_handler lists {cmd}:\n{ipc}");
    }
    assert!(ipc.contains("workout_1::create(&ontogen_store, input)"), "IPC CRUD calls through the alias:\n{ipc}");
    assert!(
        ipc.contains("fitness::schema::Workout") && ipc.contains("Result<Workout, String>"),
        "the shared type name is bare for the primary surface and qualified for the second:\n{ipc}"
    );
}

#[test]
fn test_two_surfaces_duplicate_fn_is_error() {
    let tmp = tempfile::tempdir().unwrap();
    let surfaces = two_surface_fixture(tmp.path());
    write_synthetic_api(
        &surfaces[1].api_dir,
        "workout.rs",
        &format!(
            "{}pub async fn start(store: &FitnessStore, input: StartWorkoutInput) -> Result<Workout, anyhow::Error> {{ todo!() }}\n",
            crud_module_source("workout", "FitnessStore")
        ),
    );
    let primary_dir = surfaces[0].api_dir.display().to_string();
    let fitness_dir = surfaces[1].api_dir.display().to_string();
    let config = two_surface_config(surfaces);

    let err = crate::servers::generate_transport(&config).expect_err("duplicate fn must fail");
    assert!(err.contains("`workout::start`"), "names the module and fn: {err}");
    assert!(err.contains(&primary_dir) && err.contains(&fitness_dir), "names both surfaces: {err}");
}

#[test]
fn test_two_surfaces_crud_split_is_error() {
    let tmp = tempfile::tempdir().unwrap();
    let surfaces = two_surface_fixture(tmp.path());
    write_synthetic_api(
        &surfaces[0].api_dir,
        "workout.rs",
        "pub async fn list(store: &Store) -> Result<Vec<Workout>, anyhow::Error> { todo!() }\n",
    );
    std::fs::remove_file(surfaces[1].api_dir.join("workout.rs")).unwrap();
    write_synthetic_api(
        &surfaces[1].api_dir,
        "workout.rs",
        "pub async fn get_by_id(store: &FitnessStore, id: &str) -> Result<Workout, anyhow::Error> { todo!() }\n",
    );
    let config = two_surface_config(surfaces);

    let err = crate::servers::generate_transport(&config).expect_err("split CRUD must fail");
    assert!(err.contains("CRUD functions of module `workout`"), "{err}");
    assert!(err.contains("`list`") && err.contains("`get_by_id`"), "{err}");
}

#[test]
fn test_route_prefix_rejects_extra_surface_with_store_type() {
    let tmp = tempfile::tempdir().unwrap();
    let surfaces = two_surface_fixture(tmp.path());
    let fitness_dir = surfaces[1].api_dir.display().to_string();
    let mut config = two_surface_config(surfaces);
    config.route_prefix = test_config_with_prefix(config.api_dir.clone()).route_prefix;

    let err = crate::servers::generate_transport(&config).expect_err("route_prefix + store-scoped extra surface");
    assert!(err.contains("`route_prefix`") && err.contains("`store_type`"), "{err}");
    assert!(err.contains(&fitness_dir) && err.contains("`FitnessStore`"), "names the surface: {err}");
    assert!(err.contains("`store_for`"), "names the prefix accessor: {err}");

    // An extra surface with no store_type has only state-scoped fns, which
    // route_prefix handles like the primary's, so it is still accepted.
    config.extra_surfaces[0].store_type = None;
    crate::servers::generate_transport(&config).expect("state-only extra surface is fine with route_prefix");
}

/// With `route_prefix`, whether a fn gets scoped-only or unscoped routes is
/// decided per fn, not by the module's first fn - so a merged module that
/// mixes state-scoped custom fns with store-scoped CRUD emits the same
/// routes whichever surface's fns come first.
#[test]
fn test_mixed_module_routes_are_per_fn_and_order_independent() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config_with_prefix(tmp.path().to_path_buf());

    let custom = ApiFn {
        name: "start".to_string(),
        is_async: true,
        doc: "Start a workout.".to_string(),
        params: vec![param("input", "StartWorkoutInput")],
        return_type: "Workout".to_string(),
        return_type_ast: ty_ast("Workout"),
        ..Default::default()
    };
    let mut state_first = make_crud_module("workout", true);
    state_first.functions.insert(0, custom.clone());
    let mut store_first = make_crud_module("workout", true);
    store_first.functions.push(custom);

    let mut outputs = Vec::new();
    for (i, module) in [state_first, store_first].into_iter().enumerate() {
        let output = tmp.path().join(format!("http_{i}.rs"));
        crate::servers::generators::http::generate(&output, std::slice::from_ref(&module), &config);
        let http = std::fs::read_to_string(&output).unwrap();

        assert!(
            http.contains(".route(\"/api/workouts/start\", axum::routing::post(workout_start).fallback(ontogen_allow([OntogenMethod::POST])))"),
            "state-scoped fn keeps its unscoped route:\n{http}"
        );
        assert!(http.contains("workout::start(&ontogen_state, input)"), "state-scoped fn takes &state:\n{http}");
        assert!(
            !http.contains(".route(\"/api/workouts\", ") && !http.contains(".route(\"/api/workouts/{id}\", "),
            "store-scoped CRUD gets no unscoped routes:\n{http}"
        );
        assert!(
            compact(&http).contains(&compact(
                ".route(\"/api/projects/{project_id}/workouts\", axum::routing::get(workout_list_scoped)"
            )),
            "store-scoped CRUD gets scoped routes:\n{http}"
        );
        assert!(!http.contains("start_scoped"), "state-scoped fn gets no scoped route:\n{http}");

        let meta = crate::servers::extract_server_metadata(&[module], &config);
        let path = |handler: &str| meta.http_routes.iter().find(|r| r.handler_name == handler).unwrap().path.clone();
        assert_eq!(path("workout_start"), "/api/workouts/start");
        assert_eq!(path("workout_list_scoped"), "/api/projects/{project_id}/workouts");
        assert!(meta.http_routes.iter().all(|r| r.handler_name != "workout_list"), "{:#?}", meta.http_routes);

        outputs.push(http);
    }
    assert_eq!(outputs[0], outputs[1], "fn order within the module does not change the output");
}

#[test]
fn test_single_surface_stamps_default_accessor() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "node.rs", &crud_module_source("node", "Store"));
    let surfaces = vec![test_config(api_dir).primary_surface()];

    let scanned = crate::servers::parse::scan_surfaces(&surfaces, "AppState").unwrap();
    let f = &scanned.modules[0].functions[0];
    assert_eq!(f.surface, 0);
    assert_eq!(f.store_accessor, "store");
    assert!(f.first_param_is_store);
}

#[test]
fn test_surface_pagination_honours_paginated_modules() {
    let surface = crate::servers::ApiSurface {
        api_dir: PathBuf::from("unused"),
        service_import_path: String::new(),
        types_import_path: String::new(),
        store_accessor: None,
        store_type: None,
        pagination: Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 }),
        paginated_modules: vec!["exercise".to_string()],
        schema_dir: None,
    };
    assert!(surface.pagination_for("exercise").is_some());
    assert!(surface.pagination_for("workout").is_none());
    assert_eq!(surface.store_accessor(), "store");

    let mut config = test_config(PathBuf::from("unused"));
    config.extra_surfaces = vec![surface];
    assert!(config.pagination_for("workout", 0).is_none(), "primary surface has no pagination");
    assert!(config.pagination_for("exercise", 1).is_some());
    assert!(config.pagination_for("workout", 1).is_none());
    assert!(config.any_pagination());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pagination pushdown: the page reaches the store, the total is a count
// ═══════════════════════════════════════════════════════════════════════════════

/// A CRUD module whose `list` takes the page and that carries a `count`,
/// the shape `ApiConfig::paginated` generates.
pub(crate) fn paged_crud_module_source(entity: &str, store_type: &str) -> String {
    let pascal = capitalize(entity);
    format!(
        "pub async fn list(store: &{st}, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<{p}>, anyhow::Error> {{ todo!() }}
pub async fn count(store: &{st}) -> Result<u64, anyhow::Error> {{ todo!() }}
pub async fn get_by_id(store: &{st}, id: &str) -> Result<{p}, anyhow::Error> {{ todo!() }}
pub async fn create(store: &{st}, input: Create{p}Input) -> Result<{p}, anyhow::Error> {{ todo!() }}
pub async fn update(store: &{st}, id: &str, input: Update{p}Input) -> Result<{p}, anyhow::Error> {{ todo!() }}
pub async fn delete(store: &{st}, id: &str) -> Result<(), anyhow::Error> {{ todo!() }}
",
        st = store_type,
        p = pascal,
    )
}

#[test]
fn a_paginated_list_pushes_the_page_into_the_store() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "workout.rs", &paged_crud_module_source("workout", "Store"));
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let workout = modules.iter().find(|m| m.name == "workout").unwrap();
    assert!(workout.has_count, "`count` is recorded on the module");
    assert!(workout.functions.iter().any(|f| f.name == "count"), "`count` is parsed like any other fn");
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();
    let workout = modules.iter().find(|m| m.name == "workout").unwrap();
    assert!(workout.functions.iter().all(|f| f.name != "count"), "`count` is not an operation of a paginated module");
    assert!(workout.is_crud(), "the CRUD surface is intact without `count`");

    let http = tmp.path().join("http.rs");
    crate::servers::generators::http::generate(&http, &modules, &config);
    let http = std::fs::read_to_string(&http).unwrap();
    assert!(
        http.contains("workout::list(&ontogen_store, Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset)))"),
        "the HTTP page handler passes the page down:\n{http}"
    );
    assert!(http.contains("workout::count(&ontogen_store)"), "the HTTP page handler asks for the total:\n{http}");
    assert!(!http.contains(".len() as u64"), "nothing is materialised to be counted:\n{http}");
    assert!(!http.contains("Query(limit)"), "limit is the page, not a filter:\n{http}");

    let ipc = tmp.path().join("ipc.rs");
    crate::servers::generators::ipc::generate(&ipc, &modules, &config);
    let ipc = std::fs::read_to_string(&ipc).unwrap();
    assert!(
        ipc.contains("workout::list(&ontogen_store, Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset)))"),
        "the IPC page command passes the page down:\n{ipc}"
    );
    assert!(ipc.contains("workout::count(&ontogen_store)"), "the IPC page command asks for the total:\n{ipc}");
    assert_eq!(ipc.matches("limit: Option<u32>").count(), 1, "the page params appear once:\n{ipc}");

    let mcp = tmp.path().join("mcp.rs");
    crate::servers::generators::mcp::generate(&mcp, &modules, &config);
    let mcp = std::fs::read_to_string(&mcp).unwrap();
    assert!(
        mcp.contains("workout::list(&ontogen_store, Some(ontogen_limit), Some(ontogen_offset))"),
        "the MCP tool passes the page down:\n{mcp}"
    );
    assert!(mcp.contains("workout::count(&ontogen_store)"), "the MCP tool asks for the total:\n{mcp}");
    assert!(!mcp.contains("ontogen_all"), "nothing is materialised to be sliced:\n{mcp}");
    assert!(
        !mcp.contains(r#"required_str(ontogen_args, "limit")"#),
        "the page is read from args, not demanded as a tool argument:\n{mcp}"
    );
}

/// The same shape, scoped to the state instead of a store. An app that reaches
/// its data through `AppState` rather than a generated `Store` still paginates:
/// the generators take the first argument from `list`, so `count` follows it.
#[test]
fn a_state_scoped_count_paginates_the_same_way() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "workout.rs", &paged_crud_module_source("workout", "AppState"));
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let workout = modules.iter().find(|m| m.name == "workout").unwrap();
    assert!(workout.has_count, "`count` is recorded on a state-scoped module too");
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();
    let workout = modules.iter().find(|m| m.name == "workout").unwrap();
    assert!(workout.functions.iter().all(|f| f.name != "count"), "`count` is not an operation once paginated");

    let http = tmp.path().join("http.rs");
    crate::servers::generators::http::generate(&http, &modules, &config);
    let http = std::fs::read_to_string(&http).unwrap();
    assert!(
        http.contains("workout::list(&ontogen_state, Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset)))"),
        "the HTTP page handler passes the page down:\n{http}"
    );
    assert!(http.contains("workout::count(&ontogen_state)"), "the total is asked of the state:\n{http}");

    let ipc = tmp.path().join("ipc.rs");
    crate::servers::generators::ipc::generate(&ipc, &modules, &config);
    let ipc = std::fs::read_to_string(&ipc).unwrap();
    assert!(
        ipc.contains("workout::list(&ontogen_state, Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset)))"),
        "the IPC page command passes the page down:\n{ipc}"
    );
    assert!(ipc.contains("workout::count(&ontogen_state)"), "the total is asked of the state:\n{ipc}");

    let mcp = tmp.path().join("mcp.rs");
    crate::servers::generators::mcp::generate(&mcp, &modules, &config);
    let mcp = std::fs::read_to_string(&mcp).unwrap();
    assert!(
        mcp.contains("workout::list(ontogen_state, Some(ontogen_limit), Some(ontogen_offset))"),
        "the MCP tool passes the page down:\n{mcp}"
    );
    assert!(mcp.contains("workout::count(ontogen_state)"), "the MCP tool asks the state for the total:\n{mcp}");
    assert!(!mcp.contains("ontogen_all"), "nothing is materialised to be sliced:\n{mcp}");
}

#[test]
fn a_paginated_list_without_the_page_or_a_count_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "workout.rs", &crud_module_source("workout", "Store"));
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let err = crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces)
        .unwrap_err();
    assert!(err.contains("module `workout` is paginated"), "{err}");
    assert!(err.contains("`workout::list` must take `limit: Option<u64>, offset: Option<u64>`"), "{err}");

    // Not paginated: the plain list is fine as it is.
    config.pagination = None;
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();
}

#[test]
fn a_paginated_list_whose_page_params_are_not_option_u64_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "workout.rs",
        &paged_crud_module_source("workout", "Store").replace("Option<u64>", "Option<usize>"),
    );
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    assert!(!modules[0].functions.iter().find(|f| f.name == "list").unwrap().takes_page());
    let err = crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces)
        .unwrap_err();
    assert!(err.contains("`workout::list` must take `limit: Option<u64>, offset: Option<u64>`"), "{err}");
}

/// A filtered page is fine as long as the total counts the same rows: the
/// `count` takes the filter the `list` takes.
#[test]
fn a_paginated_list_may_filter_when_its_count_filters_alike() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "workout.rs",
        &paged_crud_module_source("workout", "Store")
            .replace("store: &Store, limit", "store: &Store, plan_id: &str, limit")
            .replace("count(store: &Store)", "count(store: &Store, plan_id: &str)"),
    );
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();

    let http = tmp.path().join("http.rs");
    crate::servers::generators::http::generate(&http, &modules, &config);
    let http = std::fs::read_to_string(&http).unwrap();
    let flat = compact(&http);
    assert!(
        flat.contains(&compact(
            "workout::list(&ontogen_store, &ontogen_filter_plan_id, Some(u64::from(ontogen_limit)), \
             Some(u64::from(ontogen_offset)))"
        )),
        "the page carries the filter:\n{http}"
    );
    assert!(
        flat.contains(&compact("workout::count(&ontogen_store, &ontogen_filter_plan_id)")),
        "the total carries the filter:\n{http}"
    );

    // IPC and MCP read the filter from their flat payload and pass it to
    // `list` and `count` alike.
    let ipc = tmp.path().join("ipc.rs");
    crate::servers::generators::ipc::generate(&ipc, &modules, &config);
    let ipc = compact(&std::fs::read_to_string(&ipc).unwrap());
    assert!(
        ipc.contains(
            "workout::list(&ontogen_store,&plan_id,Some(u64::from(ontogen_limit)),Some(u64::from(ontogen_offset)))"
        ),
        "{ipc}"
    );
    assert!(ipc.contains("workout::count(&ontogen_store,&plan_id)"), "{ipc}");
    let mcp = tmp.path().join("mcp.rs");
    crate::servers::generators::mcp::generate(&mcp, &modules, &config);
    let mcp = compact(&std::fs::read_to_string(&mcp).unwrap());
    assert!(mcp.contains("workout::list(&ontogen_store,plan_id,Some(ontogen_limit),Some(ontogen_offset))"), "{mcp}");
    assert!(mcp.contains("workout::count(&ontogen_store,plan_id)"), "{mcp}");
}

/// The filter is usually a by-value `Query` struct. `list` consumes it, so the
/// generators hand `list` a clone and give `count` the original.
#[test]
fn a_by_value_filter_is_cloned_into_the_list_and_counted_from_the_original() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "workout.rs",
        &paged_crud_module_source("workout", "Store")
            .replace("store: &Store, limit", "store: &Store, query: ListWorkoutQuery, limit")
            .replace("count(store: &Store)", "count(store: &Store, query: ListWorkoutQuery)"),
    );
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();

    for (name, emit) in [
        (
            "ipc",
            crate::servers::generators::ipc::generate
                as fn(&std::path::Path, &[crate::servers::parse::ApiModule], &Config),
        ),
        ("http", crate::servers::generators::http::generate),
        ("mcp", crate::servers::generators::mcp::generate),
    ] {
        let out = tmp.path().join(format!("{name}.rs"));
        emit(&out, &modules, &config);
        let out = std::fs::read_to_string(&out).unwrap();
        let (store, filter) = if name == "http" || name == "mcp" {
            ("ontogen_store", "ontogen_filter")
        } else {
            ("ontogen_store", "query")
        };
        let flat = compact(&out);
        assert!(
            flat.contains(&format!("workout::list(&{store},{filter}.clone()")),
            "{name}: the list takes a clone:\n{out}"
        );
        assert!(
            flat.contains(&format!("workout::count(&{store},{filter})")),
            "{name}: the total takes the original:\n{out}"
        );
    }
}

/// IPC and MCP for a paginated `workout::list` taking `filter` before its
/// page, with a `count` taking the same filter: each file as generated, and
/// compacted.
fn typed_filter_transports(filter: &str) -> [(&'static str, String, String); 2] {
    filter_transports(filter, test_config)
}

/// [`typed_filter_transports`] under the config `config` makes.
fn filter_transports(filter: &str, config: fn(PathBuf) -> Config) -> [(&'static str, String, String); 2] {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "workout.rs",
        &paged_crud_module_source("workout", "Store")
            .replace("store: &Store, limit", &format!("store: &Store, {filter}, limit"))
            .replace("count(store: &Store)", &format!("count(store: &Store, {filter})")),
    );
    let mut config = config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });
    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();
    let emit = |name: &'static str, generate: fn(&std::path::Path, &[ApiModule], &Config)| {
        let out = tmp.path().join(format!("{name}.rs"));
        generate(&out, &modules, &config);
        let code = std::fs::read_to_string(&out).unwrap();
        syn::parse_file(&code).unwrap_or_else(|e| panic!("{name} does not parse: {e}\n{code}"));
        (name, compact(&code), code)
    };
    [emit("ipc", crate::servers::generators::ipc::generate), emit("mcp", crate::servers::generators::mcp::generate)]
}

/// A bare filter of any one-value type reaches `list` and `count` on IPC
/// and MCP as its fn declares it: the IPC command takes it as its owned
/// type, and the MCP tool reads it from `args` as that type, optional when
/// it is an `Option`. A `&str` keeps the `required_str` read.
#[test]
fn ipc_and_mcp_read_a_typed_bare_filter() {
    let [(_, ipc, ipc_code), (_, mcp, mcp_code)] =
        typed_filter_transports("title: Option<&str>, owner: &str, limit_to: Option<u32>");

    for line in ["title: Option<String>,", "owner: String,", "limit_to: Option<u32>,"] {
        assert!(ipc.contains(&compact(line)), "{line}:\n{ipc_code}");
    }
    assert!(
        ipc.contains(&compact(
            "workout::list(&ontogen_store, title.as_deref(), &owner, limit_to, Some(u64::from(ontogen_limit)), \
             Some(u64::from(ontogen_offset)))"
        )),
        "{ipc_code}"
    );
    assert!(ipc.contains(&compact("workout::count(&ontogen_store, title.as_deref(), &owner, limit_to)")), "{ipc_code}");

    for line in [
        "let title: Option<String> = ontogen_args.get(\"title\").filter(|v| !v.is_null()).cloned()\
         .map(::serde_json::from_value::<String>).transpose()\
         .map_err(|e| format!(\"Invalid parameter title: {e}\"))?;",
        "let owner = required_str(ontogen_args, \"owner\")?;",
        "let limit_to: Option<u32> = ontogen_args.get(\"limit_to\").filter(|v| !v.is_null()).cloned()\
         .map(::serde_json::from_value::<u32>).transpose()\
         .map_err(|e| format!(\"Invalid parameter limit_to: {e}\"))?;",
        "workout::list(&ontogen_store, title.as_deref(), owner, limit_to, Some(ontogen_limit), Some(ontogen_offset))",
        "workout::count(&ontogen_store, title.as_deref(), owner, limit_to)",
    ] {
        assert!(mcp.contains(&compact(line)), "{line}:\n{mcp_code}");
    }
}

/// `count` takes the filter after `list`, so a filter `list` would consume
/// (the `*Query` struct, an owned `String`) is cloned into `list`, and a
/// copied one (`u32`) is not, as on HTTP. A required non-`&str` filter is
/// read from `args` as its type, a missing one being the tool's error.
#[test]
fn ipc_and_mcp_clone_only_the_filters_list_consumes() {
    let [(_, ipc, ipc_code), (_, mcp, mcp_code)] =
        typed_filter_transports("query: ListWorkoutQuery, tag: Option<String>, n: u32");

    assert!(ipc.contains(&compact("query: ListWorkoutQuery, tag: Option<String>, n: u32,")), "{ipc_code}");
    for (name, flat, code, store, query) in [
        ("ipc", &ipc, &ipc_code, "&ontogen_store", "query"),
        ("mcp", &mcp, &mcp_code, "&ontogen_store", "ontogen_filter"),
    ] {
        assert!(
            flat.contains(&compact(&format!("workout::list({store}, {query}.clone(), tag.clone(), n,"))),
            "{name}:\n{code}"
        );
        assert!(flat.contains(&compact(&format!("workout::count({store}, {query}, tag, n)"))), "{name}:\n{code}");
    }
    assert!(
        mcp.contains(&compact(
            "let n: u32 = ::serde_json::from_value(ontogen_args.get(\"n\").cloned()\
             .ok_or(\"Missing required parameter: n\")?)\
             .map_err(|e| format!(\"Invalid parameter n: {e}\"))?;"
        )),
        "{mcp_code}"
    );
}

/// The MCP list tool's input schema names every argument it reads: its bare
/// filters as their owned types, required unless `Option`, beside the
/// `*Query` struct's fields and the page. A filter struct the remaining
/// arguments cannot deserialize into is the tool's error, not an empty
/// filter, and an argument the schema does not name is refused.
#[test]
fn the_mcp_list_tool_advertises_its_bare_filters_and_refuses_a_malformed_filter() {
    let [_, (_, mcp, mcp_code)] = typed_filter_transports("query: &ListWorkoutQuery, title: Option<&str>, owner: &str");
    assert!(
        mcp.contains(&compact(
            "#[derive(::schemars::JsonSchema)] pub struct OntogenWorkoutListFilter { pub title: Option<String>, pub owner: \
             String, #[serde(flatten)] pub ontogen_query: ListWorkoutQuery, }"
        )),
        "{mcp_code}"
    );
    assert!(
        mcp.contains(&compact("schema_fn: || with_pagination_schema(schema_for::<OntogenWorkoutListFilter>()),")),
        "{mcp_code}"
    );
    assert!(
        mcp.contains(&compact(
            "refuse_unknown_args(ontogen_args, with_pagination_schema(schema_for::<OntogenWorkoutListFilter>()))?;"
        )),
        "{mcp_code}"
    );
    assert!(!mcp.contains("unwrap_or_default"), "{mcp_code}");
    assert!(
        mcp.contains(&compact("workout::count(&ontogen_store, &ontogen_filter, title.as_deref(), owner)")),
        "{mcp_code}"
    );

    // A struct alone is its own schema; a list with no filter takes none.
    let [_, (_, mcp, mcp_code)] = typed_filter_transports("query: ListWorkoutQuery");
    assert!(!mcp.contains("WorkoutListFilter"), "{mcp_code}");
    assert!(
        mcp.contains(&compact("schema_fn: || with_pagination_schema(schema_for::<ListWorkoutQuery>()),")),
        "{mcp_code}"
    );
}

/// The `*Query` struct of an MCP list tool is read from the tool's
/// arguments without the ones the tool reads itself, so a struct that
/// refuses unknown fields reads beside them: each bare filter, the page
/// when the tool reads one, and the scope's argument when scoped.
#[test]
fn the_mcp_list_tool_reads_its_filter_struct_without_its_own_arguments() {
    let read = |keys: &str| {
        compact(&format!(
            "let ontogen_filter: ListWorkoutQuery = ::serde_json::from_value({keys})\
             .map_err(|e| format!(\"Invalid filter: {{e}}\"))?;"
        ))
    };

    // Paged: the page.
    let [_, (_, mcp, code)] = typed_filter_transports("query: ListWorkoutQuery");
    assert!(mcp.contains(&read(r#"args_without(ontogen_args, &["limit", "offset"])"#)), "{code}");

    // Paged, beside bare filters: the bare filters too, in declaration order.
    let [_, (_, mcp, code)] = typed_filter_transports("owner: &str, query: ListWorkoutQuery, tag: Option<u32>");
    assert!(mcp.contains(&read(r#"args_without(ontogen_args, &["owner", "tag", "limit", "offset"])"#)), "{code}");

    // Paged and scoped, beside a bare filter: the scope's argument too.
    let [_, (_, mcp, code)] = filter_transports("query: ListWorkoutQuery, owner: &str", test_config_with_prefix);
    assert!(
        mcp.contains(&read(r#"args_without(ontogen_args, &["owner", "limit", "offset", "project_id"])"#)),
        "{code}"
    );
    assert!(
        mcp.contains(&compact(
            "refuse_unknown_args(ontogen_args, \
             with_pagination_schema(with_project_id_schema(schema_for::<OntogenWorkoutListFilter>())))?;"
        )),
        "{code}"
    );

    // Neither page nor scope nor bare filter: the arguments as they are.
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "workout.rs",
        &paged_crud_module_source("workout", "Store")
            .replace("store: &Store, limit: Option<u64>, offset: Option<u64>", "store: &Store, query: ListWorkoutQuery")
            .replace("count(store: &Store)", "count(store: &Store, query: ListWorkoutQuery)"),
    );
    let config = test_config(api_dir);
    let modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let out = tmp.path().join("mcp.rs");
    crate::servers::generators::mcp::generate(&out, &modules, &config);
    let code = std::fs::read_to_string(&out).unwrap();
    assert!(compact(&code).contains(&read("ontogen_args.clone()")), "{code}");
    assert!(compact(&code).contains(&compact("schema_fn: schema_for::<ListWorkoutQuery>,")), "{code}");
}

/// A `count` that ignores the filter would report the whole table as the total
/// of a filtered page, so the two parameter lists must agree.
#[test]
fn a_filtered_page_whose_count_does_not_filter_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "workout.rs",
        &paged_crud_module_source("workout", "Store")
            .replace("store: &Store, limit", "store: &Store, plan_id: &str, limit"),
    );
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let err = crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces)
        .unwrap_err();
    assert!(err.contains("must take the same filter"), "{err}");
    assert!(err.contains("`list` filters by plan_id: &str, `count` by nothing"), "{err}");
}

#[test]
fn a_count_on_an_unpaginated_module_stays_an_operation() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "workout.rs", &paged_crud_module_source("workout", "Store"));
    let config = test_config(api_dir);

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();
    let workout = modules.iter().find(|m| m.name == "workout").unwrap();
    assert!(
        workout.functions.iter().any(|f| f.name == "count"),
        "nothing pages, so `count` is the consumer's own endpoint"
    );

    let http = tmp.path().join("http.rs");
    crate::servers::generators::http::generate(&http, &modules, &config);
    let http = std::fs::read_to_string(&http).unwrap();
    assert!(http.contains("workout::count(&ontogen_store)"), "`count` gets a handler:\n{http}");
}

#[test]
fn an_unpaginated_surface_hands_a_page_taking_list_no_page() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "workout.rs", &paged_crud_module_source("workout", "Store"));
    let config = test_config(api_dir);

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();

    let http = tmp.path().join("http.rs");
    crate::servers::generators::http::generate(&http, &modules, &config);
    let http = std::fs::read_to_string(&http).unwrap();
    assert!(http.contains("workout::list(&ontogen_store, None, None)"), "the whole table, as before:\n{http}");
    assert!(!http.contains("Query(limit)"), "limit is not a filter:\n{http}");

    let ipc = tmp.path().join("ipc.rs");
    crate::servers::generators::ipc::generate(&ipc, &modules, &config);
    let ipc = std::fs::read_to_string(&ipc).unwrap();
    assert!(ipc.contains("workout::list(&ontogen_store, None, None)"), "the whole table, as before:\n{ipc}");
    assert!(!ipc.contains("limit: Option<u64>"), "limit is not a command param:\n{ipc}");
}

/// `count` is only folded into the page handler on a module some surface
/// paginates. In a module that does not paginate it is an ordinary command,
/// and dropping it there would lose a real operation without a word.
#[test]
fn a_count_beside_an_unpaged_list_stays_a_command() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    let source = format!(
        "{}pub async fn count(state: &AppState) -> Result<u64, anyhow::Error> {{ todo!() }}\n",
        crud_module_source("workout", "AppState")
    );
    write_synthetic_api(&api_dir, "workout.rs", &source);
    let config = test_config(api_dir);

    let scanned = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap();
    assert!(scanned.skips.is_empty(), "nothing was dropped: {:?}", scanned.skips);
    let mut modules = scanned.modules;
    crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces).unwrap();
    let workout = modules.iter().find(|m| m.name == "workout").unwrap();
    assert!(workout.functions.iter().any(|f| f.name == "count"), "`count` is kept as an operation");
}

/// A stateless `count()` declares no argument for the generators to pass, so
/// it is never taken for the companion: it stays an operation and a paginated
/// module without a scoped `count` is refused rather than generating a call
/// that does not compile.
#[test]
fn a_stateless_count_is_not_the_pages_companion() {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    let source = "pub async fn list(state: &AppState, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Workout>, anyhow::Error> { todo!() }
#[ontogen::stateless]
pub async fn count() -> Result<u64, anyhow::Error> { todo!() }
";
    write_synthetic_api(&api_dir, "workout.rs", source);
    let mut config = test_config(api_dir);
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let mut modules = crate::servers::parse::scan_surfaces(&config.surfaces(), &config.state_type).unwrap().modules;
    let workout = modules.iter().find(|m| m.name == "workout").unwrap();
    assert!(!workout.has_count, "a stateless `count()` is not recorded as the companion");
    assert!(workout.functions.iter().any(|f| f.name == "count" && f.is_stateless), "it stays an operation");
    let err = crate::servers::parse::check_paginated_lists(&mut modules, &config.pagination, &config.extra_surfaces)
        .unwrap_err();
    assert!(err.contains("module `workout` is paginated"), "{err}");
}

#[test]
fn test_ts_transport_keeps_on_x_only_for_legacy_events() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("transport.ts");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type LoggedChange = { seq: number; };\n").unwrap();
    let config = client_test_config(tmp.path().to_path_buf());
    let modules = vec![make_event_module(), make_param_event_module()];
    crate::clients::generators::transport::generate(&output, &bindings, &modules, &config);
    let content = std::fs::read_to_string(&output).unwrap();

    assert!(content.contains("onGraphUpdated(callback: (payload: unknown) => void)"));
    assert!(content.contains("import { listen } from '@tauri-apps/api/event';"));
    assert!(!content.contains("onVaultNoteChanges"), "a parameterized op has no global onX");
}
/// The generated transport the admin-layer vitest suite drives
/// (`packages/nuxt_admin_layer/tests/event-subscriptions.test.ts`). It must
/// match the generator: rerun with `UPDATE_TS_FIXTURES=1` after an intended
/// change and commit the result.
#[test]
fn ts_event_transport_fixture_is_current() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packages/nuxt_admin_layer/tests/fixtures");
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("event-transport.generated.ts");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::copy(fixture_dir.join("bindings.ts"), &bindings).unwrap();
    let config = client_test_config(tmp.path().to_path_buf());
    crate::clients::generators::transport::generate(
        &output,
        &bindings,
        &[make_event_module(), make_param_event_module()],
        &config,
    );
    let fresh = std::fs::read_to_string(&output).unwrap();
    let committed_path = fixture_dir.join("event-transport.generated.ts");
    if std::env::var_os("UPDATE_TS_FIXTURES").is_some() {
        std::fs::write(&committed_path, &fresh).unwrap();
    }
    let committed = std::fs::read_to_string(&committed_path).unwrap_or_default();
    assert_eq!(committed, fresh, "stale fixture: rerun with UPDATE_TS_FIXTURES=1");
}

#[test]
fn test_ts_transport_event_subscriptions() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("transport.ts");
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type LoggedChange = { seq: number; };\n").unwrap();
    let config = client_test_config(tmp.path().to_path_buf());
    crate::clients::generators::transport::generate(&output, &bindings, &[make_param_event_module()], &config);
    let content = std::fs::read_to_string(&output).unwrap();

    let sig = "subscribeVaultNoteChanges(args: { vaultId: string; classes?: string | null; resume?: string | null }, \
               handlers: SubscriptionHandlers<LoggedChange>): Promise<() => void>";
    assert_eq!(content.matches(sig).count(), 3, "interface, HTTP and IPC share one signature");
    assert!(content.contains("import { Channel as IpcChannel, invoke } from '@tauri-apps/api/core';"));
    assert!(content.contains("toQueryString({ classes: args.classes, resume: resume })"));
    assert!(content.contains("'vault-note-changes',\n        args.resume ?? null,"));
    assert!(content.contains("subscribeIpc('vault_note_changes_subscribe', 'vault_note_changes_unsubscribe'"));
    assert!(!content.contains("import { listen }"), "no legacy op, no global listener");
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON:API resources (wire contract §5–§8, §13)
// ═══════════════════════════════════════════════════════════════════════════════

const RESOURCE_SCHEMA: &str = r#"
    #[derive(OntologyEntity)]
    #[ontology(entity)]
    pub struct Epic {
        #[ontology(id)]
        pub id: String,
        pub title: String,
    }

    #[derive(OntologyEntity)]
    #[ontology(entity)]
    pub struct Tag {
        #[ontology(id)]
        pub id: String,
        pub title: String,
    }

    #[derive(OntologyEntity)]
    #[ontology(entity)]
    pub struct Task {
        #[ontology(id)]
        pub id: String,
        pub title: String,
        pub notes: Option<String>,
        #[serde(default)]
        pub done: bool,
        #[ontology(relation(belongs_to, target = "Epic"))]
        pub epic_id: Option<String>,
        #[ontology(relation(many_to_many, target = "Tag"))]
        pub tags: Vec<String>,
        #[ontology(body)]
        pub body: String,
    }
"#;

const RESOURCE_APP_ERROR: &str = "
pub enum AppError {
    TaskNotFound(String),
    TaskIdRequired(String),
    TaskAlreadyExists(String),
    EpicNotFound(String),
    TagNotFound(String),
    DbError(String),
}
";

/// The paged CRUD module `gen_api` emits for `entity`, returning `AppError`.
pub(crate) fn app_error_crud_source(entity: &str) -> String {
    paged_crud_module_source(entity, "Store").replace("anyhow::Error", "AppError")
}

/// [`app_error_crud_source`] whose `list` takes the order the generated
/// CRUD `list` takes, `&[OrderBy<{Entity}SortField>]`, before its page.
pub(crate) fn sorted_crud_source(entity: &str) -> String {
    app_error_crud_source(entity).replace(
        "list(store: &Store, limit",
        &format!("list(store: &Store, order: &[OrderBy<{}SortField>], limit", capitalize(entity)),
    )
}

/// A config serving `task`, `epic` and `tag` as resources, each with the
/// sorted CRUD module `gen_api` emits ([`sorted_crud_source`]), plus
/// `report`, a module with CRUD-named fns and no entity behind it, whose
/// `list` takes no order. Every module paginates. With `app_error`, the
/// schema directory declares `AppError`.
pub(crate) fn resource_fixture(root: &std::path::Path, app_error: bool) -> Config {
    let api_dir = root.join("api");
    for entity in ["task", "epic", "tag"] {
        write_synthetic_api(&api_dir, &format!("{entity}.rs"), &sorted_crud_source(entity));
    }
    write_synthetic_api(&api_dir, "report.rs", &app_error_crud_source("report"));
    let schema_dir = root.join("schema");
    write_synthetic_api(&schema_dir, "mod.rs", if app_error { RESOURCE_APP_ERROR } else { "" });

    let entities =
        crate::schema::parse::parse_schema_source(RESOURCE_SCHEMA, std::path::Path::new("schema.rs")).unwrap();
    let mut config = test_config(api_dir);
    config.resources = crate::resource::ResourceModel::build(&entities, &config.naming);
    config.error_map = crate::servers::error_map::scan(&schema_dir).unwrap();
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });
    config
}

/// Custom ops, a module with no entity, junction ops, filtered lists and
/// event ops beside the resources of [`resource_fixture`]: every kind of op
/// served as a custom op (§10), a list's filter (§7.3, §10.4), and both
/// event frame shapes (§12).
///
/// `task` gains junction ops over `tag`, which define its relationship
/// `labels` (§9.1), and a lone `list_overdue`, a custom GET; `workout`, which
/// has no entity, has junction ops of its own, served as custom ops
/// (§10.4); `epic`'s list takes a
/// `ListEpicsQuery` struct and two bare filters, `title` (optional) and
/// `owner` (required), declared out of byte order, then an order; `tag`'s
/// list is hand-written without an order, so it refuses `sort`; `agent`, which has no
/// entity, has a list that takes an `AgentQuery` struct and an optional
/// owned `skill_id`; `workout` has a custom GET with path and `opArg` arguments, custom
/// POSTs with and without arguments, and a stateless GET; `activity` has an
/// event yielding the `Task` entity, a fallible resumable one yielding
/// `Activity`, and one failing with a `String`. With `scoped`, a
/// `route_prefix` serves every store-scoped op under `projects/{project_id}`.
pub(crate) fn ops_fixture(root: &std::path::Path, scoped: bool) -> Config {
    let mut config = resource_fixture(root, true);
    let api_dir = root.join("api");
    let task = std::fs::read_to_string(api_dir.join("task.rs")).unwrap()
        + "pub async fn list_labels(store: &Store, task_id: &str) -> Result<Vec<Tag>, AppError> { todo!() }\n\
           pub async fn add_label(store: &Store, task_id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }\n\
           pub async fn remove_label(store: &Store, task_id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }\n\
           pub async fn list_overdue(store: &Store, before: &str) -> Result<Vec<Task>, AppError> { todo!() }\n";
    write_synthetic_api(&api_dir, "task.rs", &task);
    let epic_filter = "query: ListEpicsQuery, title: Option<&str>, owner: &str";
    write_synthetic_api(
        &api_dir,
        "epic.rs",
        &app_error_crud_source("epic")
            .replace(
                "list(store: &Store, limit",
                &format!("list(store: &Store, {epic_filter}, order: &[OrderBy<EpicSortField>], limit"),
            )
            .replace("count(store: &Store)", &format!("count(store: &Store, {epic_filter})")),
    );
    write_synthetic_api(&api_dir, "tag.rs", &app_error_crud_source("tag"));
    write_synthetic_api(
        &api_dir,
        "agent.rs",
        "pub async fn list(store: &Store, query: AgentQuery, skill_id: Option<String>, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Agent>, AppError> { todo!() }\n\
         pub async fn count(store: &Store, query: AgentQuery, skill_id: Option<String>) -> Result<u64, AppError> { todo!() }\n",
    );
    write_synthetic_api(
        &api_dir,
        "workout.rs",
        "/// A workout's summary.\n\
         pub async fn get_summary(store: &Store, id: &str, verbose: Option<bool>, label: Option<&str>) -> Result<WorkoutSummary, AppError> { todo!() }\n\
         pub async fn start(store: &Store, input: StartWorkoutInput, note: Option<String>) -> Result<WorkoutSummary, anyhow::Error> { todo!() }\n\
         pub async fn rename(store: &Store, id: &str, name: String) -> Result<(), AppError> { todo!() }\n\
         pub async fn pause(store: &Store) -> Result<(), AppError> { todo!() }\n\
         pub async fn list_labels(store: &Store, workout_id: &str) -> Result<Vec<String>, AppError> { todo!() }\n\
         pub async fn add_label(store: &Store, workout_id: &str, label: &str) -> Result<(), AppError> { todo!() }\n\
         pub async fn remove_label(store: &Store, workout_id: &str, label: &str) -> Result<(), AppError> { todo!() }\n\
         #[ontogen::stateless]\n\
         pub fn get_version() -> Result<String, anyhow::Error> { todo!() }\n",
    );
    write_synthetic_api(
        &api_dir,
        "activity.rs",
        "pub fn task_changed(state: &AppState) -> tokio::sync::broadcast::Receiver<Task> { todo!() }\n\
         pub async fn activity_for_kind(state: &AppState, kind: String, resume: Option<String>) -> Result<tokio::sync::broadcast::Receiver<Activity>, AppError> { todo!() }\n\
         pub fn log_lines(state: &AppState, level: Option<u32>) -> Result<tokio::sync::broadcast::Receiver<String>, String> { todo!() }\n",
    );
    if scoped {
        config.route_prefix = test_config_with_prefix(PathBuf::new()).route_prefix;
    }
    config
}

/// Run `generate_transport` with only the HTTP generator and read its output.
pub(crate) fn generate_http(root: &std::path::Path, mut config: Config) -> String {
    let output = root.join("http.rs");
    config.generators = vec![ServerGenerator::HttpAxum { output: output.clone() }];
    crate::servers::generate_transport(&config).expect("generate_transport failed");
    std::fs::read_to_string(output).unwrap()
}

#[test]
fn a_resource_module_is_served_as_jsonapi() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), resource_fixture(tmp.path(), true));
    let flat = compact(&http);

    for route in [
        ".route(\"/api/tasks\", axum::routing::get(task_list).post(task_create).fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::POST])))",
        ".route(\"/api/tasks/{id}\", axum::routing::get(task_get_by_id).patch(task_update).delete(task_delete)\
         .fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::PATCH, OntogenMethod::DELETE])))",
    ] {
        assert!(flat.contains(&compact(route)), "expected {route} in:\n{http}");
    }
    assert!(!flat.contains("put(task_update)"), "PATCH replaces PUT (§8.3):\n{http}");

    // Checks in §13.2 order: `Accept`, the media type `Body` checks, then
    // the path and the query, which a handler reading a body defers.
    for (handler, order) in [
        (
            "task_create",
            &[
                "_: OntogenAcceptGuard",
                "query: Result<OntogenQuery<OntogenNoParams>, OntogenErrorObject>",
                "body: OntogenBody",
                ") -> Result<OntogenResponse, OntogenErrorObject> {",
                "query?;",
                "let body = body.into_bytes()?;",
                "ontogen_jsonapi::request::parse_create(&body,",
            ][..],
        ),
        (
            "task_update",
            &[
                "_: OntogenAcceptGuard",
                "path_params: Result<OntogenPath<OntogenLookupKey>, OntogenErrorObject>",
                "query: Result<OntogenQuery<OntogenNoParams>, OntogenErrorObject>",
                "body: OntogenBody",
                ") -> Result<OntogenResponse, OntogenErrorObject> {",
                "let OntogenPath(id) = path_params?;",
                "query?;",
                "let body = body.into_bytes()?;",
                "ontogen_jsonapi::request::parse_update(&body,",
            ][..],
        ),
    ] {
        let code = &http[http.find(&format!("async fn {handler}(")).unwrap()..];
        let code = &code[..code.find("\n}\n").unwrap()];
        let at: Vec<usize> = order.iter().map(|x| code.find(x).unwrap_or_else(|| panic!("{x} in {code}"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "checks in order:\n{code}");
    }
    // A handler with no body extracts its path directly.
    assert!(http.contains("OntogenPath(id): OntogenPath<OntogenLookupKey>,\n    _: OntogenQuery<OntogenNoParams>,\n) -> Result<OntogenResponse, OntogenErrorObject>"));

    // Documents and links come from the runtime crate. An unfiltered list
    // accepts no `filter[…]`, so its links repeat only `include`.
    assert!(flat.contains(&compact("let link_query = query.link_query(include.as_deref())?;")));
    assert!(flat.contains(&compact(
        "let links = ontogen_jsonapi::links::pagination_links(collection, &link_query, offset, limit, total);"
    )));
    assert!(flat.contains(&compact("with_meta(OntogenPageMeta { total, limit, offset })")));
    assert!(flat.contains(&compact("let (offset, limit) = ontogen_page(&query, 20, 100)?;")));
    assert!(
        flat.contains(&compact("task::list(&ontogen_store, &order, Some(u64::from(limit)), Some(u64::from(offset)))"))
    );
    // `Location` is the created resource's `links.self` (§8.2).
    assert!(flat.contains(&compact(
        "let document = OntogenDocument::resource(ontogen_task_as_resource(&entity, collection), &OntogenCanonicalQuery::new()); \
         let location = document.data().and_then(OntogenResourceObject::links).map(OntogenLinks::self_link); \
         Ok(ontogen_jsonapi::response::created(location, &document))"
    )));
    assert!(flat.contains(&compact("Ok(ontogen_jsonapi::response::no_content())")));
    assert!(flat.contains(&compact("ontogen_core::id::validate_id(id).map_err(|e| e.reason)")));

    // Attributes split from relationships (§5.3, §5.4), each relationship
    // with the links of the routes that serve it (§9).
    assert!(http.contains("attributes.serialize_field(\"title\", &self.0.title)?;"));
    assert!(!http.contains("serialize_field(\"epic_id\""), "a relation field is not an attribute:\n{http}");
    assert!(flat.contains(&compact(
        ".with_relationship(\"epic\", OntogenRelationship::new(OntogenLinks::new(format!(\"{self_link}/relationships/epic\"))\
         .with_related(format!(\"{self_link}/epic\")), OntogenLinkage::ToOne(entity.epic_id.as_ref()\
         .map(|id| OntogenResourceIdentifier::new(\"epics\", id.as_str())))))"
    )));
    assert!(!http.contains("OntogenRelationship::from_data("), "every relationship carries links:\n{http}");

    // Requiredness follows `CreateTaskInput`: `title` and `body` have no
    // default, `notes` is an `Option` and `done` carries `#[serde(default)]`.
    for (attribute, ty, required) in
        [("title", "String", "create"), ("notes", "Option<String>", "false"), ("done", "bool", "false")]
    {
        let line = format!("ontogen_jsonapi::request::attribute::<{ty}>(attributes, \"{attribute}\", {required})?");
        assert!(flat.contains(&compact(&line)), "expected {line} in:\n{http}");
    }
    assert!(flat.contains(&compact(
        "ontogen_jsonapi::request::check_attribute_names(attributes, \"tasks\", &[\"title\", \"notes\", \"done\", \"body\"], \
         &[(\"epic_id\", \"epic\"), (\"tags\", \"tags\")])?;"
    )));

    // Step 8: each linked id is looked up with its target's `get_by_id`, in
    // one helper both writes call.
    assert_eq!(http.matches("ontogen_task_check_linked(&ontogen_state, &linked).await?;").count(), 2, "{http}");
    assert!(flat.contains(&compact(
        "if let Some(linked) = &linked.epic { match epic::get_by_id(&store, &linked.id).await { Ok(_) => {} \
         Err(crate::schema::AppError::EpicNotFound(..)) => return Err(linked.not_found(\"epics\")), \
         Err(e) => return Err(ontogen_app_error(e)), } }"
    )));
    assert!(flat.contains(&compact("for linked in &linked.tags {")));

    // A duplicate client id points at it (§8.2).
    assert!(flat.contains(&compact("e @ crate::schema::AppError::TaskAlreadyExists(..) if data.id.is_some() =>")));
    assert!(flat.contains(&compact("ontogen_app_error(e).with_pointer(\"/data/id\")")));
    assert!(!http.contains("ErrorResponse") && !http.contains("fn err("), "no `{{error}}` bodies:\n{http}");
}

#[test]
fn the_app_error_scan_becomes_one_mapping_fn() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), resource_fixture(tmp.path(), true));
    let flat = compact(&http);

    assert!(http.contains("fn ontogen_app_error(e: crate::schema::AppError) -> OntogenErrorObject {"));
    for arm in [
        "crate::schema::AppError::TaskNotFound(..) => (OntogenStatusCode::NOT_FOUND, \"task_not_found\"),",
        "crate::schema::AppError::TaskIdRequired(..) => (OntogenStatusCode::BAD_REQUEST, \"task_id_required\"),",
        "crate::schema::AppError::TaskAlreadyExists(..) => (OntogenStatusCode::CONFLICT, \"task_already_exists\"),",
        "crate::schema::AppError::DbError(..) => (OntogenStatusCode::INTERNAL_SERVER_ERROR, \"db_error\"),",
    ] {
        assert!(flat.contains(&compact(arm)), "expected {arm} in:\n{http}");
    }
    assert!(http.contains("OntogenErrorObject::app(status, code, e.to_string())"), "detail is the Display text");
    // AppError-typed calls map through it; store construction does not.
    assert!(flat.contains(&compact(
        "task::get_by_id(&ontogen_store, ontogen_task_lookup_key(&id)?).await.map_err(ontogen_app_error)?;"
    )));
    assert!(http.contains("let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;"));
    // An `{id}` that does not decode is the entity's own 404.
    assert!(flat.contains(&compact(
        "id.as_str().ok_or_else(|| ontogen_app_error(crate::schema::AppError::TaskNotFound(id.to_string())))"
    )));
}

#[test]
fn without_an_app_error_every_app_error_site_is_a_500() {
    let tmp = tempfile::tempdir().unwrap();
    let config = resource_fixture(tmp.path(), false);
    assert!(config.error_map.is_none());
    let http = generate_http(tmp.path(), config);

    assert!(http.contains(
        "fn ontogen_app_error(e: impl std::fmt::Display) -> OntogenErrorObject {\n    OntogenErrorObject::internal("
    ));
    assert!(!http.contains("AppError::"), "no variant is matched:\n{http}");
    assert!(!http.contains(".not_found(\"epics\")"), "a missing link cannot be told from a failure:\n{http}");
}

/// A module with no entity serves its CRUD-named ops as custom ops (§10.4):
/// meta-only documents, `meta.args` bodies, `opArg` pages and no `PUT`.
#[test]
fn a_module_with_no_entity_serves_its_crud_ops_as_custom_ops() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), resource_fixture(tmp.path(), true));
    let flat = compact(&http);

    assert!(
        flat.contains(&compact(
            ".route(\"/api/reports/{id}\", axum::routing::get(report_get_by_id).patch(report_update).delete(report_delete)\
             .fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::PATCH, OntogenMethod::DELETE])))"
        )),
        "update is PATCH:\n{http}"
    );
    assert!(!http.contains("put("), "no route is PUT:\n{http}");
    let handler = |name: &str| {
        let code = &http[http.find(&format!("async fn {name}(")).unwrap_or_else(|| panic!("{name}:\n{http}"))..];
        compact(&code[..code.find("\n}\n").unwrap()])
    };

    // `list`: the page from `opArg[limit]` then `opArg[offset]`, into the
    // store's page-taking list and `count`, answered as `meta.result`.
    let list = handler("report_list");
    for step in [
        "_: OntogenAcceptGuard, ontogen_query: OntogenQuery<OntogenPageOpArgs>",
        "let ontogen_limit = ontogen_query.page_op_arg(\"limit\")?.unwrap_or(20).min(100);",
        "let ontogen_offset = ontogen_query.page_op_arg(\"offset\")?.unwrap_or(0);",
        "report::list(&ontogen_store, Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset))).await.map_err(ontogen_app_error)?;",
        "let ontogen_total = report::count(&ontogen_store).await.map_err(ontogen_app_error)?;",
        "let ontogen_result = OntogenPaginatedResult { items: ontogen_items, total: ontogen_total, limit: ontogen_limit, offset: ontogen_offset, };",
        "Ok(ontogen_jsonapi::response::ok(&OntogenDocument::meta_only(OntogenResultMeta { result: ontogen_result })))",
    ] {
        assert!(list.contains(&compact(step)), "report_list: {step}\n{list}");
    }
    assert!(http.contains("op_args: &[\"limit\", \"offset\"]"), "only the page is accepted:\n{http}");

    // `create` and `update`: `meta.args.input`, `200` with `meta.result`.
    let create = handler("report_create");
    assert!(
        create.contains(&compact("let ontogen_args = ontogen_jsonapi::request::op_args(&ontogen_bytes, true)?;")),
        "{create}"
    );
    assert!(
        create.contains(&compact(
            "let input = ontogen_jsonapi::request::op_arg::<CreateReportInput>(&ontogen_args, \"input\", true)?;"
        )),
        "{create}"
    );
    assert!(create.contains(&compact(
        "Ok(ontogen_jsonapi::response::ok(&OntogenDocument::meta_only(OntogenResultMeta { result: ontogen_result })))"
    )));
    assert!(!create.contains("created("), "never 201:\n{create}");
    let update = handler("report_update");
    for step in [
        "ontogen_path: Result<OntogenPath<String>, OntogenErrorObject>, ontogen_query: Result<OntogenQuery<OntogenNoParams>, OntogenErrorObject>, \
         ontogen_body: OntogenBody",
        "let OntogenPath(id) = ontogen_path?; ontogen_query?;",
        "ontogen_jsonapi::request::check_op_arg_names(&ontogen_args, &[\"input\"])?;",
        "report::update(&ontogen_store, &id, input).await.map_err(ontogen_app_error)?;",
    ] {
        assert!(update.contains(&compact(step)), "report_update: {step}\n{update}");
    }

    // `get_by_id` and `delete`: the id from the path, no body; delete is 204.
    assert!(
        handler("report_get_by_id")
            .contains(&compact("OntogenPath(id): OntogenPath<String>, _: OntogenQuery<OntogenNoParams>"))
    );
    let delete = handler("report_delete");
    assert!(delete.contains(&compact(
        "report::delete(&ontogen_store, &id).await.map_err(ontogen_app_error)?; Ok(ontogen_jsonapi::response::no_content())"
    )));
    assert!(!delete.contains("OntogenBody"), "delete ignores a body:\n{delete}");

    assert!(!http.contains("ontogen_report_as_resource"), "no resource helpers for it:\n{http}");
    assert!(!http.contains("Json"), "no flat bodies remain:\n{http}");
}

#[test]
fn scoped_resource_routes_carry_the_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    config.route_prefix = test_config_with_prefix(PathBuf::new()).route_prefix;
    let http = generate_http(tmp.path(), config);
    let flat = compact(&http);

    assert!(flat.contains(&compact(
        ".route(\"/api/projects/{project_id}/tasks/{id}\", axum::routing::get(task_get_by_id_scoped)\
         .patch(task_update_scoped).delete(task_delete_scoped)\
         .fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::PATCH, OntogenMethod::DELETE])))"
    )));
    assert!(http.contains("OntogenPath((ontogen_scope, id)): OntogenPath<(uuid::Uuid, OntogenLookupKey)>,"));
    assert!(http.contains("OntogenPath(ontogen_scope): OntogenPath<uuid::Uuid>,"));
    // A handler reading a body checks its prefix after the media type.
    assert!(http.contains("path_params: Result<OntogenPath<uuid::Uuid>, OntogenErrorObject>,"));
    assert!(http.contains("let OntogenPath(ontogen_scope) = path_params?;"));
    assert!(http.contains("path_params: Result<OntogenPath<(uuid::Uuid, OntogenLookupKey)>, OntogenErrorObject>,"));
    assert!(http.contains("let OntogenPath((ontogen_scope, id)) = path_params?;"));
    // The linked-resource checks open the scoped store, once per resource.
    assert!(flat.contains(&compact(
        "async fn ontogen_task_check_linked_scoped(state: &AppState, ontogen_scope: &uuid::Uuid, linked: &OntogenTaskLinkedIds) \
         -> Result<(), OntogenErrorObject> { let store = state.store_for(ontogen_scope).map_err(ontogen_internal_error)?;"
    )));
    assert_eq!(
        http.matches("ontogen_task_check_linked_scoped(&ontogen_state, &ontogen_scope, &linked).await?;").count(),
        2,
        "{http}"
    );
    assert!(!http.contains("async fn ontogen_task_check_linked("), "no unscoped handler links:\n{http}");
    assert!(flat.contains(&compact(
        "let collection = &format!(\"/api/projects/{}/tasks\", ontogen_jsonapi::links::encode_path_segment(&ontogen_scope.to_string()));"
    )));
    assert!(
        http.contains("let ontogen_store = ontogen_state.store_for(&ontogen_scope).map_err(ontogen_internal_error)?;")
    );
    // The scoped list pages through the store like the unscoped one.
    assert!(flat.contains(&compact("let total = task::count(&ontogen_store).await.map_err(ontogen_app_error)?;")));
    assert!(!flat.contains(&compact(".route(\"/api/tasks\"")), "store-scoped CRUD has no unscoped route:\n{http}");
}

/// Every name a pattern binds.
fn pattern_idents(pat: &syn::Pat, out: &mut Vec<String>) {
    match pat {
        syn::Pat::Ident(i) => out.push(i.ident.to_string()),
        syn::Pat::Tuple(t) => t.elems.iter().for_each(|p| pattern_idents(p, out)),
        syn::Pat::TupleStruct(t) => t.elems.iter().for_each(|p| pattern_idents(p, out)),
        syn::Pat::Paren(p) => pattern_idents(&p.pat, out),
        syn::Pat::Type(t) => pattern_idents(&t.pat, out),
        _ => {}
    }
}

/// A prefix param may share a name with any binding of a generated handler:
/// the handlers bind its value as `ontogen_scope`, and the name stays in the
/// route only.
#[test]
fn a_prefix_param_may_be_named_like_a_handler_binding() {
    for name in ["query", "path_params"] {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = resource_fixture(tmp.path(), true);
        config.route_prefix = Some(RoutePrefix {
            segments: format!("projects/:{name}"),
            state_accessor: "store_for".to_string(),
            params: vec![PrefixParam {
                name: name.to_string(),
                rust_type: "uuid::Uuid".to_string(),
                ts_type: "string".to_string(),
            }],
        });
        let http = generate_http(tmp.path(), config);
        let flat = compact(&http);

        assert!(flat.contains(&compact(&format!(".route(\"/api/projects/{{{name}}}/tasks/{{id}}\""))), "{http}");
        for handler in ["task_create_scoped", "task_update_scoped"] {
            let body =
                &http[http.find(&format!("async fn {handler}(")).unwrap_or_else(|| panic!("{handler}:\n{http}"))..];
            let body = &body[..body.find("\n}\n").unwrap()];
            let handler_fn = syn::parse_str::<syn::ItemFn>(&format!("{body}\n}}")).expect("the handler parses");
            let mut bound = Vec::new();
            for arg in &handler_fn.sig.inputs {
                if let syn::FnArg::Typed(t) = arg {
                    pattern_idents(&t.pat, &mut bound);
                }
            }
            for stmt in &handler_fn.block.stmts {
                if let syn::Stmt::Local(local) = stmt
                    && let syn::Pat::TupleStruct(_) = &local.pat
                {
                    pattern_idents(&local.pat, &mut bound);
                }
            }
            let mut unique = bound.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), bound.len(), "{handler} binds a name twice: {bound:?}\n{body}");
            assert!(bound.contains(&"ontogen_scope".to_string()), "{handler}: {bound:?}");
            assert!(body.contains("query?;"), "{handler} still answers its query check:\n{body}");
        }
        assert!(http.contains("let OntogenPath(ontogen_scope) = path_params?;"), "{http}");
        assert!(http.contains("let OntogenPath((ontogen_scope, id)) = path_params?;"), "{http}");
        assert!(
            http.contains(
                "let ontogen_store = ontogen_state.store_for(&ontogen_scope).map_err(ontogen_internal_error)?;"
            ),
            "{http}"
        );
        assert!(
            flat.contains(&compact(
                "let collection = &format!(\"/api/projects/{}/tasks\", ontogen_jsonapi::links::encode_path_segment(&ontogen_scope.to_string()));"
            )),
            "{http}"
        );
        // The list and get handlers read the prefix beside their own `query`.
        assert!(
            flat.contains(&compact(
                "OntogenPath(ontogen_scope): OntogenPath<uuid::Uuid>, query: OntogenQuery<OntogenPagedListParams>"
            )),
            "{http}"
        );
    }
}

/// The names `handler` binds in `http`: its extractor patterns and its
/// `let`s. A `let` that unwraps an extractor into its own name
/// (`let Query(x) = x.map_err(…)?;`) rebinds it, so it is not counted again.
fn handler_bindings(http: &str, handler: &str) -> Vec<String> {
    let body = &http[http.find(&format!("async fn {handler}(")).unwrap_or_else(|| panic!("{handler}:\n{http}"))..];
    let body = &body[..body.find("\n}\n").unwrap()];
    let handler_fn = syn::parse_str::<syn::ItemFn>(&format!("{body}\n}}")).expect("the handler parses");
    let mut bound = Vec::new();
    for arg in &handler_fn.sig.inputs {
        if let syn::FnArg::Typed(t) = arg {
            pattern_idents(&t.pat, &mut bound);
        }
    }
    for stmt in &handler_fn.block.stmts {
        if let syn::Stmt::Local(local) = stmt {
            let mut names = Vec::new();
            pattern_idents(&local.pat, &mut names);
            let unwrapped = local.init.as_ref().and_then(|init| match &*init.expr {
                syn::Expr::Try(t) => match &*t.expr {
                    syn::Expr::MethodCall(call) => match &*call.receiver {
                        syn::Expr::Path(p) => p.path.get_ident().map(ToString::to_string),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            });
            if names.len() == 1 && unwrapped.as_ref() == names.first() && bound.contains(&names[0]) {
                continue;
            }
            bound.extend(names);
        }
    }
    bound
}

/// Each name `handler` binds is bound once, is an argument of its fn (one of
/// `args`) or carries the `ontogen_` prefix, and names no fn of the generated
/// file: a binding named like a helper the handler calls (`ontogen_app_error`,
/// `ontogen_sse_stream`, …) would shadow it.
fn assert_bindings_shadow_nothing(http: &str, handler: &str, args: &[&str]) {
    let fns: Vec<String> = syn::parse_file(http)
        .expect("the generated file parses")
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(f) => Some(f.sig.ident.to_string()),
            _ => None,
        })
        .collect();
    let bound = handler_bindings(http, handler);
    let mut unique = bound.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), bound.len(), "{handler} binds a name twice: {bound:?}");
    assert!(bound.contains(&"ontogen_state".to_string()), "{handler}: {bound:?}");
    for name in &bound {
        assert!(
            name.starts_with("ontogen_") || args.contains(&name.as_str()),
            "{handler} binds `{name}`, which is no argument of the fn: {bound:?}"
        );
        assert!(!fns.contains(name), "{handler} binds `{name}`, which shadows the generated fn `{name}`");
    }
}

/// A fn's arguments are bound under their own names and every other binding
/// of a handler carries the `ontogen_` prefix, so an argument named like a
/// handler binding (`state`, `store`, `query`, `body`, `id`, …) neither
/// shadows one nor is shadowed. Nor is an argument named like a helper the
/// handler calls as that helper was once named (`app_error`,
/// `internal_error`, `query_rejection`, `sse_stream`, `result_frame`,
/// `{module}_frame_data`): every such helper carries the prefix too. Covers
/// every handler that binds an argument: custom `GET` and `POST`, the
/// CRUD-named ops of a module with no entity, junction ops, a list that
/// takes a filter (whose filters are bound as `ontogen_filter_{name}`), and
/// event streams, unscoped and under a route prefix.
#[test]
fn an_op_argument_may_be_named_like_a_handler_binding() {
    let source = "\
pub async fn get_report(ctx: &Store, state: &str, store: Option<String>, query: Option<bool>, body: Option<u32>, \
  app_error: &str, internal_error: Option<u32>) -> Result<String, AppError> { todo!() }
pub async fn set_state(ctx: &Store, id: String, state: String, store: Option<String>, body: Option<String>, \
  query: String, args: Option<u32>, result: Option<u32>, bytes: Option<u32>, app_error: Option<u32>, \
  internal_error: String) -> Result<String, AppError> { todo!() }
pub async fn list(ctx: &Store, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<String>, AppError> { todo!() }
pub async fn count(ctx: &Store) -> Result<u64, AppError> { todo!() }
pub async fn get_by_id(ctx: &Store, state: &str) -> Result<String, AppError> { todo!() }
pub async fn create(ctx: &Store, store: NewThing) -> Result<String, AppError> { todo!() }
pub async fn update(ctx: &Store, query: &str, body: ThingPatch) -> Result<String, AppError> { todo!() }
pub async fn delete(ctx: &Store, store: &str) -> Result<(), AppError> { todo!() }
pub async fn list_tags(ctx: &Store, state: &str) -> Result<Vec<String>, AppError> { todo!() }
pub async fn add_tag(ctx: &Store, state: &str, store: &str) -> Result<(), AppError> { todo!() }
pub async fn remove_tag(ctx: &Store, app_error: &str, internal_error: &str) -> Result<(), AppError> { todo!() }
";
    let filtered = "\
pub async fn list(ctx: &Store, app_error: &str, query_rejection: &str, limit: Option<u64>, offset: Option<u64>) \
  -> Result<Vec<String>, AppError> { todo!() }
pub async fn count(ctx: &Store, app_error: &str, query_rejection: &str) -> Result<u64, AppError> { todo!() }
";
    let events = "\
pub async fn thing_changes(state: &AppState, app_error: String, internal_error: String, sse_stream: String, \
  result_frame: String, query_rejection: Option<String>) -> Result<tokio::sync::broadcast::Receiver<String>, AppError> \
  { todo!() }
";
    let args = [
        "id",
        "state",
        "store",
        "query",
        "body",
        "args",
        "result",
        "bytes",
        "limit",
        "offset",
        "app_error",
        "internal_error",
        "query_rejection",
        "sse_stream",
        "result_frame",
    ];
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let api_dir = tmp.path().join("api");
        write_synthetic_api(&api_dir, "thing.rs", source);
        write_synthetic_api(&api_dir, "gadget.rs", filtered);
        write_synthetic_api(&api_dir, "feed.rs", events);
        let mut config = test_config(api_dir);
        config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });
        if scoped {
            config.route_prefix = test_config_with_prefix(PathBuf::new()).route_prefix;
        }
        let http = generate_http(tmp.path(), config);
        let suffix = if scoped { "_scoped" } else { "" };
        for op in [
            "thing_get_report",
            "thing_set_state",
            "thing_list",
            "thing_get_by_id",
            "thing_create",
            "thing_update",
            "thing_delete",
            "thing_list_tags",
            "thing_add_tag",
            "thing_remove_tag",
            "gadget_list",
        ] {
            assert_bindings_shadow_nothing(&http, &format!("{op}{suffix}"), &args);
        }
        assert_bindings_shadow_nothing(&http, &format!("thing_changes_sse{suffix}"), &args);
        // Each argument reaches the fn under its own name.
        let flat = compact(&http);
        assert!(
            flat.contains(
                "thing::set_state(&ontogen_store,id,state,store,body,query,args,result,bytes,app_error,\
                 internal_error).await.map_err(ontogen_app_error)?"
            ),
            "{http}"
        );
        assert!(flat.contains("thing::add_tag(&ontogen_store,&state,&store)"), "{http}");
        assert!(flat.contains("thing::remove_tag(&ontogen_store,&app_error,&internal_error)"), "{http}");
        assert!(
            flat.contains("gadget::count(&ontogen_store,&ontogen_filter_app_error,&ontogen_filter_query_rejection)"),
            "a filter is read into a binding of its own:\n{http}"
        );
        assert!(flat.contains("ontogen_sse_stream(\"thing-changes\",ontogen_rx,"), "{http}");
    }

    // An event whose item is a resource writes its frames with that
    // resource's own helper.
    let tmp = tempfile::tempdir().unwrap();
    let config = ops_fixture(tmp.path(), false);
    let activity = config.api_dir.join("activity.rs");
    let mut source = std::fs::read_to_string(&activity).unwrap();
    source.push_str(
        "pub fn task_moves(state: &AppState, task_frame_data: String, app_error: String) -> \
         tokio::sync::broadcast::Receiver<Task> { todo!() }\n",
    );
    std::fs::write(&activity, source).unwrap();
    let http = generate_http(tmp.path(), config);
    assert_bindings_shadow_nothing(&http, "task_moves_sse", &["task_frame_data", "app_error"]);
    assert!(
        compact(&http).contains(&compact(
            "ontogen_sse_stream(\"task-moves\", ontogen_rx, ontogen_core::events::no_id, ontogen_task_frame_data)"
        )),
        "{http}"
    );
}

/// Every Tauri command of `ipc`: its name, its parameters' names, and the
/// names its body binds (each `let`, `if let` and closure pattern).
fn ipc_command_bindings(ipc: &str) -> Vec<(String, Vec<String>, Vec<String>)> {
    struct Bindings(Vec<String>);
    impl<'ast> syn::visit::Visit<'ast> for Bindings {
        fn visit_pat_ident(&mut self, i: &'ast syn::PatIdent) {
            self.0.push(i.ident.to_string());
            syn::visit::visit_pat_ident(self, i);
        }
    }
    syn::parse_file(ipc)
        .unwrap_or_else(|e| panic!("the generated IPC does not parse: {e}\n{ipc}"))
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(f) if f.attrs.iter().any(|a| a.path().segments.iter().any(|s| s.ident == "command")) => {
                let mut params = Vec::new();
                for arg in &f.sig.inputs {
                    if let syn::FnArg::Typed(t) = arg {
                        pattern_idents(&t.pat, &mut params);
                    }
                }
                let mut bindings = Bindings(Vec::new());
                syn::visit::Visit::visit_block(&mut bindings, &f.block);
                Some((f.sig.ident.to_string(), params, bindings.0))
            }
            _ => None,
        })
        .collect()
}

/// A Tauri command names each parameter that carries a fn argument after
/// it, as that name is the IPC wire key, and `ontogen_`-prefixes every
/// binding of its own (`ontogen_state`, `ontogen_store`, `ontogen_pid`,
/// `ontogen_uuid`, `ontogen_e`, `ontogen_limit`, `ontogen_offset`,
/// `ontogen_items`, `ontogen_total`, `ontogen_all`, `ontogen_rx`,
/// `ontogen_frame`). So an argument or a bare filter named like one of them,
/// or like what one was once called (`state`, `store`, `pid`, `uuid`, `e`,
/// `items`, `total`, `all_items`, `rx`, `frame`), takes no parameter name
/// twice and is shadowed by no binding. Covers every command kind: custom
/// `GET` and `POST` over the store and over the state, CRUD, junction ops
/// (paged and not), a list with a `*Query` struct beside bare filters, a list
/// with bare filters only, and event subscriptions, unscoped and under a
/// route prefix, with pagination and without.
#[test]
fn an_ipc_argument_may_be_named_like_a_command_binding() {
    let thing = "\
pub async fn get_report(s: &Store, state: &str, store: Option<String>, query: Option<bool>, ctx: Option<u32>, \
  e: Option<u32>) -> Result<String, AppError> { todo!() }
pub async fn set_state(s: &Store, id: String, state: String, store: Option<String>, query: String, \
  result: Option<u32>, items: Option<u32>, total: Option<u32>, all_items: Option<u32>, pid: Option<String>, \
  uuid: Option<String>, rx: Option<u32>, frame: Option<u32>) -> Result<String, AppError> { todo!() }
pub async fn list(s: &Store, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<String>, AppError> { todo!() }
pub async fn count(s: &Store) -> Result<u64, AppError> { todo!() }
pub async fn get_by_id(s: &Store, state: &str) -> Result<String, AppError> { todo!() }
pub async fn create(s: &Store, store: NewThing) -> Result<String, AppError> { todo!() }
pub async fn update(s: &Store, query: &str, state: ThingPatch) -> Result<String, AppError> { todo!() }
pub async fn delete(s: &Store, store: &str) -> Result<(), AppError> { todo!() }
pub async fn list_tags(s: &Store, state: &str) -> Result<Vec<String>, AppError> { todo!() }
pub async fn add_tag(s: &Store, state: &str, store: &str) -> Result<(), AppError> { todo!() }
pub async fn remove_tag(s: &Store, query: &str, e: &str) -> Result<(), AppError> { todo!() }
";
    // A state-based op, which a route prefix validates rather than scopes.
    let report = "\
pub async fn rename(app: &AppState, state: String, store: Option<String>, pid: Option<String>, uuid: Option<String>, \
  e: Option<u32>) -> Result<String, AppError> { todo!() }
";
    // A `*Query` struct beside bare filters, the page pushed down.
    let gadget = "\
pub async fn list(s: &Store, query: GadgetQuery, state: Option<String>, store: Option<String>, items: Option<u32>, \
  limit: Option<u64>, offset: Option<u64>) -> Result<Vec<String>, AppError> { todo!() }
pub async fn count(s: &Store, query: GadgetQuery, state: Option<String>, store: Option<String>, items: Option<u32>) \
  -> Result<u64, AppError> { todo!() }
";
    // Bare filters only, one of them named `query`.
    let widget = "\
pub async fn list(s: &Store, query: &str, state: Option<String>, store: Option<u32>, e: Option<u32>, \
  total: Option<u32>, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<String>, AppError> { todo!() }
pub async fn count(s: &Store, query: &str, state: Option<String>, store: Option<u32>, e: Option<u32>, \
  total: Option<u32>) -> Result<u64, AppError> { todo!() }
";
    let feed = "\
pub async fn thing_changes(app: &AppState, state: String, store: Option<String>, rx: Option<String>, \
  frame: Option<String>, e: Option<u32>) -> Result<tokio::sync::broadcast::Receiver<String>, AppError> { todo!() }
";
    for (scoped, paginated) in [(false, false), (false, true), (true, false), (true, true)] {
        let tmp = tempfile::tempdir().unwrap();
        let api_dir = tmp.path().join("api");
        for (file, source) in [
            ("thing.rs", thing),
            ("report.rs", report),
            ("gadget.rs", gadget),
            ("widget.rs", widget),
            ("feed.rs", feed),
        ] {
            write_synthetic_api(&api_dir, file, source);
        }
        let mut config = test_config(api_dir);
        if paginated {
            config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });
        }
        if scoped {
            config.route_prefix = test_config_with_prefix(PathBuf::new()).route_prefix;
        }
        let output = tmp.path().join("ipc.rs");
        config.generators = vec![ServerGenerator::TauriIpc { output: output.clone() }];
        crate::servers::generate_transport(&config).expect("generate_transport failed");
        let ipc = std::fs::read_to_string(output).unwrap();
        let case = format!("scoped: {scoped}, paginated: {paginated}");

        let commands = ipc_command_bindings(&ipc);
        let mut seen: Vec<&str> = commands.iter().map(|(name, ..)| name.as_str()).collect();
        seen.sort_unstable();
        let mut expected = vec![
            "thing_get_report",
            "thing_set_state",
            "thing_list",
            "thing_get_by_id",
            "thing_create",
            "thing_update",
            "thing_delete",
            "thing_list_tags",
            "thing_add_tag",
            "thing_remove_tag",
            "report_rename",
            "gadget_list",
            "widget_list",
            "thing_changes_subscribe",
            "thing_changes_unsubscribe",
        ];
        if !paginated {
            // A paginated module's `count` is served by its list's page.
            expected.extend(["thing_count", "gadget_count", "widget_count"]);
        }
        expected.sort_unstable();
        assert_eq!(seen, expected, "{case}");

        for (command, params, bindings) in &commands {
            let mut unique = params.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), params.len(), "{case}: {command} takes a name twice: {params:?}");
            for name in bindings {
                assert!(
                    name.starts_with("ontogen_"),
                    "{case}: {command} binds `{name}`, which is not `ontogen_`-prefixed: {bindings:?}"
                );
                assert!(!params.contains(name), "{case}: {command} binds `{name}`, which shadows its parameter");
            }
        }

        // Each argument keeps its own name, the IPC wire key, and reaches
        // the fn under it.
        let params_of = |command: &str| -> Vec<String> {
            commands.iter().find(|(name, ..)| name == command).map(|(_, p, _)| p.clone()).unwrap()
        };
        let tail = |mut names: Vec<&str>| -> Vec<String> {
            if paginated {
                names.extend(["limit", "offset"]);
            }
            names.into_iter().map(String::from).collect()
        };
        let scope_and_state = |mut names: Vec<String>| -> Vec<String> {
            if scoped {
                names.push("project_id".to_string());
            }
            names.push("ontogen_state".to_string());
            names
        };
        assert_eq!(
            params_of("thing_set_state"),
            scope_and_state(
                [
                    "id",
                    "state",
                    "store",
                    "query",
                    "result",
                    "items",
                    "total",
                    "all_items",
                    "pid",
                    "uuid",
                    "rx",
                    "frame"
                ]
                .map(String::from)
                .to_vec()
            ),
            "{case}"
        );
        assert_eq!(params_of("gadget_list"), scope_and_state(tail(vec!["query", "state", "store", "items"])), "{case}");
        assert_eq!(
            params_of("widget_list"),
            scope_and_state(tail(vec!["query", "state", "store", "e", "total"])),
            "{case}"
        );
        assert_eq!(params_of("thing_list_tags"), scope_and_state(tail(vec!["state"])), "{case}");
        assert_eq!(
            params_of("thing_changes_subscribe"),
            ["state", "store", "rx", "frame", "e", "channel", "ontogen_state"].map(String::from).to_vec(),
            "{case}: an IPC subscription is not scoped"
        );

        let flat = compact(&ipc);
        for call in [
            "thing::set_state(&ontogen_store,id,state,store,query,result,items,total,all_items,pid,uuid,rx,frame)",
            "thing::get_report(&ontogen_store,&state,store,query,ctx,e)",
            "thing::add_tag(&ontogen_store,&state,&store)",
            "thing::remove_tag(&ontogen_store,&query,&e)",
            "report::rename(&ontogen_state,state,store,pid,uuid,e)",
            "feed::thing_changes(&ontogen_state,state,store,rx,frame,e)",
        ] {
            assert!(flat.contains(&compact(call)), "{case}: {call}\n{ipc}");
        }
        let (page, counted) = if paginated {
            (",Some(u64::from(ontogen_limit)),Some(u64::from(ontogen_offset)))", true)
        } else {
            (",None,None)", false)
        };
        let gadget_list = if counted {
            "gadget::list(&ontogen_store,query.clone(),state.clone(),store.clone(),items"
        } else {
            "gadget::list(&ontogen_store,query,state,store,items"
        };
        assert!(flat.contains(&format!("{gadget_list}{page}")), "{case}:\n{ipc}");
        let widget_state = if counted { "state.clone()" } else { "state" };
        assert!(
            flat.contains(&format!("widget::list(&ontogen_store,&query,{widget_state},store,e,total{page}")),
            "{case}:\n{ipc}"
        );
        if paginated {
            assert!(flat.contains("gadget::count(&ontogen_store,query,state,store,items)"), "{case}:\n{ipc}");
            assert!(
                flat.contains(
                    "Ok(PaginatedResult{items:ontogen_items,total:ontogen_total,limit:ontogen_limit,\
                               offset:ontogen_offset})"
                ),
                "{case}:\n{ipc}"
            );
        }
    }
}

/// The error `generate_transport` gives for an IPC generator over `files`,
/// paginated or not.
fn ipc_generation_error(files: &[(&str, &str)], paginated: bool) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    for (file, source) in files {
        write_synthetic_api(&api_dir, file, source);
    }
    let mut config = test_config(api_dir);
    if paginated {
        config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });
    }
    config.generators = vec![ServerGenerator::TauriIpc { output: tmp.path().join("ipc.rs") }];
    crate::servers::generate_transport(&config).expect_err("the IPC command cannot be generated")
}

/// A list that takes a `*Query` struct takes it as `query`, its IPC wire
/// key, so another argument named `query` fails the build, naming the
/// module, the fn and the argument.
#[test]
fn an_ipc_list_refuses_an_argument_named_like_its_query_struct() {
    let gadget = "\
pub async fn list(store: &Store, filter: GadgetQuery, query: Option<String>) -> Result<Vec<Gadget>, AppError> \
  { todo!() }
";
    assert_eq!(
        ipc_generation_error(&[("gadget.rs", gadget)], false),
        "ontogen: the IPC command `gadget_list` cannot be generated: `gadget::list` takes an argument named `query`, \
         which the command takes under the IPC wire key `query`, the key it uses for the list's `*Query` filter \
         struct, so the two would collide. Rename the argument."
    );
}

/// A paginated junction list takes the page as `limit` and `offset`, so its
/// one argument, the parent's id, named either fails the build.
/// Unpaginated, the same fn generates. (`add_tag` makes `list_tags` a
/// junction list; alone it would be a custom read, with no page.)
#[test]
fn a_paginated_ipc_command_refuses_an_argument_named_like_the_page() {
    let task = "\
pub async fn list_tags(store: &Store, limit: &str) -> Result<Vec<Tag>, AppError> { todo!() }
pub async fn add_tag(store: &Store, task_id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }
";
    assert_eq!(
        ipc_generation_error(&[("task.rs", task)], true),
        "ontogen: the IPC command `task_list_tags` cannot be generated: `task::list_tags` takes an argument named \
         `limit`, which the command takes under the IPC wire key `limit`, the key it uses for the page's `limit` and \
         `offset`, so the two would collide. Rename the argument."
    );
    let offset = task.replace("limit: &str", "offset: &str");
    assert!(ipc_generation_error(&[("task.rs", &offset)], true).contains("takes an argument named `offset`"));

    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "task.rs", task);
    let mut config = test_config(api_dir);
    config.generators = vec![ServerGenerator::TauriIpc { output: tmp.path().join("ipc.rs") }];
    crate::servers::generate_transport(&config).unwrap_or_else(|e| panic!("unpaginated: {e}"));
}

/// An event subscription takes its channel as `channel`, so an event fn
/// argument named `channel` fails the build.
#[test]
fn an_ipc_subscription_refuses_an_argument_named_channel() {
    let feed = "\
pub fn thing_changes(state: &AppState, channel: String) -> tokio::sync::broadcast::Receiver<String> { todo!() }
";
    assert_eq!(
        ipc_generation_error(&[("feed.rs", feed)], false),
        "ontogen: the IPC command `thing_changes_subscribe` cannot be generated: `feed::thing_changes` takes an \
         argument named `channel`, which the command takes under the IPC wire key `channel`, the key it uses for the \
         subscription's event channel, so the two would collide. Rename the argument."
    );
}

/// The error generating `files` scoped under `project_id`, with `generator`
/// as the one transport.
fn scoped_generation_error(files: &[(&str, &str)], generator: fn(PathBuf) -> ServerGenerator) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    for (file, source) in files {
        write_synthetic_api(&api_dir, file, source);
    }
    let mut config = test_config_with_prefix(api_dir);
    config.generators = vec![generator(tmp.path().join("out.rs"))];
    crate::servers::generate_transport(&config).expect_err("the scoped transport cannot be generated")
}

/// Every scoped IPC command takes the route prefix parameter under its
/// name, so a fn argument named like it fails the build, for a custom op
/// and a list's bare filter alike; unscoped, the same fns generate.
#[test]
fn a_scoped_ipc_command_refuses_an_argument_named_like_the_route_prefix() {
    let ipc = |output| ServerGenerator::TauriIpc { output };
    let archive = "\
pub async fn archive(store: &Store, project_id: &str) -> Result<(), AppError> { todo!() }
";
    assert_eq!(
        scoped_generation_error(&[("gadget.rs", archive)], ipc),
        "ontogen: the IPC command `gadget_archive` cannot be generated: `gadget::archive` takes an argument named \
         `project_id`, which the command takes under the IPC wire key `projectId`, the key it uses for the route \
         prefix parameter, so the two would collide. Rename the argument."
    );
    let list = "\
pub async fn list(store: &Store, project_id: Option<String>) -> Result<Vec<Gadget>, AppError> { todo!() }
";
    assert!(
        scoped_generation_error(&[("gadget.rs", list)], ipc)
            .contains("`gadget::list` takes an argument named `project_id`")
    );

    for source in [archive, list] {
        let tmp = tempfile::tempdir().unwrap();
        let api_dir = tmp.path().join("api");
        write_synthetic_api(&api_dir, "gadget.rs", source);
        let mut config = test_config(api_dir);
        config.generators = vec![ServerGenerator::TauriIpc { output: tmp.path().join("ipc.rs") }];
        crate::servers::generate_transport(&config).unwrap_or_else(|e| panic!("unscoped: {e}"));
    }
}

/// Tauri reads each command parameter under its name camelCased, so an
/// argument whose name camelCases to one of the command's own keys, or to
/// another argument's, collides with it although the Rust names differ:
/// `query_` with a `*Query` struct's `query`, `limit_` with a paginated
/// junction list's page, `channel_` with a subscription's channel, and
/// `tag_` with an argument `tag`.
#[test]
fn an_ipc_command_refuses_arguments_that_camelcase_to_one_key() {
    let collision = |command: &str, function: &str, arg: &str, key: &str, use_: &str| {
        format!(
            "ontogen: the IPC command `{command}` cannot be generated: `{function}` takes an argument named `{arg}`, \
             which the command takes under the IPC wire key `{key}`, the key it uses for {use_}, so the two would \
             collide. Rename the argument."
        )
    };
    let gadget = "\
pub async fn list(store: &Store, filter: GadgetQuery, query_: Option<String>) -> Result<Vec<Gadget>, AppError> \
  { todo!() }
";
    assert_eq!(
        ipc_generation_error(&[("gadget.rs", gadget)], false),
        collision("gadget_list", "gadget::list", "query_", "query", "the list's `*Query` filter struct")
    );

    let task = "\
pub async fn list_tags(store: &Store, limit_: &str) -> Result<Vec<Tag>, AppError> { todo!() }
pub async fn add_tag(store: &Store, task_id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }
";
    assert_eq!(
        ipc_generation_error(&[("task.rs", task)], true),
        collision("task_list_tags", "task::list_tags", "limit_", "limit", "the page's `limit` and `offset`")
    );

    let feed = "\
pub fn thing_changes(state: &AppState, channel_: String) -> tokio::sync::broadcast::Receiver<String> { todo!() }
";
    assert_eq!(
        ipc_generation_error(&[("feed.rs", feed)], false),
        collision(
            "thing_changes_subscribe",
            "feed::thing_changes",
            "channel_",
            "channel",
            "the subscription's event channel"
        )
    );

    let archive = "\
pub async fn archive(store: &Store, tag: &str, tag_: &str) -> Result<(), AppError> { todo!() }
";
    assert_eq!(
        ipc_generation_error(&[("gadget.rs", archive)], false),
        collision("gadget_archive", "gadget::archive", "tag_", "tag", "the argument `tag`")
    );
}

/// Tauri strips an argument's `r#` and lowerCamelCases it with heck, which
/// drops leading underscores: `_sort` travels as `sort`, `_project_id` as
/// `projectId` and `type_` as `type`, so each collides with the argument
/// whose key it takes.
#[test]
fn an_ipc_command_refuses_arguments_tauri_keys_alike() {
    let collision = |arg: &str, key: &str, other: &str| {
        format!(
            "ontogen: the IPC command `gadget_archive` cannot be generated: `gadget::archive` takes an argument \
             named `{arg}`, which the command takes under the IPC wire key `{key}`, the key it uses for the argument \
             `{other}`, so the two would collide. Rename the argument."
        )
    };
    for (args, arg, key, other) in [
        ("sort: &str, _sort: &str", "_sort", "sort", "sort"),
        ("project_id: &str, _project_id: &str", "_project_id", "projectId", "project_id"),
        ("r#type: &str, type_: &str", "type_", "type", "r#type"),
    ] {
        let archive = format!("pub async fn archive(store: &Store, {args}) -> Result<(), AppError> {{ todo!() }}\n");
        assert_eq!(ipc_generation_error(&[("gadget.rs", &archive)], false), collision(arg, key, other));
    }

    let archive = "\
pub async fn archive(store: &Store, _project_id: &str) -> Result<(), AppError> { todo!() }
";
    assert_eq!(
        scoped_generation_error(&[("gadget.rs", archive)], |output| ServerGenerator::TauriIpc { output }),
        "ontogen: the IPC command `gadget_archive` cannot be generated: `gadget::archive` takes an argument named \
         `_project_id`, which the command takes under the IPC wire key `projectId`, the key it uses for the route \
         prefix parameter, so the two would collide. Rename the argument."
    );
}

/// The keys [`ipc_arg_key`] derives are Tauri's: heck's lowerCamelCase of
/// the name without its `r#`, which drops every underscore, leading,
/// trailing or doubled, and splits a word before a digit run only at an
/// underscore.
#[test]
fn ipc_arg_keys_are_the_ones_tauri_reads() {
    for (param, key) in [
        ("project_id", "projectId"),
        ("_sort", "sort"),
        ("sort_", "sort"),
        ("__kind", "kind"),
        ("parent__id", "parentId"),
        ("r#type", "type"),
        ("r#in", "in"),
        ("page_2", "page2"),
        ("v2_beta", "v2Beta"),
        ("x_1_b", "x1B"),
        ("_1st", "1st"),
        ("fooBar", "fooBar"),
        ("HTTPServer", "httpServer"),
    ] {
        assert_eq!(ipc_arg_key(param), key, "{param}");
    }
}

/// A TS parameter is the IPC key, made a valid binding that no generated
/// call in the method body reads under the same name.
#[test]
fn ts_params_are_bindings_that_shadow_nothing() {
    for (param, name) in [
        ("project_id", "projectId"),
        ("_kind", "kind"),
        ("r#type", "type"),
        ("r#in", "in_"),
        ("class", "class_"),
        ("new", "new_"),
        ("default", "default_"),
        ("r#await", "await_"),
        ("arguments", "arguments_"),
        ("invoke", "invoke_"),
        ("call_op", "callOp_"),
        ("flatten_task", "flattenTask_"),
        ("flattened", "flattened"),
        ("_1st", "_1st"),
    ] {
        assert_eq!(ts_param(param), name, "{param}");
    }
    assert_eq!(ts_key("in"), "in");
    assert_eq!(ts_key("1st"), "'1st'");
}

/// The route prefix parameter travels camelCased too: an argument
/// `project_id_` takes its key, `projectId`.
#[test]
fn a_scoped_ipc_command_refuses_an_argument_that_camelcases_to_the_route_prefix() {
    let archive = "\
pub async fn archive(store: &Store, project_id_: &str) -> Result<(), AppError> { todo!() }
";
    assert_eq!(
        scoped_generation_error(&[("gadget.rs", archive)], |output| ServerGenerator::TauriIpc { output }),
        "ontogen: the IPC command `gadget_archive` cannot be generated: `gadget::archive` takes an argument named \
         `project_id_`, which the command takes under the IPC wire key `projectId`, the key it uses for the route \
         prefix parameter, so the two would collide. Rename the argument."
    );
}

/// A scoped MCP tool reads the route prefix parameter from its arguments
/// to pick the store, so an op argument named like it would share that
/// key: the build fails for a custom op and a list's bare filter. An
/// `*Input` taken as the whole argument object names no key, so it is not
/// refused.
#[test]
fn a_scoped_mcp_tool_refuses_an_argument_named_like_the_route_prefix() {
    let mcp = |output| ServerGenerator::Mcp { output };
    let archive = "\
pub async fn archive(store: &Store, project_id: &str) -> Result<(), AppError> { todo!() }
";
    assert_eq!(
        scoped_generation_error(&[("gadget.rs", archive)], mcp),
        "ontogen: the MCP tool `gadget_archive` cannot be generated: `gadget::archive` takes an argument named \
         `project_id`, which is the argument the tool itself reads for the route prefix parameter, so the two would \
         share one key. Rename the argument."
    );
    let list = "\
pub async fn list(store: &Store, project_id: Option<String>) -> Result<Vec<Gadget>, AppError> { todo!() }
";
    assert!(
        scoped_generation_error(&[("gadget.rs", list)], mcp)
            .contains("`gadget::list` takes an argument named `project_id`")
    );

    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "gadget.rs",
        "pub async fn capture(store: &Store, project_id: CaptureGadgetInput) -> Result<(), AppError> { todo!() }\n",
    );
    let mut config = test_config_with_prefix(api_dir);
    config.generators = vec![ServerGenerator::Mcp { output: tmp.path().join("mcp.rs") }];
    crate::servers::generate_transport(&config).unwrap_or_else(|e| panic!("a sole body: {e}"));
}

#[test]
fn server_metadata_routes_a_resource_update_as_patch() {
    let tmp = tempfile::tempdir().unwrap();
    let config = resource_fixture(tmp.path(), true);
    let modules = crate::servers::generate_transport(&config).unwrap();
    let meta = crate::servers::extract_server_metadata(&modules, &config);
    let route = |handler: &str| {
        let r = meta.http_routes.iter().find(|r| r.handler_name == handler).unwrap();
        (r.method.clone(), r.path.clone())
    };
    assert_eq!(route("task_update"), ("PATCH".to_string(), "/api/tasks/{id}".to_string()));
    assert_eq!(route("report_update"), ("PATCH".to_string(), "/api/reports/{id}".to_string()), "no route is PUT");
}

#[test]
fn a_resource_op_that_does_not_return_its_entity_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    write_synthetic_api(
        &config.api_dir,
        "epic.rs",
        &app_error_crud_source("epic").replace(
            "-> Result<Epic, AppError> { todo!() }\npub async fn create",
            "-> Result<EpicSummary, AppError> { todo!() }\npub async fn create",
        ),
    );
    config.generators = vec![ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") }];
    let err = crate::servers::generate_transport(&config).unwrap_err();
    assert!(err.contains("`epic::get_by_id` is served as the JSON:API resource `epics`"), "{err}");
    assert!(err.contains("must return `Epic`"), "{err}");
}

/// `tag::list` in the unpaginated resource fixture taking `filter` (the
/// parameters between the store and the page), with a `count` taking the
/// same filter when `paginated`.
fn filtered_tag_fixture(root: &std::path::Path, filter: &str, paginated: bool) -> Config {
    let mut config = resource_fixture(root, true);
    let page = if paginated { ", limit: Option<u64>, offset: Option<u64>" } else { "" };
    let mut tag = app_error_crud_source("tag")
        .replace("store: &Store, limit: Option<u64>, offset: Option<u64>", &format!("store: &Store, {filter}{page}"))
        .replace("count(store: &Store)", &format!("count(store: &Store, {filter})"));
    if !paginated {
        config.pagination = None;
        tag = tag.lines().filter(|l| !l.contains("fn count(")).map(|l| format!("{l}\n")).collect();
    }
    write_synthetic_api(&config.api_dir, "tag.rs", &tag);
    config
}

/// A list that takes a filter is served as its resource (§7.3): the
/// `*Query` struct is read from `filter[…]` with the members serde declares
/// for it, and `links.self` repeats the filter the request sent.
#[test]
fn a_filtered_resource_list_reads_its_filter_from_the_filter_family() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), filtered_tag_fixture(tmp.path(), "query: ListTagsQuery", false));
    let flat = compact(&http);

    assert!(
        flat.contains(&compact(
            "struct OntogenTagListFilterParams; impl OntogenRouteQuery for OntogenTagListFilterParams { const SPEC: OntogenQuerySpec = \
             OntogenQuerySpec { filter: &[], filter_fields: Some(ontogen_jsonapi::filter_fields::<ListTagsQuery>), sort: true, \
             include: true, ..OntogenQuerySpec::NONE }; }"
        )),
        "no page on an unpaginated list:\n{http}"
    );
    let list = handler_body(&http, "tag_list");
    assert_in_order(
        "tag_list",
        &list,
        &[
            "_: OntogenAcceptGuard, query: OntogenQuery<OntogenTagListFilterParams>, ) -> Result<OntogenResponse, OntogenErrorObject> {",
            "let ontogen_filter: ListTagsQuery = query.filter()?;",
            "ontogen_refuse_sort(&query, \"tags\")?;",
            "let include = query.include_paths(\"tags\", &[], &[])?;",
            "let link_query = query.link_query(include.as_deref())?;",
            "let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;",
            "let items = tag::list(&ontogen_store, ontogen_filter).await.map_err(ontogen_app_error)?;",
            "let mut document = OntogenDocument::new(data, OntogenLinks::new(link_query.href(collection)));",
            "if include.is_some() { document = document.with_included(Vec::new()); }",
            "Ok(ontogen_jsonapi::response::ok(&document))",
        ],
    );
    assert!(!list.contains("count("), "an unpaginated list has no total:\n{list}");
    assert!(!list.contains(".clone()"), "nothing reads the filter after the list:\n{list}");
    assert!(
        flat.contains("filter_fields:Some(ontogen_jsonapi::filter_fields::<ListTagsQuery>)"),
        "the runtime's member probe reads the filter struct:\n{http}"
    );
    assert!(flat.contains(&compact("get(tag_get_by_id).patch(tag_update)")), "the rest is served:\n{http}");
}

/// A paginated list reads its `*Query` struct, then its bare filters in byte
/// order of name, then `sort`, `include` and the page (§13.2 step 5), and
/// hands `list` and `count` the same filter in declaration order. Its links
/// carry the filter beside the page.
#[test]
fn a_paginated_filtered_resource_list_reads_struct_then_bare_filters_in_byte_order() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));
    let flat = compact(&http);

    assert!(
        flat.contains(&compact(
            "struct OntogenEpicListFilterParams; impl OntogenRouteQuery for OntogenEpicListFilterParams { const SPEC: OntogenQuerySpec = \
             OntogenQuerySpec { filter: &[\"owner\", \"title\"], filter_fields: Some(ontogen_jsonapi::filter_fields::<ListEpicsQuery>), \
             sort: true, include: true, page: true, ..OntogenQuerySpec::NONE }; }"
        )),
        "{http}"
    );
    assert_in_order(
        "epic_list",
        &handler_body(&http, "epic_list"),
        &[
            "query: OntogenQuery<OntogenEpicListFilterParams>)",
            "let ontogen_filter: ListEpicsQuery = query.filter()?;",
            "let ontogen_filter_owner = query.required_filter_member::<String>(\"owner\")?;",
            "let ontogen_filter_title = query.filter_member::<String>(\"title\")?;",
            "let order = query.sort_order(\"epics\")?;",
            "let include = query.include_paths(\"epics\", &[], &[])?;",
            "let (offset, limit) = ontogen_page(&query, 20, 100)?;",
            "let link_query = query.link_query(include.as_deref())?;",
            "let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;",
            "let items = epic::list(&ontogen_store, ontogen_filter.clone(), ontogen_filter_title.as_deref(), \
             &ontogen_filter_owner, &order, Some(u64::from(limit)), Some(u64::from(offset))).await\
             .map_err(ontogen_app_error)?;",
            "let total = epic::count(&ontogen_store, ontogen_filter, ontogen_filter_title.as_deref(), \
             &ontogen_filter_owner).await.map_err(ontogen_app_error)?;",
            "let collection = \"/api/epics\";",
            "let links = ontogen_jsonapi::links::pagination_links(collection, &link_query, offset, limit, total);",
            "let mut document = OntogenDocument::new(data, links).with_meta(OntogenPageMeta { total, limit, offset });",
            "Ok(ontogen_jsonapi::response::ok(&document))",
        ],
    );
    // An unfiltered list beside it keeps its shared spec.
    assert!(
        handler_body(&http, "task_list").contains(&compact("query: OntogenQuery<OntogenPagedListParams>")),
        "{http}"
    );
}

/// Under a route prefix the filtered list is the same handler, reading the
/// prefix first and linking under it (§11.1).
#[test]
fn a_scoped_filtered_resource_list_reads_its_filter_as_the_unscoped_one_does() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), true));
    let flat = compact(&http);

    assert!(
        flat.contains(&compact(
            "impl OntogenRouteQuery for OntogenEpicListScopedFilterParams { const SPEC: OntogenQuerySpec = OntogenQuerySpec { filter: \
             &[\"owner\", \"title\"], filter_fields: Some(ontogen_jsonapi::filter_fields::<ListEpicsQuery>), sort: true, include: \
             true, page: true, ..OntogenQuerySpec::NONE }; }"
        )),
        "{http}"
    );
    assert_in_order(
        "epic_list_scoped",
        &handler_body(&http, "epic_list_scoped"),
        &[
            "OntogenPath(ontogen_scope): OntogenPath<uuid::Uuid>, query: OntogenQuery<OntogenEpicListScopedFilterParams>",
            "let ontogen_filter: ListEpicsQuery = query.filter()?;",
            "let ontogen_filter_owner = query.required_filter_member::<String>(\"owner\")?;",
            "let ontogen_filter_title = query.filter_member::<String>(\"title\")?;",
            "let order = query.sort_order(\"epics\")?;",
            "let include = query.include_paths(\"epics\", &[], &[])?;",
            "let (offset, limit) = ontogen_page(&query, 20, 100)?;",
            "let link_query = query.link_query(include.as_deref())?;",
            "let ontogen_store = ontogen_state.store_for(&ontogen_scope).map_err(ontogen_internal_error)?;",
            "epic::list(&ontogen_store, ontogen_filter.clone(), ontogen_filter_title.as_deref(), &ontogen_filter_owner, \
             &order,",
            "epic::count(&ontogen_store, ontogen_filter, ontogen_filter_title.as_deref(), &ontogen_filter_owner)",
            "let collection = &format!(\"/api/projects/{}/epics\", ontogen_jsonapi::links::encode_path_segment(&ontogen_scope.to_string()));",
            "let links = ontogen_jsonapi::links::pagination_links(collection, &link_query, offset, limit, total);",
        ],
    );
    assert!(
        flat.contains(&compact(".route(\"/api/projects/{project_id}/epics\", axum::routing::get(epic_list_scoped)")),
        "{http}"
    );
}

/// A resource list binds `query`, `items`, `collection`, `links` and others
/// of its own, so its filters are bound as `ontogen_filter_{name}`: a filter
/// may take any of those names. An owned filter `count` reads after `list`
/// is cloned into `list`; a copied one is not.
#[test]
fn a_resource_list_filter_may_be_named_like_a_handler_binding() {
    let tmp = tempfile::tempdir().unwrap();
    let filter = "query: Option<&str>, items: Option<u32>, collection: &str, links: Option<bool>, total: String, \
                  link_query: u64, include: Option<u32>, document: Option<bool>";
    let http = generate_http(tmp.path(), filtered_tag_fixture(tmp.path(), filter, true));

    let bound = handler_bindings(&http, "tag_list");
    let mut unique = bound.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), bound.len(), "tag_list binds a name twice: {bound:?}");
    for name in ["query", "items", "collection", "links", "total", "link_query", "include", "document"] {
        assert!(bound.contains(&format!("ontogen_filter_{name}")), "{name}: {bound:?}");
    }
    assert!(http.contains(
        "filter: &[\"collection\", \"document\", \"include\", \"items\", \"link_query\", \"links\", \"query\", \"total\"],"
    ));
    assert!(!http.contains("filter_fields"), "no struct, no member probe:\n{http}");
    let list = handler_body(&http, "tag_list");
    assert!(
        list.contains(&compact(
            "tag::list(&ontogen_store, ontogen_filter_query.as_deref(), ontogen_filter_items, \
             &ontogen_filter_collection, ontogen_filter_links, ontogen_filter_total.clone(), \
             ontogen_filter_link_query, ontogen_filter_include, ontogen_filter_document, Some(u64::from(limit)), \
             Some(u64::from(offset)))"
        )),
        "{list}"
    );
    assert!(
        list.contains(&compact(
            "tag::count(&ontogen_store, ontogen_filter_query.as_deref(), ontogen_filter_items, \
             &ontogen_filter_collection, ontogen_filter_links, ontogen_filter_total, ontogen_filter_link_query, \
             ontogen_filter_include, ontogen_filter_document)"
        )),
        "{list}"
    );
}

/// A list in a module with no entity is served as a custom op (§10.4): its
/// filter is read from `filter[…]` before its `opArg` page, and the page and
/// the total come from the store's `list` and `count`, which take the same
/// filter, answered as `meta.result`.
#[test]
fn an_entityless_filtered_list_reads_its_filter_then_its_op_arg_page() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));
    let flat = compact(&http);

    assert!(
        flat.contains(&compact(
            "struct OntogenAgentListFilterParams; impl OntogenRouteQuery for OntogenAgentListFilterParams { const SPEC: OntogenQuerySpec = \
             OntogenQuerySpec { filter: &[\"skill_id\"], filter_fields: Some(ontogen_jsonapi::filter_fields::<AgentQuery>), op_args: \
             &[\"limit\", \"offset\"], ..OntogenQuerySpec::NONE }; }"
        )),
        "{http}"
    );
    assert_in_order(
        "agent_list",
        &handler_body(&http, "agent_list"),
        &[
            "_: OntogenAcceptGuard, ontogen_query: OntogenQuery<OntogenAgentListFilterParams>, ) -> Result<OntogenResponse, OntogenErrorObject> {",
            "let ontogen_filter: AgentQuery = ontogen_query.filter()?;",
            "let ontogen_filter_skill_id = ontogen_query.filter_member::<String>(\"skill_id\")?;",
            "let ontogen_limit = ontogen_query.page_op_arg(\"limit\")?.unwrap_or(20).min(100);",
            "let ontogen_offset = ontogen_query.page_op_arg(\"offset\")?.unwrap_or(0);",
            "let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;",
            "let ontogen_items = agent::list(&ontogen_store, ontogen_filter.clone(), ontogen_filter_skill_id.clone(), \
             Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset))).await.map_err(ontogen_app_error)?;",
            "let ontogen_total = agent::count(&ontogen_store, ontogen_filter, ontogen_filter_skill_id)\
             .await.map_err(ontogen_app_error)?;",
            "let ontogen_result = OntogenPaginatedResult { items: ontogen_items, total: ontogen_total, limit: ontogen_limit, \
             offset: ontogen_offset, };",
            "Ok(ontogen_jsonapi::response::ok(&OntogenDocument::meta_only(OntogenResultMeta { result: ontogen_result })))",
        ],
    );
    assert!(flat.contains(&compact(".route(\"/api/agents\", axum::routing::get(agent_list)")), "{http}");
    assert!(!handler_body(&http, "agent_list").contains("OntogenLinks"), "a meta.result page has no links:\n{http}");
}

/// Unpaginated, the entity-less filtered list accepts its filter and no
/// `opArg`, and answers the whole list, unscoped and under a prefix alike.
/// A fn that takes the page on a surface that does not paginate is passed
/// no page.
#[test]
fn an_unpaginated_entityless_filtered_list_answers_the_whole_list() {
    let source = "pub async fn list(store: &Store, kind: &str, verbose: Option<bool>, limit: Option<u64>, \
                  offset: Option<u64>) -> Result<Vec<String>, anyhow::Error> { todo!() }\n";
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let api_dir = tmp.path().join("api");
        write_synthetic_api(&api_dir, "feed.rs", source);
        let config = if scoped { test_config_with_prefix(api_dir) } else { test_config(api_dir) };
        let http = generate_http(tmp.path(), config);
        let (handler, spec) = if scoped {
            ("feed_list_scoped", "OntogenFeedListScopedFilterParams")
        } else {
            ("feed_list", "OntogenFeedListFilterParams")
        };
        assert!(
            compact(&http).contains(&compact(&format!(
                "impl OntogenRouteQuery for {spec} {{ const SPEC: OntogenQuerySpec = OntogenQuerySpec {{ filter: &[\"kind\", \"verbose\"], \
                 ..OntogenQuerySpec::NONE }}; }}"
            ))),
            "{http}"
        );
        let open = if scoped {
            "let ontogen_store = ontogen_state.store_for(&ontogen_scope).map_err(ontogen_internal_error)?;"
        } else {
            "let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;"
        };
        assert_in_order(
            handler,
            &handler_body(&http, handler),
            &[
                &format!("ontogen_query: OntogenQuery<{spec}>"),
                "let ontogen_filter_kind = ontogen_query.required_filter_member::<String>(\"kind\")?;",
                "let ontogen_filter_verbose = ontogen_query.filter_member::<bool>(\"verbose\")?;",
                open,
                "let ontogen_result = feed::list(&ontogen_store, &ontogen_filter_kind, ontogen_filter_verbose, None, \
                 None).await.map_err(ontogen_internal_error)?;",
                "Ok(ontogen_jsonapi::response::ok(&OntogenDocument::meta_only(OntogenResultMeta { result: ontogen_result })))",
            ],
        );
        let body = handler_body(&http, handler);
        assert!(!body.contains("count(") && !body.contains("page_op_arg"), "{body}");
        assert_bindings_shadow_nothing(&http, handler, &[]);
    }
}

/// No generated route reads its query with Axum's own `Query`: every list
/// reads the JSON:API families. Only an event stream, which is no JSON:API
/// route (§12), still does.
#[test]
fn no_list_speaks_the_flat_query_dialect() {
    let tmp = tempfile::tempdir().unwrap();
    let files = [
        generate_http(tmp.path(), filtered_tag_fixture(tmp.path(), "title: &str, query: ListTagsQuery", false)),
        generate_http(tmp.path(), filtered_tag_fixture(tmp.path(), "title: &str, query: ListTagsQuery", true)),
        generate_http(tmp.path(), resource_fixture(tmp.path(), true)),
    ];
    for http in &files {
        for flat in ["axum::extract::Query", "QueryRejection", "ontogen_query_rejection", "PaginationParams", "Json"] {
            assert!(!http.contains(flat), "{flat}:\n{http}");
        }
    }
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let http = generate_http(tmp.path(), ops_fixture(tmp.path(), scoped));
        assert!(!http.contains("PaginationParams") && !http.contains("Json"), "{http}");
        for chunk in http.split("\nasync fn ").skip(1) {
            let name = chunk.split('(').next().unwrap();
            if !name.contains("_sse") {
                assert!(!chunk.contains("axum::extract::Query"), "{name} reads a flat query:\n{chunk}");
            }
        }
    }
}

#[test]
fn only_the_primary_surfaces_app_error_maps_through_app_error() {
    let tmp = tempfile::tempdir().unwrap();
    let surfaces = two_surface_fixture(tmp.path());
    let ops = |surface: &str, file: &str, uses: &str, store: &str, errors: &[(&str, &str)]| {
        let file = tmp.path().join(surface).join(file);
        let mut source = std::fs::read_to_string(&file).unwrap_or_default();
        source.insert_str(0, uses);
        for (name, error) in errors {
            source.push_str(&format!(
                "pub async fn {name}({store}, id: &str) -> Result<Workout, {error}> {{ todo!() }}\n"
            ));
        }
        std::fs::write(file, source).unwrap();
    };
    ops("primary", "workout.rs", "", "state: &AppState", &[("finish", "AppError"), ("resume", "schema::AppError")]);
    ops(
        "primary",
        "session.rs",
        "use fitness::schema::AppError;\nuse crate::schema::{self as api_schema, AppError as ApiError};\nuse \
         fitness::schema;\n",
        "state: &AppState",
        &[
            ("close", "AppError"),
            ("reopen", "ApiError"),
            ("pause", "api_schema::AppError"),
            ("skip", "schema::AppError"),
            ("halt", "::fitness::schema::AppError"),
        ],
    );
    ops(
        "primary",
        "plan.rs",
        "use crate::schema::*;\nuse crate::schema::error;\n",
        "state: &AppState",
        &[
            ("adopt", "AppError"),
            ("draft", "crate::schema::error::AppError"),
            ("shelve", "error::AppError"),
            ("borrow", "super::schema::AppError"),
        ],
    );
    ops(
        "fitness",
        "workout.rs",
        "",
        "store: &FitnessStore",
        &[
            ("archive", "AppError"),
            ("rename", "schema::AppError"),
            ("retire", "fitness::schema::AppError"),
            ("restore", "crate::schema::AppError"),
        ],
    );
    write_synthetic_api(&tmp.path().join("schema"), "mod.rs", "pub enum AppError { WorkoutNotFound(String) }\n");
    let mut config = two_surface_config(surfaces);
    config.error_map = crate::servers::error_map::scan(&tmp.path().join("schema")).unwrap();
    config.generators = vec![ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") }];
    let modules = crate::servers::generate_transport(&config).expect("generate_transport failed");
    let http = std::fs::read_to_string(tmp.path().join("http.rs")).unwrap();

    assert!(http.contains("fn ontogen_app_error(e: crate::schema::AppError) -> OntogenErrorObject {"), "{http}");
    let mapping = |handler: &str| {
        let body = &http[http.find(&format!("async fn {handler}(")).unwrap_or_else(|| panic!("{handler}:\n{http}"))..];
        let body = &body[..body.find("\n}\n").unwrap()];
        if body.contains(".map_err(ontogen_app_error)") {
            "ontogen_app_error"
        } else if body.contains(".map_err(ontogen_internal_error)") {
            "ontogen_internal_error"
        } else {
            panic!("no error mapping in {body}")
        }
    };
    for (handler, expected) in [
        ("workout_finish", "ontogen_app_error"),
        ("workout_resume", "ontogen_app_error"),
        // The file's `use` items decide what a bare or leading-segment name is.
        ("session_close", "ontogen_internal_error"),
        ("session_reopen", "ontogen_app_error"),
        ("session_pause", "ontogen_app_error"),
        ("session_skip", "ontogen_internal_error"),
        ("session_halt", "ontogen_internal_error"),
        // A glob binds no name: a bare `AppError` is read as the surface's own.
        ("plan_adopt", "ontogen_app_error"),
        // Under the primary types module, but not provably its `AppError`.
        ("plan_draft", "ontogen_internal_error"),
        ("plan_shelve", "ontogen_internal_error"),
        ("plan_borrow", "ontogen_internal_error"),
        // A bare or relative `AppError` in the fitness surface is its own,
        // and its `crate::` is the `fitness` crate its service path names.
        ("workout_archive", "ontogen_internal_error"),
        ("workout_rename", "ontogen_internal_error"),
        ("workout_retire", "ontogen_internal_error"),
        ("workout_restore", "ontogen_internal_error"),
    ] {
        assert_eq!(mapping(handler), expected, "{handler}");
    }

    let warnings = crate::servers::generators::http::unplaced_app_error_warnings(&modules, &config);
    let warned: Vec<&str> = ["plan::draft", "plan::shelve", "plan::borrow"]
        .into_iter()
        .filter(|f| warnings.iter().any(|w| w.contains(&format!("`{f}`"))))
        .collect();
    assert_eq!(warned, ["plan::draft", "plan::shelve", "plan::borrow"], "{warnings:#?}");
    assert_eq!(warnings.len(), 3, "another surface's own `AppError` is not warned about: {warnings:#?}");
    assert!(warnings[0].starts_with("cargo:warning="), "{}", warnings[0]);
    assert!(
        warnings.iter().any(|w| w.contains("returns `crate::schema::error::AppError`")
            && w.contains("not proven to be `crate::schema::AppError`")
            && w.contains("500 internal_error")),
        "{warnings:#?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON:API custom ops, ops served as custom ops, and event frames
// (wire contract §10, §11.1, §12)
// ═══════════════════════════════════════════════════════════════════════════════

/// The body of handler `name` in `http`, compacted.
fn handler_body(http: &str, name: &str) -> String {
    let code = &http[http.find(&format!("async fn {name}(")).unwrap_or_else(|| panic!("no {name} in:\n{http}"))..];
    compact(&code[..code.find("\n}\n").unwrap()])
}

/// Each of `steps` appears in `body`, in order.
fn assert_in_order(name: &str, body: &str, steps: &[&str]) {
    let mut from = 0;
    for step in steps {
        let step = compact(step);
        let at = body[from..].find(&step).unwrap_or_else(|| panic!("{name}: `{step}` after byte {from} in:\n{body}"));
        from += at + step.len();
    }
}

#[test]
fn a_custom_get_reads_its_options_as_op_args_in_byte_order() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));

    // Only the declared `opArg`s are accepted (§10.2); everything else,
    // `?verbose=` included, is a 400 from the `Query` extractor.
    assert!(http.contains("op_args: &[\"verbose\", \"label\"]"), "{http}");
    assert!(compact(&http).contains(&compact(
        ".route(\"/api/workouts/summary/{id}\", axum::routing::get(workout_get_summary).fallback(ontogen_allow([OntogenMethod::GET])))"
    )));
    assert_in_order(
        "workout_get_summary",
        &handler_body(&http, "workout_get_summary"),
        &[
            "_: OntogenAcceptGuard,",
            "OntogenPath(id): OntogenPath<String>,",
            "ontogen_query: OntogenQuery<OntogenWorkoutGetSummaryOpArgs>",
            // `label` sorts before `verbose`.
            "let label = ontogen_query.op_arg::<String>(\"label\")?;",
            "let verbose = ontogen_query.op_arg::<bool>(\"verbose\")?;",
            "let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;",
            "let ontogen_result = workout::get_summary(&ontogen_store, &id, verbose, label.as_deref()).await.map_err(ontogen_app_error)?;",
            "Ok(ontogen_jsonapi::response::ok(&OntogenDocument::meta_only(OntogenResultMeta { result: ontogen_result })))",
        ],
    );
    // A stateless GET takes no state and still checks `Accept` and its query.
    assert!(
        compact(&http).contains(&compact(
            "async fn workout_get_version(_: OntogenAcceptGuard, _: OntogenQuery<OntogenNoParams>) -> Result<OntogenResponse, OntogenErrorObject>"
        )),
        "{http}"
    );
}

#[test]
fn a_custom_post_reads_every_argument_from_meta_args() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));

    // The media type, then the query (none accepted), then the body.
    assert_in_order(
        "workout_start",
        &handler_body(&http, "workout_start"),
        &[
            "_: OntogenAcceptGuard,",
            "ontogen_query: Result<OntogenQuery<OntogenNoParams>, OntogenErrorObject>,",
            "ontogen_body: OntogenBody",
            "ontogen_query?;",
            "let ontogen_bytes = ontogen_body.into_bytes()?;",
            "let ontogen_args = ontogen_jsonapi::request::op_args(&ontogen_bytes, true)?;",
            "ontogen_jsonapi::request::check_op_arg_names(&ontogen_args, &[\"input\", \"note\"])?;",
            // Declaration order; the `*Input` and the `Option` are members too.
            "let input = ontogen_jsonapi::request::op_arg::<StartWorkoutInput>(&ontogen_args, \"input\", true)?;",
            "let note = ontogen_jsonapi::request::op_arg::<Option<String>>(&ontogen_args, \"note\", false)?;",
            "let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;",
            // Not an `AppError`: a 500.
            "let ontogen_result = workout::start(&ontogen_store, input, note).await.map_err(ontogen_internal_error)?;",
            "Ok(ontogen_jsonapi::response::ok(&OntogenDocument::meta_only(OntogenResultMeta { result: ontogen_result })))",
        ],
    );
    // A POST has no path parameters: a required argument is a member too.
    let rename = handler_body(&http, "workout_rename");
    assert!(!rename.contains("OntogenPath"), "{rename}");
    assert!(
        rename.contains(&compact("let id = ontogen_jsonapi::request::op_arg::<String>(&ontogen_args, \"id\", true)?;"))
    );
    assert!(rename.contains(&compact(
        "workout::rename(&ontogen_store, &id, name).await.map_err(ontogen_app_error)?; Ok(ontogen_jsonapi::response::no_content())"
    )));
    // With no required argument, a missing `meta` or `args` is `{}`; a `()`
    // result is a 204.
    let pause = handler_body(&http, "workout_pause");
    assert!(pause.contains(&compact("ontogen_jsonapi::request::op_args(&ontogen_bytes, false)?;")), "{pause}");
    assert!(pause.contains(&compact("ontogen_jsonapi::request::check_op_arg_names(&ontogen_args, &[])?;")), "{pause}");
    assert!(pause.contains(&compact("Ok(ontogen_jsonapi::response::no_content())")), "{pause}");
    assert!(!handler_body(&http, "workout_start").contains("created("), "a custom op is never a 201");
}

#[test]
fn junction_ops_outside_a_resource_module_are_served_as_custom_ops_at_their_routes() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));
    let flat = compact(&http);

    assert!(flat.contains(&compact(
        ".route(\"/api/workouts/{parent_id}/labels\", axum::routing::get(workout_list_labels).post(workout_add_label)\
         .fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::POST])))"
    )));
    assert!(flat.contains(&compact(
        ".route(\"/api/workouts/{parent_id}/labels/{child_id}\", axum::routing::delete(workout_remove_label)\
         .fallback(ontogen_allow([OntogenMethod::DELETE])))"
    )));
    // A paginated junction list slices the fn's whole result.
    assert_in_order(
        "workout_list_labels",
        &handler_body(&http, "workout_list_labels"),
        &[
            "OntogenPath(workout_id): OntogenPath<String>, ontogen_query: OntogenQuery<OntogenPageOpArgs>",
            "let ontogen_limit = ontogen_query.page_op_arg(\"limit\")?.unwrap_or(20).min(100);",
            "let ontogen_offset = ontogen_query.page_op_arg(\"offset\")?.unwrap_or(0);",
            "let ontogen_all = workout::list_labels(&ontogen_store, &workout_id).await.map_err(ontogen_app_error)?;",
            "let ontogen_total = ontogen_all.len() as u64;",
            "let ontogen_items = ontogen_all.into_iter().skip(ontogen_offset as usize).take(ontogen_limit as usize).collect();",
            "let ontogen_result = OntogenPaginatedResult { items: ontogen_items, total: ontogen_total, limit: ontogen_limit, offset: ontogen_offset, };",
        ],
    );
    assert_in_order(
        "workout_add_label",
        &handler_body(&http, "workout_add_label"),
        &[
            "ontogen_path: Result<OntogenPath<String>, OntogenErrorObject>,",
            "let OntogenPath(workout_id) = ontogen_path?;",
            "ontogen_jsonapi::request::check_op_arg_names(&ontogen_args, &[\"label\"])?;",
            "workout::add_label(&ontogen_store, &workout_id, &label).await.map_err(ontogen_app_error)?;",
            "Ok(ontogen_jsonapi::response::no_content())",
        ],
    );
    assert_in_order(
        "workout_remove_label",
        &handler_body(&http, "workout_remove_label"),
        &[
            "OntogenPath((workout_id, label)): OntogenPath<(String, String)>, _: OntogenQuery<OntogenNoParams>",
            "Ok(ontogen_jsonapi::response::no_content())",
        ],
    );

    // A resource module's junction ops have no handler of their own: they
    // serve its relationship routes (§9.1).
    for op in ["list_labels", "add_label", "remove_label"] {
        assert!(!http.contains(&format!("async fn task_{op}(")), "task::{op}:\n{http}");
    }
    // A `list_X` with no add or remove beside it is a custom GET, answering
    // its whole result: it has no page.
    assert!(flat.contains(&compact(
        ".route(\"/api/tasks/list-overdue/{before}\", axum::routing::get(task_list_overdue).fallback(ontogen_allow([OntogenMethod::GET])))"
    )));
    let lone = handler_body(&http, "task_list_overdue");
    assert!(
        lone.contains(&compact("OntogenPath(before): OntogenPath<String>, _: OntogenQuery<OntogenNoParams>")),
        "{lone}"
    );
    assert!(lone.contains(&compact("let ontogen_result = task::list_overdue(&ontogen_store, &before)")), "{lone}");
    assert!(!lone.contains("OntogenPaginatedResult"), "{lone}");
}

#[test]
fn event_frames_send_entities_as_unlinked_resources_and_other_items_as_meta_result() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));
    let flat = compact(&http);

    // `Task` is an entity: its resource object, with every link left out.
    assert!(flat.contains(&compact(
        "fn ontogen_task_frame_data(event: OntogenEvent, entity: &Task) -> Result<OntogenEvent, axum::Error> { \
         event.json_data(ontogen_task_as_resource(entity, \"/api/tasks\").into_unlinked()) }"
    )));
    assert!(http.contains(
        "ontogen_sse_stream(\"task-changed\", ontogen_rx, ontogen_core::events::no_id, ontogen_task_frame_data)"
    ));
    // `Activity` and `String` are not: `{"meta":{"result":…}}`.
    assert!(http.contains(
        "ontogen_sse_stream(\"activity-for-kind\", ontogen_rx, ontogen_core::events::seq_id, ontogen_result_frame))"
    ));
    assert!(
        http.contains(
            "ontogen_sse_stream(\"log-lines\", ontogen_rx, ontogen_core::events::no_id, ontogen_result_frame))"
        )
    );
    assert!(http.contains("event.json_data(OntogenResultFrame::new(item))"));
    // A lag frame keeps its bare shape.
    assert!(http.contains("event(\"lag\").data(format!(\"{{\\\"skipped\\\":{skipped}}}\"))"));
}

#[test]
fn an_entity_only_events_carry_still_gets_its_resource_builder() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    // Only `report` is left with ops; `Task` reaches the file as an event item.
    for module in ["task", "epic", "tag"] {
        std::fs::remove_file(config.api_dir.join(format!("{module}.rs"))).unwrap();
    }
    write_synthetic_api(
        &config.api_dir,
        "activity.rs",
        "pub fn task_changed(state: &AppState) -> tokio::sync::broadcast::Receiver<Task> { todo!() }\n",
    );
    config.pagination = None;
    let http = generate_http(tmp.path(), config);

    assert!(http.contains("struct OntogenTaskResourceAttributes<'a>(&'a Task);"), "{http}");
    // No module serves `get_by_id` for it, so it has no links to leave out.
    assert!(http.contains("fn ontogen_task_as_resource<'a>(entity: &'a Task) -> OntogenResourceObject<"), "{http}");
    assert!(http.contains(
        "OntogenResourceObject::without_links(\"tasks\", entity.id.clone(), OntogenTaskResourceAttributes(entity))"
    ));
    assert!(http.contains("fn ontogen_task_frame_data(event: OntogenEvent, entity: &Task)"), "{http}");
    assert!(http.contains("event.json_data(ontogen_task_as_resource(entity).into_unlinked())"), "{http}");
    assert!(!http.contains("ontogen_task_lookup_key"), "no handler serves `tasks`:\n{http}");
    assert!(!http.contains("OntogenListParams"), "no resource route, no resource query specs:\n{http}");
}

#[test]
fn a_failed_subscribe_maps_its_error_like_any_op() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let http = generate_http(tmp.path(), ops_fixture(tmp.path(), scoped));
        let flat = compact(&http);
        // `AppError` gets its status; any other error is a 500.
        let mut calls = vec![
            "activity::activity_for_kind(&ontogen_state, kind, ontogen_resume).await.map_err(ontogen_app_error)?;",
            "activity::log_lines(&ontogen_state, ontogen_query.level).map_err(ontogen_internal_error)?;",
        ];
        if scoped {
            calls.extend([
                "ontogen_state.subscribe_activity_for_kind_for(&ontogen_scope, kind, \
                 ontogen_resume).await.map_err(ontogen_app_error)?;",
                "ontogen_state.subscribe_log_lines_for(&ontogen_scope, ontogen_query.level).map_err(ontogen_internal_error)?;",
            ]);
        }
        for call in calls {
            assert!(flat.contains(&compact(call)), "{call}:\n{http}");
        }
    }
}

#[test]
fn scoped_ops_have_the_unscoped_wire() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), true));
    let flat = compact(&http);

    // A list that is not served as a resource pages through the store under
    // the prefix too: no in-memory slicing.
    assert_in_order(
        "report_list_scoped",
        &handler_body(&http, "report_list_scoped"),
        &[
            "OntogenPath(ontogen_scope): OntogenPath<uuid::Uuid>, ontogen_query: OntogenQuery<OntogenPageOpArgs>",
            "let ontogen_store = ontogen_state.store_for(&ontogen_scope).map_err(ontogen_internal_error)?;",
            "report::list(&ontogen_store, Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset))).await.map_err(ontogen_app_error)?;",
            "let ontogen_total = report::count(&ontogen_store).await.map_err(ontogen_app_error)?;",
        ],
    );
    assert_in_order(
        "agent_list_scoped",
        &handler_body(&http, "agent_list_scoped"),
        &[
            "OntogenPath(ontogen_scope): OntogenPath<uuid::Uuid>, ontogen_query: OntogenQuery<OntogenAgentListScopedFilterParams>",
            "let ontogen_filter: AgentQuery = ontogen_query.filter()?;",
            "let ontogen_filter_skill_id = ontogen_query.filter_member::<String>(\"skill_id\")?;",
            "let ontogen_limit = ontogen_query.page_op_arg(\"limit\")?.unwrap_or(20).min(100);",
            "let ontogen_store = ontogen_state.store_for(&ontogen_scope).map_err(ontogen_internal_error)?;",
            "agent::list(&ontogen_store, ontogen_filter.clone(), ontogen_filter_skill_id.clone(), \
             Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset)))",
            "let ontogen_total = agent::count(&ontogen_store, ontogen_filter, ontogen_filter_skill_id)\
             .await.map_err(ontogen_app_error)?;",
            "Ok(ontogen_jsonapi::response::ok(&OntogenDocument::meta_only(OntogenResultMeta { result: ontogen_result })))",
        ],
    );
    for list in ["report_list_scoped", "agent_list_scoped"] {
        assert!(!handler_body(&http, list).contains("skip("), "{list} slices nothing in memory");
    }
    // A scoped junction list outside a resource module has the unscoped
    // route under the prefix, and pages like the unscoped one.
    assert_in_order(
        "workout_list_labels_scoped",
        &handler_body(&http, "workout_list_labels_scoped"),
        &[
            "OntogenPath((ontogen_scope, workout_id)): OntogenPath<(uuid::Uuid, String)>, ontogen_query: OntogenQuery<OntogenPageOpArgs>",
            "let ontogen_limit = ontogen_query.page_op_arg(\"limit\")?.unwrap_or(20).min(100);",
            "let ontogen_store = ontogen_state.store_for(&ontogen_scope).map_err(ontogen_internal_error)?;",
            "workout::list_labels(&ontogen_store, &workout_id)",
            "let ontogen_result = OntogenPaginatedResult { items: ontogen_items, total: ontogen_total, limit: ontogen_limit, offset: ontogen_offset, };",
        ],
    );
    assert_in_order(
        "workout_add_label_scoped",
        &handler_body(&http, "workout_add_label_scoped"),
        &[
            "ontogen_path: Result<OntogenPath<(uuid::Uuid, String)>, OntogenErrorObject>,",
            "let OntogenPath((ontogen_scope, workout_id)) = ontogen_path?;",
            "ontogen_jsonapi::request::check_op_arg_names(&ontogen_args, &[\"label\"])?;",
        ],
    );
    // A custom op reads its arguments as the unscoped one does, after the
    // prefix.
    assert_in_order(
        "workout_start_scoped",
        &handler_body(&http, "workout_start_scoped"),
        &[
            "ontogen_path: Result<OntogenPath<uuid::Uuid>, OntogenErrorObject>,",
            "let OntogenPath(ontogen_scope) = ontogen_path?;",
            "ontogen_query?;",
            "ontogen_jsonapi::request::check_op_arg_names(&ontogen_args, &[\"input\", \"note\"])?;",
            "let ontogen_store = ontogen_state.store_for(&ontogen_scope).map_err(ontogen_internal_error)?;",
        ],
    );
    assert!(flat.contains(&compact(
        "OntogenPath((ontogen_scope, id)): OntogenPath<(uuid::Uuid, String)>, ontogen_query: OntogenQuery<OntogenWorkoutGetSummaryOpArgs>"
    )));
    assert!(flat.contains(&compact(
        ".route(\"/api/projects/{project_id}/workouts/{parent_id}/labels\", axum::routing::get(workout_list_labels_scoped)\
         .post(workout_add_label_scoped).fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::POST])))"
    )));
    assert!(flat.contains(&compact(
        ".route(\"/api/projects/{project_id}/workouts/{parent_id}/labels/{child_id}\", \
         axum::routing::delete(workout_remove_label_scoped).fallback(ontogen_allow([OntogenMethod::DELETE])))"
    )));
    assert!(!flat.contains("list-labels") && !flat.contains("add-label"), "no action-style junction route:\n{http}");
    // Every handler name is unique, including `tag::list` and
    // `workout::list_labels`, whose scoped names are derived from different
    // command names.
    let names: Vec<&str> = http.lines().filter_map(|l| l.strip_prefix("async fn ")?.split('(').next()).collect();
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "{names:?}");
    // The stateless op has no store to scope: it keeps its unscoped route.
    assert!(flat.contains(&compact(".route(\"/api/workouts/version\", axum::routing::get(workout_get_version)")));
}

/// A junction add or remove answers `204` whatever its fn returns, under the
/// route prefix as at its unscoped route: served as a custom op outside a
/// resource module, and through the relationship route inside one.
#[test]
fn a_junction_add_or_remove_answers_204_scoped_or_not() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = resource_fixture(tmp.path(), true);
        let api_dir = tmp.path().join("api");
        let ops = |module: &str, parent: &str| {
            format!(
                "pub async fn list_labels(store: &Store, {parent}: &str) -> Result<Vec<Tag>, AppError> {{ todo!() }}\n\
                 pub async fn add_label(store: &Store, {parent}: &str, tag_id: &str) -> Result<bool, AppError> {{ todo!() }}\n\
                 pub async fn remove_label(store: &Store, {parent}: &str, tag_id: &str) -> Result<bool, AppError> {{ todo!() }}\n\
                 // {module}\n"
            )
        };
        let task = std::fs::read_to_string(api_dir.join("task.rs")).unwrap() + &ops("task", "task_id");
        write_synthetic_api(&api_dir, "task.rs", &task);
        write_synthetic_api(&api_dir, "crew.rs", &ops("crew", "crew_id"));
        if scoped {
            config.route_prefix = test_config_with_prefix(PathBuf::new()).route_prefix;
        }
        let http = generate_http(tmp.path(), config);
        let suffix = if scoped { "_scoped" } else { "" };
        for handler in ["crew_add_label", "crew_remove_label"] {
            let body = handler_body(&http, &format!("{handler}{suffix}"));
            assert!(body.contains("Ok(ontogen_jsonapi::response::no_content())"), "{handler}{suffix}: {body}");
            assert!(!body.contains("OntogenResultMeta"), "{handler}{suffix}: {body}");
        }
        for (handler, op) in [("relationship_post", "add_label"), ("relationship_delete", "remove_label")] {
            let body = handler_body(&http, &format!("ontogen_task_{handler}{suffix}"));
            assert_in_order(
                handler,
                &body,
                &[
                    &format!("task::{op}("),
                    ".await.map_err(ontogen_app_error)?;",
                    "Ok(ontogen_jsonapi::response::no_content())",
                ],
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON:API relationship and related routes (wire contract §9)
// ═══════════════════════════════════════════════════════════════════════════════

/// A resource module serving relationships has the relationship route
/// (`GET`, `PATCH`, `POST`, `DELETE`) and the related route (`GET`), each
/// with the method fallback that lists them, under the scope of its
/// `get_by_id`. Its junction ops have no route of their own.
#[test]
fn a_resource_with_relationships_serves_the_relationship_and_related_routes() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let http = generate_http(tmp.path(), ops_fixture(tmp.path(), scoped));
        let flat = compact(&http);
        let (base, suffix) = if scoped { ("/api/projects/{project_id}", "_scoped") } else { ("/api", "") };
        assert!(
            flat.contains(&compact(&format!(
                ".route(\"{base}/tasks/{{id}}/relationships/{{rel}}\", axum::routing::get(ontogen_task_relationship_get{suffix})\
             .patch(ontogen_task_relationship_patch{suffix}).post(ontogen_task_relationship_post{suffix})\
             .delete(ontogen_task_relationship_delete{suffix})\
             .fallback(ontogen_allow([OntogenMethod::GET, OntogenMethod::PATCH, OntogenMethod::POST, OntogenMethod::DELETE])))"
            ))),
            "{http}"
        );
        assert!(flat.contains(&compact(&format!(
            ".route(\"{base}/tasks/{{id}}/{{rel}}\", axum::routing::get(ontogen_task_related_get{suffix})\
             .fallback(ontogen_allow([OntogenMethod::GET])))"
        ))));
        // `epic` and `tag` have no relationship, so no such route.
        assert_eq!(http.matches("/relationships/{rel}\"").count(), 1, "{http}");
        assert!(!http.contains("ontogen_epic_relationship") && !http.contains("ontogen_tag_related"));

        // Each handler matches `{rel}` against every relationship, field
        // ones first, and answers any other name with `404`.
        let path = if scoped {
            "OntogenPath((ontogen_scope, id, rel)): OntogenPath<(uuid::Uuid, OntogenLookupKey, OntogenLookupKey)>"
        } else {
            "OntogenPath((id, rel)): OntogenPath<(OntogenLookupKey, OntogenLookupKey)>"
        };
        for handler in ["relationship_get", "related_get"] {
            assert_in_order(
                handler,
                &handler_body(&http, &format!("ontogen_task_{handler}{suffix}")),
                &[
                    path,
                    "OntogenRawQuery(ontogen_raw_query): OntogenRawQuery",
                    "match rel.as_str() {",
                    "Some(\"epic\") =>",
                    "Some(\"tags\") =>",
                    "Some(\"labels\") =>",
                    "_ => Err(ontogen_jsonapi::error::relationship_not_found(\"tasks\", &rel)),",
                ],
            );
        }
        for handler in ["relationship_patch", "relationship_post", "relationship_delete"] {
            let ty = if scoped {
                "(uuid::Uuid, OntogenLookupKey, OntogenLookupKey)"
            } else {
                "(OntogenLookupKey, OntogenLookupKey)"
            };
            assert_in_order(
                handler,
                &handler_body(&http, &format!("ontogen_task_{handler}{suffix}")),
                &[
                    "_: OntogenAcceptGuard,",
                    &format!("ontogen_path: Result<OntogenPath<{ty}>, OntogenErrorObject>,"),
                    "ontogen_query: Result<OntogenQuery<OntogenNoParams>, OntogenErrorObject>,",
                    "ontogen_body: OntogenBody",
                    "= ontogen_path?;",
                    "match rel.as_str() {",
                    "_ => Err(ontogen_jsonapi::error::relationship_not_found(\"tasks\", &rel)),",
                ],
            );
        }
        // The parent is read through the prefix when scoped.
        let read = if scoped {
            "ontogen_task_read_scoped(&ontogen_state, &ontogen_scope, &id).await?;"
        } else {
            "ontogen_task_read(&ontogen_state, &id).await?;"
        };
        assert!(flat.contains(&compact(read)), "{http}");
        let opens = if scoped {
            "let store = state.store_for(ontogen_scope).map_err(ontogen_internal_error)?;"
        } else {
            "let store = state.store().await.map_err(ontogen_internal_error)?;"
        };
        assert!(flat.contains(&compact(opens)), "{http}");
    }
}

/// Every relationship of a type whose module serves the relationship routes
/// carries their links; a junction relationship carries the links alone
/// (§5.4, §9.1). It is a member name of the type, so a create or update
/// naming it is refused rather than unknown.
#[test]
fn every_relationship_links_its_routes() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));
    let flat = compact(&http);
    for (rel, data) in [
        (
            "epic",
            "OntogenLinkage::ToOne(entity.epic_id.as_ref().map(|id| OntogenResourceIdentifier::new(\"epics\", id.as_str())))",
        ),
        (
            "tags",
            "OntogenLinkage::ToMany(entity.tags.iter().map(|id| OntogenResourceIdentifier::new(\"tags\", id.as_str())).collect())",
        ),
    ] {
        assert!(flat.contains(&compact(&format!(
            ".with_relationship(\"{rel}\", OntogenRelationship::new(OntogenLinks::new(format!(\"{{self_link}}/relationships/{rel}\"))\
             .with_related(format!(\"{{self_link}}/{rel}\")), {data}))"
        ))), "{rel}:\n{http}");
    }
    assert!(flat.contains(&compact(
        ".with_relationship(\"labels\", OntogenRelationship::from_links(OntogenLinks::new(format!(\"{self_link}/relationships/labels\"))\
         .with_related(format!(\"{self_link}/labels\"))))"
    )));
    assert!(!flat.contains("OntogenRelationship::from_data("), "{http}");
    // Event frames drop the links and the junction relationship.
    assert!(
        flat.contains(&compact("event.json_data(ontogen_task_as_resource(entity, \"/api/tasks\").into_unlinked())"))
    );

    assert_in_order(
        "ontogen_task_request_fields",
        &compact(&http[http.find("fn ontogen_task_request_fields(").unwrap()..]),
        &[
            "ontogen_jsonapi::request::check_relationship_names(relationships, \"tasks\", &[\"epic\", \"tags\", \"labels\"])?;",
            "relationships.and_then(|r| r.get(\"epic\"))",
            "relationships.and_then(|r| r.get(\"tags\"))",
            "if relationships.is_some_and(|r| r.contains_key(\"labels\")) {",
            "return Err(ontogen_jsonapi::error::relationship_update_unsupported(\"tasks\", \"labels\", \"a create or update\")\
             .with_pointer(\"/data/relationships/labels\"));",
            "Ok((fields, linked))",
        ],
    );
}

/// The type without a served `get_by_id` serves no relationship route, so
/// its relationships carry linkage only: a server must serve every link it
/// emits.
#[test]
fn without_get_by_id_relationships_carry_linkage_only() {
    let tmp = tempfile::tempdir().unwrap();
    let config = resource_fixture(tmp.path(), true);
    let task: String = std::fs::read_to_string(config.api_dir.join("task.rs"))
        .unwrap()
        .lines()
        .filter(|l| !l.contains("fn get_by_id("))
        .map(|l| format!("{l}\n"))
        .collect();
    write_synthetic_api(&config.api_dir, "task.rs", &task);
    let http = generate_http(tmp.path(), config);
    assert!(!http.contains("/relationships/{rel}"), "{http}");
    assert!(!http.contains("ontogen_task_rel") && !http.contains("OntogenLinks::new(format!(\"{self_link}"), "{http}");
    assert!(compact(&http).contains(&compact(
        ".with_relationship(\"epic\", OntogenRelationship::from_data(OntogenLinkage::ToOne(entity.epic_id.as_ref()\
         .map(|id| OntogenResourceIdentifier::new(\"epics\", id.as_str())))))"
    )));
}

/// The arms of a relationship handler, in §13.2 order: the query (step 5),
/// a refusal (6), the body (7), the parent and each linked resource (8),
/// then the write (9).
#[test]
fn relationship_handlers_check_in_the_contract_order() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));
    let arm = |handler: &str, rel: &str| {
        let body = handler_body(&http, handler);
        let at = body.find(&compact(&format!("Some(\"{rel}\") =>"))).unwrap_or_else(|| panic!("{rel} in {body}"));
        let arm = &body[at..];
        arm[..arm.find("Some(\"").filter(|&i| i > 0).or_else(|| arm.find("_=>")).unwrap()].to_string()
    };

    assert_in_order(
        "PATCH epic",
        &arm("ontogen_task_relationship_patch", "epic"),
        &[
            "ontogen_query?;",
            "let ontogen_bytes = ontogen_body.into_bytes()?;",
            "ontogen_jsonapi::request::to_one(&ontogen_jsonapi::request::parse_relationship(&ontogen_bytes)?, \"\", \"epics\", true)?",
            ".map(|ontogen_member| OntogenLinkedId { id: ontogen_member, pointer: \"/data\".to_owned() });",
            "let ontogen_entity = ontogen_task_read(&ontogen_state, &id).await?;",
            "ontogen_epic_check_ids(&ontogen_state, ontogen_linked.as_slice()).await?;",
            "ontogen_task_write_field(&ontogen_state, &ontogen_entity.id, \"epic_id\", \
             ontogen_linked.map(|ontogen_member| ontogen_member.id).into()).await?;",
            "Ok(ontogen_jsonapi::response::no_content())",
        ],
    );
    assert_in_order(
        "PATCH tags",
        &arm("ontogen_task_relationship_patch", "tags"),
        &[
            "ontogen_jsonapi::request::to_many_linked(&ontogen_jsonapi::request::parse_relationship(&ontogen_bytes)?, \"\", \"tags\", None)?;",
            "ontogen_tag_check_ids(&ontogen_state, &ontogen_linked).await?;",
            "\"tags\", ontogen_linked.into_iter().map(|ontogen_member| ontogen_member.id).collect()",
        ],
    );
    assert_in_order(
        "POST tags",
        &arm("ontogen_task_relationship_post", "tags"),
        &[
            "ontogen_jsonapi::request::to_many_linked(&ontogen_jsonapi::request::parse_relationship(&ontogen_bytes)?, \"\", \"tags\", Some(1))?;",
            "ontogen_task_read(&ontogen_state, &id).await?;",
            "ontogen_tag_check_ids(&ontogen_state, &ontogen_linked).await?;",
            "if let Some(ontogen_ids) = ontogen_added(&ontogen_entity.tags, &ontogen_linked) {",
        ],
    );
    // `DELETE` never checks its target.
    let delete = arm("ontogen_task_relationship_delete", "tags");
    assert!(!delete.contains("check_ids"), "{delete}");
    assert!(
        delete
            .contains(&compact("if let Some(ontogen_ids) = ontogen_removed(&ontogen_entity.tags, &ontogen_linked) {"))
    );

    // A junction write reads the membership, then calls the op only when it
    // must.
    assert_in_order(
        "POST labels",
        &arm("ontogen_task_relationship_post", "labels"),
        &[
            "ontogen_tag_check_ids(&ontogen_state, &ontogen_linked).await?;",
            "if let Some(ontogen_child) = ontogen_linked.first() {",
            "let ontogen_store = ontogen_state.store().await.map_err(ontogen_internal_error)?;",
            "let ontogen_members = task::list_labels(&ontogen_store, &ontogen_entity.id).await.map_err(ontogen_app_error)?;",
            "if !ontogen_members.iter().any(|ontogen_member| ontogen_member.id == ontogen_child.id) {",
            "task::add_label(&ontogen_store, &ontogen_entity.id, &ontogen_child.id).await.map_err(ontogen_app_error)?;",
        ],
    );
    assert_in_order(
        "DELETE labels",
        &arm("ontogen_task_relationship_delete", "labels"),
        &[
            "if ontogen_members.iter().any(|ontogen_member| ontogen_member.id == ontogen_child.id) {",
            "task::remove_label(&ontogen_store, &ontogen_entity.id, &ontogen_child.id)",
        ],
    );

    // A paginated junction pages its members in memory, on both routes.
    for handler in ["ontogen_task_relationship_get", "ontogen_task_related_get"] {
        assert_in_order(
            handler,
            &arm(handler, "labels"),
            &[
                "OntogenQueryParams::parse(ontogen_raw_query.as_deref(), &OntogenQuerySpec { page: true, ..OntogenQuerySpec::NONE })?;",
                "let (ontogen_offset, ontogen_limit) = ontogen_page(&ontogen_query, 20, 100)?;",
                "let ontogen_entity = ontogen_task_read(&ontogen_state, &id).await?;",
                "task::list_labels(&ontogen_store, &ontogen_entity.id)",
                ".skip(ontogen_offset as usize).take(ontogen_limit as usize)",
                "ontogen_jsonapi::links::pagination_links(&ontogen_self, &OntogenCanonicalQuery::new(), ontogen_offset, ontogen_limit, ontogen_total)",
                ".with_meta(OntogenPageMeta { total: ontogen_total, limit: ontogen_limit, offset: ontogen_offset",
            ],
        );
    }
    // A field relationship accepts no query parameter and is not paged.
    let epic = arm("ontogen_task_relationship_get", "epic");
    assert!(
        epic.starts_with(&compact(
            "Some(\"epic\") => { OntogenQueryParams::parse(ontogen_raw_query.as_deref(), &OntogenQuerySpec::NONE)?;"
        )),
        "{epic}"
    );
    // Related resources: the target's resource objects, a dangling id
    // skipped by the fetch.
    assert_in_order(
        "related epic",
        &arm("ontogen_task_related_get", "epic"),
        &[
            "let ontogen_related = ontogen_epic_fetch(&ontogen_state, ontogen_entity.epic_id.as_slice()).await?;",
            "ontogen_related.first().map(|ontogen_member| ontogen_epic_as_resource(ontogen_member, \"/api/epics\"));",
            "OntogenLinks::new(ontogen_self)",
        ],
    );
    assert!(compact(&http).contains(&compact("Err(crate::schema::AppError::EpicNotFound(..)) => {}")));
}

/// The `403`s decided from the route alone (§9's table, §13.2 step 6): a
/// to-one has no members to add or remove, no junction op replaces a set,
/// a junction without `remove_Y` removes nothing, and a module without
/// `update` writes no field relationship. A handler whose every arm refuses
/// reads neither the state nor the body, but still extracts the body.
#[test]
fn unsupported_relationship_writes_are_refused_from_the_route() {
    let tmp = tempfile::tempdir().unwrap();
    let config = resource_fixture(tmp.path(), true);
    let task: String = std::fs::read_to_string(config.api_dir.join("task.rs"))
        .unwrap()
        .lines()
        .filter(|l| !l.contains("fn update("))
        .map(|l| format!("{l}\n"))
        .collect::<String>()
        + "pub async fn list_labels(store: &Store, task_id: &str) -> Result<Vec<Tag>, AppError> { todo!() }\n\
           pub async fn add_label(store: &Store, task_id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }\n";
    write_synthetic_api(&config.api_dir, "task.rs", &task);
    let http = generate_http(tmp.path(), config);
    let refused = |handler: &str, rel: &str, method: &str| {
        let body = handler_body(&http, handler);
        let arm = compact(&format!(
            "Some(\"{rel}\") => {{ ontogen_query?; Err(ontogen_jsonapi::error::relationship_update_unsupported(\"tasks\", \"{rel}\", \"{method}\")) }}"
        ));
        assert!(body.contains(&arm), "{handler} {rel}: {body}");
    };
    for rel in ["epic", "tags", "labels"] {
        refused("ontogen_task_relationship_patch", rel, "PATCH");
    }
    for rel in ["epic", "tags"] {
        refused("ontogen_task_relationship_post", rel, "POST");
    }
    for rel in ["epic", "tags", "labels"] {
        refused("ontogen_task_relationship_delete", rel, "DELETE");
    }
    // Only the junction's `POST` writes.
    assert!(handler_body(&http, "ontogen_task_relationship_post").contains("task::add_label("));
    for handler in ["ontogen_task_relationship_patch", "ontogen_task_relationship_delete"] {
        let body = handler_body(&http, handler);
        assert!(
            body.starts_with(&compact(&format!(
                "async fn {handler}( _: OntogenAcceptGuard, ontogen_path: Result<OntogenPath<(OntogenLookupKey, OntogenLookupKey)>, OntogenErrorObject>, \
             ontogen_query: Result<OntogenQuery<OntogenNoParams>, OntogenErrorObject>, _: OntogenBody, ) -> Result<OntogenResponse, OntogenErrorObject> {{ \
             let OntogenPath((_, rel)) = ontogen_path?;"
            ))),
            "{body}"
        );
    }
    assert!(!http.contains("ontogen_task_write_field"), "{http}");
}

/// A junction op that cannot define a relationship fails generation (the
/// rules themselves are `ResourceModel::junctions`'s tests).
#[test]
fn a_junction_op_that_defines_no_relationship_fails_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    let task = std::fs::read_to_string(config.api_dir.join("task.rs")).unwrap()
        + "pub async fn add_label(store: &Store, task_id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }\n";
    write_synthetic_api(&config.api_dir, "task.rs", &task);
    config.generators = vec![ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") }];
    let err = crate::servers::generate_transport(&config).unwrap_err();
    assert!(err.contains("`task::add_label` is a junction op") && err.contains("list_labels"), "{err}");
}

#[test]
fn server_metadata_routes_every_op_where_the_generator_does() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let config = ops_fixture(tmp.path(), scoped);
        let http = generate_http(tmp.path(), config.clone());
        let modules = crate::servers::generate_transport(&config).unwrap();
        let meta = crate::servers::extract_server_metadata(&modules, &config);
        let flat = compact(&http);
        for route in meta.http_routes.iter() {
            let handler = &route.handler_name;
            let method = route.method.to_ascii_lowercase();
            let needle = format!("{method}({handler})");
            let registered = flat.split(".route(\"").skip(1).any(|r| {
                let (template, handlers) = r.split_once('"').unwrap();
                template == route.path && handlers.contains(&needle)
            });
            assert!(registered, "{} {} ({handler}) is not a generated route:\n{http}", route.method, route.path);
            assert_ne!(route.method, "PUT", "no route is PUT");
        }
        let mut registrations: Vec<(&str, &str)> =
            meta.http_routes.iter().map(|r| (r.method.as_str(), r.path.as_str())).collect();
        registrations.sort_unstable();
        let rows = registrations.len();
        registrations.dedup();
        assert_eq!(registrations.len(), rows, "one row per method of each route:\n{:#?}", meta.http_routes);
    }
}

/// A resource module serving relationships reports its relationship and
/// related routes with their `{rel}` capture, one row per method, under the
/// scope of its `get_by_id` and named for the handlers serving them; its
/// junction ops, reached only through those routes, have no rows of their
/// own. A junction op outside a resource module keeps its own route, the
/// same scoped or not.
#[test]
fn server_metadata_reports_relationship_routes_with_their_rel_template() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let config = ops_fixture(tmp.path(), scoped);
        let epic = std::fs::read_to_string(config.api_dir.join("epic.rs")).unwrap()
            + "pub async fn list_tags(store: &Store, epic_id: &str) -> Result<Vec<String>, AppError> { todo!() }\n\
               pub async fn add_tag(store: &Store, epic_id: &str, tag_id: &str) -> Result<(), AppError> { todo!() }\n";
        write_synthetic_api(&config.api_dir, "epic.rs", &epic);
        let modules = crate::servers::generate_transport(&config).unwrap();
        let meta = crate::servers::extract_server_metadata(&modules, &config);
        let prefix = if scoped { "/api/projects/{project_id}" } else { "/api" };
        let suffix = if scoped { "_scoped" } else { "" };
        let rows = |module: &str| -> Vec<(String, String, String)> {
            meta.http_routes
                .iter()
                .filter(|r| r.module_name == module && r.path.contains("{rel}"))
                .map(|r| (r.method.clone(), r.path.clone(), r.handler_name.clone()))
                .collect()
        };
        for (module, plural) in [("task", "tasks"), ("epic", "epics")] {
            let row = |method: &str, path: &str, kind: &str| {
                (method.to_string(), format!("{prefix}/{plural}{path}"), format!("ontogen_{module}_{kind}{suffix}"))
            };
            assert_eq!(
                rows(module),
                [
                    row("GET", "/{id}/relationships/{rel}", "relationship_get"),
                    row("PATCH", "/{id}/relationships/{rel}", "relationship_patch"),
                    row("POST", "/{id}/relationships/{rel}", "relationship_post"),
                    row("DELETE", "/{id}/relationships/{rel}", "relationship_delete"),
                    row("GET", "/{id}/{rel}", "related_get"),
                ],
                "{module}"
            );
        }
        assert!(rows("tag").is_empty(), "a type with no relationship serves no relationship routes");
        for command in ["task_list_labels", "task_add_label", "task_remove_label", "epic_list_tags", "epic_add_tag"] {
            assert!(
                meta.http_routes.iter().all(|r| r.handler_name.trim_end_matches("_scoped") != command),
                "`{command}` has no route of its own:\n{:#?}",
                meta.http_routes
            );
        }
        let own = |command: &str| {
            let r = meta.http_routes.iter().find(|r| r.handler_name == format!("{command}{suffix}")).unwrap();
            (r.method.as_str(), r.path.clone())
        };
        assert_eq!(own("workout_list_labels"), ("GET", format!("{prefix}/workouts/{{parent_id}}/labels")));
        assert_eq!(
            own("workout_remove_label"),
            ("DELETE", format!("{prefix}/workouts/{{parent_id}}/labels/{{child_id}}"))
        );
    }
}

/// `module.rs` with `source`, in a fresh API dir, run through both
/// `gen_servers` with an Axum server and `gen_clients` with an HTTP client,
/// each of which must fail.
fn both_pipelines(source: &str, module: &str) -> (String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let (server, client) = pipelines(tmp.path(), source, module, true);
    (server.unwrap_err(), client.unwrap_err())
}

/// `module.rs` with `source` under `root`, run through `gen_servers` and
/// `gen_clients`: with an Axum server and an HTTP client when `http`, with
/// only the IPC and MCP servers and no client otherwise.
fn pipelines(
    root: &std::path::Path,
    source: &str,
    module: &str,
    http: bool,
) -> (Result<(), String>, Result<(), String>) {
    let api_dir = root.join("api");
    write_synthetic_api(&api_dir, &format!("{module}.rs"), source);
    let mut server_config = test_config(api_dir.clone());
    server_config.generators = if http {
        vec![ServerGenerator::HttpAxum { output: root.join("http.rs") }]
    } else {
        vec![
            ServerGenerator::TauriIpc { output: root.join("ipc.rs") },
            ServerGenerator::Mcp { output: root.join("mcp.rs") },
        ]
    };
    let server = crate::servers::generate_transport(&server_config).map(|_| ());
    let generators = if http {
        vec![ClientGenerator::HttpTs { output: root.join("client.ts"), bindings_path: root.join("bindings.ts") }]
    } else {
        vec![]
    };
    let clients = crate::ClientsConfig {
        api_dir,
        state_type: "AppState".to_string(),
        service_import_path: "crate::api::v1".to_string(),
        types_import_path: "crate::schema".to_string(),
        state_import: "crate::AppState".to_string(),
        naming: NamingConfig::default(),
        generators,
        ts_formatter: crate::TsFormatter::None,
        sse_route_overrides: HashMap::new(),
        ts_skip_commands: vec![],
        route_prefix: None,
        store_type: Some("Store".to_string()),
        store_import: Some("crate::store::Store".to_string()),
        pagination: None,
        label_overrides: HashMap::new(),
        pool_extra_roots: Vec::new(),
        pool_exclude_paths: Vec::new(),
        extra_surfaces: Vec::new(),
    };
    let client =
        crate::clients::generate(&crate::ir::SchemaOutput::default(), None, &[], &clients).map_err(|e| e.to_string());
    (server, client)
}

#[test]
fn a_collection_op_in_a_singleton_module_is_a_codegen_error() {
    for (op, source) in [
        ("list", "pub fn list(state: &AppState) -> Result<Vec<Setting>, anyhow::Error> { todo!() }\n"),
        ("delete", "pub fn delete(state: &AppState, id: &str) -> Result<(), anyhow::Error> { todo!() }\n"),
        (
            "add_tag",
            "pub fn add_tag(state: &AppState, id: &str, tag_id: &str) -> Result<(), anyhow::Error> { todo!() }\n",
        ),
    ] {
        let (server, client) = both_pipelines(
            &format!(
                "// ontogen:singleton\npub fn get_path(state: &AppState) -> Result<String, anyhow::Error> {{ todo!() }}\n{source}"
            ),
            "settings",
        );
        for err in [&server, &client] {
            assert!(err.contains(&format!("`settings::{op}`")), "{err}");
            assert!(err.contains("singleton"), "{err}");
        }
    }
    // A singleton's custom ops are fine.
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "settings.rs",
        "// ontogen:singleton\npub fn get_path(state: &AppState) -> Result<String, anyhow::Error> { todo!() }\n",
    );
    assert!(crate::servers::generate_transport(&test_config(api_dir)).is_ok());
}

#[test]
fn an_input_on_a_route_with_no_body_is_a_codegen_error() {
    for (op, source) in [
        // Forced GET.
        (
            "get_report",
            "#[ontogen::http::get]\npub fn get_report(state: &AppState, input: ReportInput) -> Result<String, anyhow::Error> { todo!() }\n",
        ),
        // `get_*` whose first argument fits the path.
        (
            "get_summary",
            "pub fn get_summary(state: &AppState, id: &str, input: SummaryInput) -> Result<String, anyhow::Error> { todo!() }\n",
        ),
    ] {
        let (server, client) = both_pipelines(source, "stats");
        for err in [&server, &client] {
            assert!(err.contains(&format!("`stats::{op}`")), "{err}");
            assert!(err.contains("without a request body"), "{err}");
        }
    }
}

/// The HTTP rules are HTTP's: IPC and MCP read every argument from one flat
/// payload, so a build with neither an Axum server nor an HTTP client takes
/// a collection op in a singleton module, and an input on an op HTTP would
/// serve without a body.
#[test]
fn an_ipc_only_build_does_not_apply_the_http_rules() {
    for source in [
        "// ontogen:singleton\npub fn delete(state: &AppState, id: &str) -> Result<(), anyhow::Error> { todo!() }\n",
        "#[ontogen::http::get]\npub fn get_report(state: &AppState, input: ReportInput) -> Result<String, anyhow::Error> { todo!() }\n",
        "pub fn get_by_id(state: &AppState, id: &str, verbose: Option<bool>) -> Result<String, anyhow::Error> { todo!() }\n",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let (server, client) = pipelines(tmp.path(), source, "settings", false);
        assert_eq!(server, Ok(()), "{source}");
        assert_eq!(client, Ok(()), "{source}");
    }
}

/// A CRUD-named op in a module with no schema entity is served at its
/// collection's route, which carries the route's own arguments only, and the
/// TypeScript clients call it with exactly those: an op that takes others is
/// a codegen error, not a route no client can call.
#[test]
fn an_entityless_crud_op_with_other_arguments_is_a_codegen_error() {
    for (op, source, wants) in [
        (
            "get_by_id",
            "pub fn get_by_id(state: &AppState, id: &str, verbose: Option<bool>) -> Result<String, anyhow::Error> { todo!() }\n",
            "passes `id` after",
        ),
        (
            "delete",
            "pub fn delete(state: &AppState) -> Result<(), anyhow::Error> { todo!() }\n",
            "the fn takes nothing",
        ),
        (
            "create",
            "pub fn create(state: &AppState, input: NewThing, note: String) -> Result<String, anyhow::Error> { todo!() }\n",
            "passes its input after",
        ),
        (
            "update",
            "pub fn update(state: &AppState, input: ThingPatch) -> Result<String, anyhow::Error> { todo!() }\n",
            "passes `id` and its input after",
        ),
    ] {
        let (server, client) = both_pipelines(source, "settings");
        for err in [&server, &client] {
            assert!(err.contains(&format!("`settings::{op}`")), "{err}");
            assert!(err.contains(wants), "{err}");
            assert!(err.contains("no schema entity"), "{err}");
        }
    }
    // The route's own arguments are fine, and a list may take a filter.
    let tmp = tempfile::tempdir().unwrap();
    let source = "pub fn get_by_id(state: &AppState, key: &str) -> Result<String, anyhow::Error> { todo!() }\n\
                  pub fn create(state: &AppState, input: NewThing) -> Result<String, anyhow::Error> { todo!() }\n\
                  pub fn update(state: &AppState, id: &str, patch: ThingPatch) -> Result<String, anyhow::Error> { todo!() }\n\
                  pub fn delete(state: &AppState, id: &str) -> Result<(), anyhow::Error> { todo!() }\n\
                  pub fn list(state: &AppState, kind: &str) -> Result<Vec<String>, anyhow::Error> { todo!() }\n";
    let (server, _) = pipelines(tmp.path(), source, "settings", true);
    assert_eq!(server, Ok(()));
}

/// An optional argument of a `GET` is one `opArg[…]` query-string value: a
/// type that needs more than one (a `Vec`, a map) is a codegen error, not a
/// route that refuses every request. A type that only contains the word
/// `Input` is not an input.
#[test]
fn an_op_arg_that_one_value_cannot_carry_is_a_codegen_error() {
    for ty in ["Vec<String>", "HashMap<String, String>", "(u32, u32)"] {
        let source = format!(
            "pub fn get_report(state: &AppState, tags: Option<{ty}>) -> Result<String, anyhow::Error> {{ todo!() }}\n"
        );
        let (server, client) = both_pipelines(&source, "stats");
        for err in [&server, &client] {
            assert!(err.contains("`stats::get_report`"), "{err}");
            assert!(err.contains("opArg[tags]"), "{err}");
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let source = "pub fn get_report(state: &AppState, mode: Option<InputMode>, label: Option<&str>, n: Option<u32>) \
                  -> Result<String, anyhow::Error> { todo!() }\n";
    let (server, _) = pipelines(tmp.path(), source, "stats", true);
    assert_eq!(server, Ok(()));
}

/// A list's filter is read from `filter[…]` (§7.3): every member of one
/// `*Query` struct, taken by value or by `&`, and each other filter argument from one
/// value. Anything else is a codegen error naming the argument, on the server
/// and the client alike.
#[test]
fn a_list_filter_the_filter_family_cannot_carry_is_a_codegen_error() {
    let list = |filter: &str| {
        format!("pub fn list(state: &AppState, {filter}) -> Result<Vec<String>, anyhow::Error> {{ todo!() }}\n")
    };
    for (filter, wants) in [
        (
            "a: ListAQuery, b: ListBQuery",
            &["takes two `*Query` filter structs, `a: ListAQuery` and `b: ListBQuery`", "merge them into one"][..],
        ),
        (
            "query: Option<ListThingsQuery>",
            &[
                "takes its filter struct as `query: Option<ListThingsQuery>`",
                "take it by value (`query: ListThingsQuery`) or borrowed (`query: &ListThingsQuery`)",
            ],
        ),
        ("query: Option<&ListThingsQuery>", &["as `query: Option<&ListThingsQuery>`", "take it by value"]),
        ("query: &mut ListThingsQuery", &["as `query: &mut ListThingsQuery`", "or borrowed"]),
        ("tags: Vec<String>", &["reads `tags: Vec<String>` from the query parameter `filter[tags]`"]),
        ("tags: Option<Vec<String>>", &["reads `tags: Option<Vec<String>>`", "`filter[tags]`"]),
        ("ids: &[String]", &["reads `ids: &[String]` from the query parameter `filter[ids]`"]),
        ("range: (u32, u32)", &["`filter[range]`", "take a type one value can carry"]),
        ("by: HashMap<String, String>", &["`filter[by]`"]),
        (
            "input: ThingFilterInput",
            &[
                "`settings::list` is served without a request body, so it cannot take `input: ThingFilterInput`",
                "`*Query` struct",
            ],
        ),
    ] {
        let (server, client) = both_pipelines(&list(filter), "settings");
        for err in [&server, &client] {
            assert!(err.contains("`settings::list`"), "{filter}: {err}");
            for want in wants {
                assert!(err.contains(want), "{filter}: {want}\n{err}");
            }
        }
    }
    // One value each: strings, numbers, bools and unit enums, required or
    // optional, owned or borrowed, beside one struct, owned or borrowed.
    for query in ["query: ListThingsQuery", "query: &ListThingsQuery"] {
        let tmp = tempfile::tempdir().unwrap();
        let source = list(&format!(
            "kind: &str, owner: String, label: Option<&str>, n: Option<u32>, done: bool, status: Option<Status>, \
             mode: InputMode, {query}"
        ));
        let (server, _) = pipelines(tmp.path(), &source, "settings", true);
        assert_eq!(server, Ok(()), "{query}");
    }
}

/// A borrowed filter struct is read as the struct it names and lent to both
/// `list` and `count`, so neither needs a clone of it.
#[test]
fn a_borrowed_filter_struct_is_lent_to_list_and_count() {
    let tmp = tempfile::tempdir().unwrap();
    let config = filtered_tag_fixture(tmp.path(), "query: &ListTagsQuery", true);
    let http = generate_http(tmp.path(), config.clone());

    assert!(http.contains("filter_fields: Some(ontogen_jsonapi::filter_fields::<ListTagsQuery>)"), "{http}");
    let list = handler_body(&http, "tag_list");
    assert_in_order(
        "tag_list",
        &list,
        &[
            "let ontogen_filter: ListTagsQuery = query.filter()?;",
            "let (offset, limit) = ontogen_page(&query, 20, 100)?;",
            "tag::list(&ontogen_store, &ontogen_filter, Some(u64::from(limit)), Some(u64::from(offset)))",
            "tag::count(&ontogen_store, &ontogen_filter)",
        ],
    );
    assert!(!list.contains(".clone()"), "a lent filter is not cloned:\n{list}");

    // Both TS clients send it as the struct it names, as they do an owned one.
    let mut clients = client_test_config(config.api_dir.clone());
    clients.resources = config.resources.clone();
    clients.pagination = config.pagination.clone();
    let modules = crate::servers::parse::scan_surfaces(&clients.surfaces(), &clients.state_type).unwrap().modules;
    let bindings = tmp.path().join("bindings.ts");
    std::fs::write(&bindings, "export type Tag = { id: string; title: string };\n").unwrap();
    let (transport, http_ts) = (tmp.path().join("transport.ts"), tmp.path().join("http.ts"));
    crate::clients::generators::transport::generate(&transport, &bindings, &modules, &clients);
    crate::clients::generators::ts_client::generate(&http_ts, &bindings, &modules, &clients);
    for ts in [transport, http_ts] {
        let ts = std::fs::read_to_string(ts).unwrap();
        assert!(
            ts.contains("async tagList(query?: ListTagsQuery, limit?: number, offset?: number)"),
            "the client sends the struct a `&ListTagsQuery` names:\n{ts}"
        );
        assert!(ts.contains("toQueryString({ filter: query, page: { offset, limit } })"), "{ts}");
    }
}

/// A schema entity is no one value either, as a bare filter of a resource
/// list or of any other.
#[test]
fn an_entity_as_a_bare_filter_is_a_codegen_error() {
    for filter in ["epic: &Epic", "epic: Option<Epic>"] {
        let tmp = tempfile::tempdir().unwrap();
        let config = filtered_tag_fixture(tmp.path(), filter, false);
        let mut config = config;
        config.generators = vec![ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") }];
        let err = crate::servers::generate_transport(&config).unwrap_err();
        assert!(err.contains("`tag::list` reads `epic: "), "{err}");
        assert!(err.contains("`filter[epic]`"), "{err}");
    }
}

/// A list that takes a filter is still served as its resource, so it must
/// return the entity's rows.
#[test]
fn a_filtered_resource_list_that_does_not_return_its_entity_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = filtered_tag_fixture(tmp.path(), "title: &str", false);
    let tag = std::fs::read_to_string(config.api_dir.join("tag.rs")).unwrap();
    write_synthetic_api(
        &config.api_dir,
        "tag.rs",
        &tag.replace("Result<Vec<Tag>, AppError>", "Result<Vec<TagRow>, AppError>"),
    );
    config.generators = vec![ServerGenerator::HttpAxum { output: tmp.path().join("http.rs") }];
    let err = crate::servers::generate_transport(&config).unwrap_err();
    assert!(err.contains("`tag::list` is served as the JSON:API resource `tags`"), "{err}");
    assert!(err.contains("must return `Vec<Tag>`"), "{err}");
}

/// A paginated module's `count` backs its list's total, so no server serves
/// it on its own: no client may call it either. The client pipeline drops it
/// with the same check the server pipeline does.
#[test]
fn a_paginated_count_is_neither_served_nor_called() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct Tag {\n    #[ontology(id)]\n    \
                  pub id: String,\n    pub name: String,\n}\n";
    let entities = crate::schema::parse::parse_schema_source(schema, std::path::Path::new("schema.rs")).unwrap();
    let api_dir = tmp.path().join("api");
    let api = crate::gen_api(
        &entities,
        &crate::ApiConfig {
            output_dir: api_dir.clone(),
            exclude: Vec::new(),
            scan_dirs: Vec::new(),
            state_type: "AppState".into(),
            store_type: Some("Store".into()),
            schema_module_path: "crate::schema".into(),
            paginated: vec!["tag".to_string()],
        },
    )
    .unwrap();
    assert!(std::fs::read_to_string(api_dir.join("tag.rs")).unwrap().contains("pub async fn count("));
    let pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });

    let out = tmp.path().join("out");
    let mut server = test_config(api_dir.clone());
    server.pagination = pagination.clone();
    server.resources = crate::resource::ResourceModel::build(&entities, &server.naming);
    server.generators = vec![
        ServerGenerator::HttpAxum { output: out.join("http.rs") },
        ServerGenerator::TauriIpc { output: out.join("ipc.rs") },
        ServerGenerator::Mcp { output: out.join("mcp.rs") },
    ];
    let modules = crate::servers::generate_transport(&server).unwrap();
    let meta = crate::servers::extract_server_metadata(&modules, &server);
    assert!(meta.ipc_commands.iter().all(|c| c.command_name != "tag_count"), "{:?}", meta.ipc_commands);
    assert!(meta.mcp_tools.iter().all(|t| t.tool_name != "tag_count"));
    assert!(meta.http_routes.iter().all(|r| !r.path.ends_with("/count")));
    for file in ["http.rs", "ipc.rs", "mcp.rs"] {
        let code = std::fs::read_to_string(out.join(file)).unwrap();
        assert!(!code.contains("fn tag_count"), "{file}:\n{code}");
    }

    let clients = crate::ClientsConfig {
        generators: vec![
            ClientGenerator::HttpTauriIpcSplit {
                output: out.join("transport.ts"),
                bindings_path: out.join("bindings.ts"),
            },
            ClientGenerator::HttpTs { output: out.join("client.ts"), bindings_path: out.join("bindings.ts") },
        ],
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination,
        ..crate::ClientsConfig::new(api_dir, "AppState", "crate::api", "crate::schema", "crate::AppState")
    };
    crate::gen_clients(&crate::schema::schema_of(&entities), Some(&api), &[], &clients).unwrap();
    for file in ["transport.ts", "client.ts"] {
        let ts = std::fs::read_to_string(out.join(file)).unwrap();
        assert!(ts.contains("tagList("), "{file}:\n{ts}");
        assert!(!ts.contains("tagCount") && !ts.contains("tag_count") && !ts.contains("/count"), "{file}:\n{ts}");
    }
}

#[test]
fn an_input_is_a_type_named_for_one() {
    for (ty, input) in [
        ("CreateTaskInput", true),
        ("&UpdateTaskInput", true),
        ("crate::schema::CreateTaskInput", true),
        ("Option<CreateTaskInput>", true),
        ("Option<&CreateTaskInput>", true),
        ("InputMode", false),
        ("Option<InputMode>", false),
        ("Vec<TaskInput>", false),
        ("String", false),
    ] {
        assert_eq!(param("p", ty).is_input(), input, "{ty}");
    }
    assert!(param("p", "Option<u32>").is_option());
    assert!(!param("p", "u32").is_option());
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON:API `include` (wire contract §7.5) and resources without `get_by_id`
// ═══════════════════════════════════════════════════════════════════════════════

/// [`resource_fixture`] with `module.rs` rewritten by `edit`, line by line:
/// `None` drops the line.
fn edited_resource_fixture(root: &std::path::Path, module: &str, edit: impl Fn(&str) -> Option<String>) -> Config {
    let config = resource_fixture(root, true);
    let path = config.api_dir.join(format!("{module}.rs"));
    let source: String =
        std::fs::read_to_string(&path).unwrap().lines().filter_map(edit).map(|l| format!("{l}\n")).collect();
    write_synthetic_api(&config.api_dir, &format!("{module}.rs"), &source);
    config
}

/// The emitted fn `name`, compacted.
fn emitted_fn(http: &str, name: &str) -> String {
    let code = &http[http.find(&format!("fn {name}")).unwrap_or_else(|| panic!("no fn {name} in:\n{http}"))..];
    compact(&code[..code.find("\n}\n").unwrap()])
}

/// A list and a get read `include` after `sort` and before the page, ahead
/// of the store (§13.2 step 5), repeat it in their links, and answer
/// `included` when the request carried it. Every flavour of list does.
#[test]
fn a_list_and_a_get_read_include_in_the_contract_order() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let http = generate_http(tmp.path(), ops_fixture(tmp.path(), scoped));
        let (suffix, scope) = if scoped { ("_scoped", "&ontogen_scope, ") } else { ("", "") };
        let include = "let include = query.include_paths(\"tasks\", &[\"epic\", \"tags\"], &[\"labels\"])?;";
        let open = if scoped { "ontogen_state.store_for(&ontogen_scope)" } else { "ontogen_state.store().await" };
        assert_in_order(
            "task_list",
            &handler_body(&http, &format!("task_list{suffix}")),
            &[
                "let order = query.sort_order(\"tasks\")?;",
                include,
                "let (offset, limit) = ontogen_page(&query, 20, 100)?;",
                "let link_query = query.link_query(include.as_deref())?;",
                open,
                "let links = ontogen_jsonapi::links::pagination_links(collection, &link_query, offset, limit, total);",
                "let mut document = OntogenDocument::new(data, links).with_meta(OntogenPageMeta { total, limit, offset });",
                &format!(
                    "if let Some(paths) = &include {{ document = document.with_included(ontogen_task_included{suffix}(\
                     &ontogen_state, {scope}&items, paths).await?); }}"
                ),
                "Ok(ontogen_jsonapi::response::ok(&document))",
            ],
        );
        assert_in_order(
            "task_get_by_id",
            &handler_body(&http, &format!("task_get_by_id{suffix}")),
            &[
                "query: OntogenQuery<OntogenGetParams>",
                include,
                "let link_query = query.link_query(include.as_deref())?;",
                open,
                "let entity = task::get_by_id(&ontogen_store, ontogen_task_lookup_key(&id)?)",
                "let mut document = OntogenDocument::resource(ontogen_task_as_resource(&entity, collection), &link_query);",
                &format!(
                    "if let Some(paths) = &include {{ document = document.with_included(ontogen_task_included{suffix}(\
                     &ontogen_state, {scope}std::slice::from_ref(&entity), paths).await?); }}"
                ),
                "Ok(ontogen_jsonapi::response::ok(&document))",
            ],
        );
        // A filtered list reads `include` after its filter, and a type with
        // nothing to include still answers `include=` with `included: []`.
        assert_in_order(
            "epic_list",
            &handler_body(&http, &format!("epic_list{suffix}")),
            &[
                "let ontogen_filter_title = query.filter_member::<String>(\"title\")?;",
                "let order = query.sort_order(\"epics\")?;",
                "let include = query.include_paths(\"epics\", &[], &[])?;",
                "let (offset, limit) = ontogen_page(&query, 20, 100)?;",
                "let link_query = query.link_query(include.as_deref())?;",
                "if include.is_some() { document = document.with_included(Vec::new()); }",
            ],
        );
        assert!(!http.contains("ontogen_epic_included"), "nothing to include, no helper:\n{http}");
        assert!(!http.contains("refuse_include"), "{http}");
    }
}

/// An unpaginated list repeats `include` in its only link.
#[test]
fn an_unpaginated_list_repeats_include_in_its_self_link() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    config.pagination = None;
    for module in ["task", "epic", "tag"] {
        write_synthetic_api(&config.api_dir, &format!("{module}.rs"), &crud_module_source(module, "Store"));
    }
    let http = generate_http(tmp.path(), config);
    assert_in_order(
        "task_list",
        &handler_body(&http, "task_list"),
        &[
            "query: OntogenQuery<OntogenListParams>",
            "let include = query.include_paths(\"tasks\", &[\"epic\", \"tags\"], &[])?;",
            "let link_query = query.link_query(include.as_deref())?;",
            "let items = task::list(&ontogen_store).await",
            "let mut document = OntogenDocument::new(data, OntogenLinks::new(link_query.href(collection)));",
            "document.with_included(ontogen_task_included(&ontogen_state, &items, paths).await?);",
        ],
    );
}

/// The include helper fetches each path's resources with the target's
/// `get_by_id`, through the `Fetch` helper the related links use, and
/// builds each as the target's own resource object.
#[test]
fn the_include_helper_reads_each_path_through_its_targets_get_by_id() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));
    assert_in_order(
        "ontogen_task_included",
        &emitted_fn(&http, "ontogen_task_included("),
        &[
            "state: &AppState, entities: &[Task], paths: &[&str], ) -> Result<Vec<OntogenAnyResource>, OntogenErrorObject> {",
            "let mut included = OntogenIncluded::new(\"tasks\", entities.iter().map(|entity| entity.id.as_str()));",
            "for path in paths { match *path {",
            "\"epic\" => { let ids = included.new_ids(\"epics\", entities.iter().filter_map(|entity| \
             entity.epic_id.as_deref()));",
            "let collection = \"/api/epics\";",
            "for related in ontogen_epic_fetch(state, &ids).await? { included.push(ontogen_epic_as_resource(&related, \
             collection))?; }",
            "\"tags\" => { let ids = included.new_ids(\"tags\", entities.iter().flat_map(|entity| \
             entity.tags.iter().map(String::as_str)));",
            "for related in ontogen_tag_fetch(state, &ids).await? { included.push(ontogen_tag_as_resource(&related, \
             collection))?; }",
            "_ => {}",
            "Ok(included.finish())",
        ],
    );
    // The junction relationship has no arm: it has no linkage to include.
    assert!(!emitted_fn(&http, "ontogen_task_included(").contains("labels"), "{http}");
    // One `Fetch` helper per target, shared with the related links.
    for helper in ["async fn ontogen_epic_fetch(", "async fn ontogen_tag_fetch("] {
        assert_eq!(http.matches(helper).count(), 1, "{helper}:\n{http}");
    }
    // The scoped helper reads under the prefix and links there.
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), true));
    assert_in_order(
        "ontogen_task_included_scoped",
        &emitted_fn(&http, "ontogen_task_included_scoped("),
        &[
            "state: &AppState, ontogen_scope: &uuid::Uuid, entities: &[Task], paths: &[&str])",
            "let collection = &format!(\"/api/projects/{}/epics\", ontogen_jsonapi::links::encode_path_segment(&ontogen_scope.to_string()));",
            "for related in ontogen_epic_fetch_scoped(state, ontogen_scope, &ids).await? {",
        ],
    );
    assert!(!http.contains("async fn ontogen_task_included("), "no unscoped handler includes:\n{http}");
    assert_eq!(http.matches("async fn ontogen_epic_fetch_scoped(").count(), 1, "{http}");
}

/// Under a route prefix, `task`'s handlers sit in one scope and `epic`'s
/// `get_by_id` takes the store, so it is read under the prefix, while
/// `tag`'s takes the state and is read outside it. `task` serves `list`,
/// and `get_by_id` and the writes when `serves_get`.
fn mixed_scope_fixture(root: &std::path::Path, task_takes_store: bool, serves_get: bool) -> Config {
    let mut config = edited_resource_fixture(root, "task", |line| {
        let keep = serves_get || line.contains("fn list(") || line.contains("fn count(");
        let line = if task_takes_store { line.to_string() } else { line.replace("store: &Store", "state: &AppState") };
        keep.then_some(line)
    });
    let tag = app_error_crud_source("tag").replace("store: &Store", "state: &AppState");
    write_synthetic_api(&config.api_dir, "tag.rs", &tag);
    config.route_prefix = test_config_with_prefix(PathBuf::new()).route_prefix;
    config
}

/// An unscoped handler cannot include a type read under the prefix: it
/// cannot open that store, and the resource's links would name routes
/// served only under the prefix. Including it is `invalid_include_path`,
/// not a build error; the type stays named in the detail.
#[test]
fn an_unscoped_list_cannot_include_a_type_read_under_the_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), mixed_scope_fixture(tmp.path(), false, false));
    let list = handler_body(&http, "task_list");
    assert!(
        list.contains(&compact("let include = query.include_paths(\"tasks\", &[\"tags\"], &[\"epic\"])?;")),
        "{list}"
    );
    assert!(
        list.contains(&compact("document.with_included(ontogen_task_included(&ontogen_state, &items, paths).await?);")),
        "{list}"
    );
    let helper = emitted_fn(&http, "ontogen_task_included(");
    assert!(helper.contains(&compact("state: &AppState, entities: &[Task], paths: &[&str]")), "{helper}");
    assert!(!helper.contains("\"epic\"") && !helper.contains("ontogen_scope"), "{helper}");
    assert!(helper.contains(&compact("let collection = \"/api/tags\";")), "{helper}");
    // A lone path is compared rather than matched.
    assert!(helper.contains(&compact("for path in paths { if *path == \"tags\" {")), "{helper}");
    assert_eq!(http.matches("async fn ontogen_tag_fetch(").count(), 1, "{http}");
    assert!(!http.contains("fn ontogen_epic_fetch"), "no handler reads epics through a fetch:\n{http}");
}

/// A scoped handler includes a type read outside the prefix with the state,
/// at its unscoped collection, and one read under it with the scope.
#[test]
fn a_scoped_handler_includes_types_read_on_either_side_of_the_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), mixed_scope_fixture(tmp.path(), true, true));
    assert!(
        handler_body(&http, "task_get_by_id_scoped")
            .contains(&compact("let include = query.include_paths(\"tasks\", &[\"epic\", \"tags\"], &[])?;"))
    );
    assert_in_order(
        "ontogen_task_included_scoped",
        &emitted_fn(&http, "ontogen_task_included_scoped("),
        &[
            "state: &AppState, ontogen_scope: &uuid::Uuid, entities: &[Task], paths: &[&str])",
            "\"epic\" =>",
            "let collection = &format!(\"/api/projects/{}/epics\", ontogen_jsonapi::links::encode_path_segment(&ontogen_scope.to_string()));",
            "for related in ontogen_epic_fetch_scoped(state, ontogen_scope, &ids).await? {",
            "\"tags\" =>",
            "let collection = \"/api/tags\";",
            "for related in ontogen_tag_fetch(state, &ids).await? {",
        ],
    );
    // A helper that reads nothing under the prefix takes no scope.
    let tmp = tempfile::tempdir().unwrap();
    let config = mixed_scope_fixture(tmp.path(), true, true);
    let epic = app_error_crud_source("epic").replace("store: &Store", "state: &AppState");
    write_synthetic_api(&config.api_dir, "epic.rs", &epic);
    let http = generate_http(tmp.path(), config);
    assert!(
        emitted_fn(&http, "ontogen_task_included_scoped(")
            .contains(&compact("(state: &AppState, entities: &[Task], paths: &[&str])")),
        "{http}"
    );
    assert!(
        handler_body(&http, "task_list_scoped")
            .contains(&compact("ontogen_task_included_scoped(&ontogen_state, &items, paths).await?")),
        "{http}"
    );
}

/// A resource module that serves no `get_by_id` names no URL for its
/// resources: they have no `links`, a create answers `201` without
/// `Location`, and a create or update document has no top-level links. Its
/// list keeps its links, since the list itself serves them, and it can
/// still include the types it links.
#[test]
fn without_get_by_id_a_resource_has_no_links_and_no_location() {
    let tmp = tempfile::tempdir().unwrap();
    let config = edited_resource_fixture(tmp.path(), "task", |l| (!l.contains("fn get_by_id(")).then(|| l.to_string()));
    let http = generate_http(tmp.path(), config);

    let builder = emitted_fn(&http, "ontogen_task_as_resource");
    assert!(
        builder.starts_with(&compact(
            "fn ontogen_task_as_resource<'a>(entity: &'a Task) -> OntogenResourceObject<OntogenTaskResourceAttributes<'a>> { \
             OntogenResourceObject::without_links(\"tasks\", entity.id.clone(), OntogenTaskResourceAttributes(entity))"
        )),
        "{builder}"
    );
    assert!(!builder.contains("self_link") && !builder.contains("OntogenLinks::new"), "{builder}");
    assert!(builder.contains("OntogenRelationship::from_data("), "{builder}");

    // The create reads `Location` from the resource as a linked type's does,
    // and finds none there.
    let create = handler_body(&http, "task_create");
    assert!(
        create.contains(&compact(
            "let document = OntogenDocument::resource(ontogen_task_as_resource(&entity), &OntogenCanonicalQuery::new()); \
             let location = document.data().and_then(OntogenResourceObject::links).map(OntogenLinks::self_link); \
             Ok(ontogen_jsonapi::response::created(location, &document))"
        )),
        "{create}"
    );
    assert!(
        handler_body(&http, "task_update").contains(&compact(
            "Ok(ontogen_jsonapi::response::ok(&OntogenDocument::resource(ontogen_task_as_resource(&entity), &OntogenCanonicalQuery::new())))"
        )),
        "{http}"
    );
    assert_in_order(
        "task_list",
        &handler_body(&http, "task_list"),
        &[
            "let include = query.include_paths(\"tasks\", &[\"epic\", \"tags\"], &[])?;",
            "let data: Vec<_> = items.iter().map(ontogen_task_as_resource).collect();",
            "let links = ontogen_jsonapi::links::pagination_links(collection, &link_query, offset, limit, total);",
            "document.with_included(ontogen_task_included(&ontogen_state, &items, paths).await?);",
        ],
    );
    // No relationship route reads the targets, so `include` alone wants
    // their `Fetch` helpers, each once.
    for helper in ["async fn ontogen_epic_fetch(", "async fn ontogen_tag_fetch("] {
        assert_eq!(http.matches(helper).count(), 1, "{helper}:\n{http}");
    }
    assert!(!http.contains("task_get_by_id"), "{http}");

    // The types that serve `get_by_id` are unchanged.
    assert!(
        compact(&http).contains(&compact("fn ontogen_epic_as_resource<'a>(entity: &'a Epic, collection: &str)")),
        "{http}"
    );
    let epic_create = handler_body(&http, "epic_create");
    assert!(
        epic_create.contains(&compact(
            "let document = OntogenDocument::resource(ontogen_epic_as_resource(&entity, collection), &OntogenCanonicalQuery::new()); \
             let location = document.data().and_then(OntogenResourceObject::links).map(OntogenLinks::self_link); \
             Ok(ontogen_jsonapi::response::created(location, &document))"
        )),
        "{epic_create}"
    );
}

/// A prefix typed `String` by its full path is borrowed as `&str` like the
/// bare name: helpers take `ontogen_scope: &str` and pass it to
/// `encode_path_segment` as it is, while a handler, which owns the value
/// `Path` read, formats it.
#[test]
fn a_string_prefix_named_by_its_path_is_borrowed_as_str() {
    for ty in ["std::string::String", "::alloc::string::String"] {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = ops_fixture(tmp.path(), true);
        config.route_prefix.as_mut().unwrap().params[0].rust_type = ty.to_string();
        let http = generate_http(tmp.path(), config);
        let flat = compact(&http);

        assert_in_order(
            "ontogen_task_included_scoped",
            &emitted_fn(&http, "ontogen_task_included_scoped("),
            &[
                "state: &AppState, ontogen_scope: &str, entities: &[Task], paths: &[&str])",
                "let collection = &format!(\"/api/projects/{}/epics\", ontogen_jsonapi::links::encode_path_segment(ontogen_scope));",
                "for related in ontogen_epic_fetch_scoped(state, ontogen_scope, &ids).await? {",
            ],
        );
        for helper in [
            "ontogen_epic_fetch_scoped(state: &AppState, ontogen_scope: &str, ids: &[String])",
            "ontogen_task_read_scoped(state: &AppState, ontogen_scope: &str, id: &OntogenLookupKey)",
            "ontogen_task_check_linked_scoped(state: &AppState, ontogen_scope: &str, linked: &OntogenTaskLinkedIds)",
        ] {
            assert!(flat.contains(&compact(&format!("async fn {helper}"))), "{helper}:\n{http}");
        }
        assert!(!http.contains(&format!("&{ty}")), "no helper borrows the `String` itself:\n{http}");
        assert!(!http.contains("ontogen_jsonapi::links::encode_path_segment(&ontogen_scope)"), "{http}");

        let create = handler_body(&http, "task_create_scoped");
        assert!(
            create.contains(&compact(&format!("path_params: Result<OntogenPath<{ty}>, OntogenErrorObject>,"))),
            "{create}"
        );
        assert!(
            create.contains(&compact(
                "let collection = &format!(\"/api/projects/{}/tasks\", ontogen_jsonapi::links::encode_path_segment(&ontogen_scope.to_string()));"
            )),
            "{create}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Name safety: the HTTP server compiles whatever the schema names things
// ═══════════════════════════════════════════════════════════════════════════════

/// Each `use` binding of `source`: the path it imports and the name it binds.
fn use_bindings(source: &str) -> Vec<(String, String)> {
    fn walk(tree: &syn::UseTree, prefix: &str, out: &mut Vec<(String, String)>) {
        match tree {
            syn::UseTree::Path(p) => walk(&p.tree, &format!("{prefix}{}::", p.ident), out),
            syn::UseTree::Name(n) => out.push((format!("{prefix}{}", n.ident), n.ident.to_string())),
            syn::UseTree::Rename(r) => out.push((format!("{prefix}{}", r.ident), r.rename.to_string())),
            syn::UseTree::Group(g) => g.items.iter().for_each(|t| walk(t, prefix, out)),
            syn::UseTree::Glob(_) => {}
        }
    }
    let file = syn::parse_file(source).expect("generated file must parse");
    let mut out = Vec::new();
    for item in &file.items {
        if let syn::Item::Use(u) = item {
            walk(&u.tree, "", &mut out);
        }
    }
    out
}

/// The first segment of every path in an item that has no leading `::`:
/// each name the item reads from its scope rather than through a path.
#[derive(Default)]
struct BareNames(Vec<String>);

impl<'ast> syn::visit::Visit<'ast> for BareNames {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if path.leading_colon.is_none() {
            self.0.push(path.segments[0].ident.to_string());
        }
        syn::visit::visit_path(self, path);
    }
}

/// The rule the generated HTTP file keeps so that a consumer may name its
/// entities and API modules anything (§13.4): the only bare names it binds
/// besides the consumer's are `Ontogen`-prefixed imports and its own
/// `Ontogen…` types and `ontogen_…` helpers, so no runtime item is named
/// bare, and every helper it defines is prefixed. Its handlers keep their
/// IPC command names (`task_list`).
fn assert_http_names_nothing_bare(http: &str) {
    let bindings = use_bindings(http);
    for (path, name) in &bindings {
        assert!(
            path.starts_with("crate::") || name.starts_with("Ontogen"),
            "`{path}` is imported as `{name}`, which a consumer type could take:\n{http}"
        );
    }
    let file = syn::parse_file(http).unwrap();
    let routes = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == "entity_routes" => Some(quote::quote!(#f).to_string()),
            _ => None,
        })
        .expect("entity_routes is emitted");
    let is_handler = |name: &str| routes.contains(&format!("({name})"));
    let mut runtime: Vec<&str> =
        crate::servers::generators::http::RUNTIME_ITEMS.iter().flat_map(|(_, names)| names.iter().copied()).collect();
    runtime.extend([
        "Arc",
        "Infallible",
        "Serialize",
        "Deserialize",
        "EventFrame",
        "Router",
        "request",
        "response",
        "encode_path_segment",
        "pagination_links",
        "method_not_allowed",
        "relationship_not_found",
        "relationship_update_unsupported",
    ]);
    // A runtime item's name that the consumer's import binds names the
    // consumer's item.
    runtime.retain(|name| !bindings.iter().any(|(path, bound)| path.starts_with("crate::") && bound == name));
    for item in &file.items {
        let (kind, name) = match item {
            syn::Item::Use(_) => continue,
            syn::Item::Struct(s) => ("type", s.ident.to_string()),
            syn::Item::Type(t) => ("type", t.ident.to_string()),
            syn::Item::Fn(f) => ("fn", f.sig.ident.to_string()),
            syn::Item::Impl(_) => ("impl", String::new()),
            other => panic!("unexpected item {}", quote::quote!(#other)),
        };
        match kind {
            "type" => assert!(name.starts_with("Ontogen"), "the file defines `{name}`:\n{http}"),
            "fn" => assert!(
                name.starts_with("ontogen_") || name == "entity_routes" || is_handler(&name),
                "the file defines the helper `{name}`:\n{http}"
            ),
            _ => {}
        }
        let mut bare = BareNames::default();
        syn::visit::Visit::visit_item(&mut bare, item);
        if let Some(name) = runtime.iter().find(|name| bare.0.iter().any(|i| i == *name)) {
            panic!("`{name}` is named bare in:\n{}", quote::quote!(#item));
        }
    }
}

#[test]
fn the_http_server_names_no_runtime_item_bare() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let http = generate_http(tmp.path(), ops_fixture(tmp.path(), scoped));
        assert_http_names_nothing_bare(&http);
    }
}

/// Entities named after what the HTTP server imports from its runtime
/// (`Document`, `Response`, `State`, …) or after a keyword (`Match`, whose
/// module is `r#match`), relationships named after keywords (`match`, `ref`,
/// `loop`, `in`), and API fn parameters written as raw idents.
const HOSTILE_SCHEMA: &str = r#"
    #[derive(OntologyEntity)]
    #[ontology(entity)]
    pub struct Document {
        #[ontology(id)]
        pub id: String,
        pub title: String,
        #[ontology(relation(belongs_to, target = "Match"))]
        pub match_id: Option<String>,
        #[ontology(relation(belongs_to, target = "Request"))]
        pub ref_id: Option<String>,
        #[ontology(relation(many_to_many, target = "Response"))]
        pub r#loop: Vec<String>,
    }

    #[derive(OntologyEntity)]
    #[ontology(entity)]
    pub struct Request {
        #[ontology(id)]
        pub id: String,
        pub title: String,
        pub kind: Option<Kind>,
    }

    #[derive(OntologyEntity)]
    #[ontology(entity)]
    pub struct Response {
        #[ontology(id)]
        pub id: String,
        pub title: String,
        #[ontology(relation(belongs_to, target = "Response"))]
        pub r#in: Option<String>,
    }

    #[derive(OntologyEntity)]
    #[ontology(entity)]
    pub struct Match {
        #[ontology(id)]
        pub id: String,
        pub title: String,
    }
"#;

/// The other entities named after runtime items, served alike.
const HOSTILE_PLAIN: [&str; 7] = ["state", "method", "links", "relationship", "endpoint", "event", "value"];

/// A config serving [`HOSTILE_SCHEMA`] and [`HOSTILE_PLAIN`] with the CRUD
/// modules `gen_api` emits, `request`'s list filtered by a bare `r#type`,
/// an event op yielding `Document` filtered by `r#type`, and `lookup`'s
/// custom ops taking raw-ident arguments, with types under `crate::model`.
fn hostile_fixture(root: &std::path::Path, scoped: bool) -> Config {
    let api_dir = root.join("api");
    let mut schema = HOSTILE_SCHEMA.to_string();
    for module in HOSTILE_PLAIN {
        let entity = capitalize(module);
        schema.push_str(&format!(
            "#[derive(OntologyEntity)]\n#[ontology(entity)]\npub struct {entity} {{\n    #[ontology(id)]\n    pub id: \
             String,\n    pub title: String,\n}}\n"
        ));
    }
    for module in HOSTILE_PLAIN.iter().chain(&["document", "response", "match"]) {
        write_synthetic_api(&api_dir, &format!("{module}.rs"), &sorted_crud_source(module));
    }
    let filter = "r#type: Option<&str>";
    write_synthetic_api(
        &api_dir,
        "request.rs",
        &sorted_crud_source("request")
            .replace("list(store: &Store, order", &format!("list(store: &Store, {filter}, order"))
            .replace("count(store: &Store)", &format!("count(store: &Store, {filter})")),
    );
    let event = std::fs::read_to_string(api_dir.join("event.rs")).unwrap()
        + "pub fn document_changed(state: &AppState, r#type: Option<String>) -> \
           tokio::sync::broadcast::Receiver<Document> { todo!() }\n";
    write_synthetic_api(&api_dir, "event.rs", &event);
    write_synthetic_api(
        &api_dir,
        "lookup.rs",
        "pub async fn find_docs(store: &Store, r#type: Option<String>, r#in: Option<String>) -> \
         Result<Vec<Document>, AppError> { todo!() }\n\
         pub async fn get_kinds(store: &Store, r#type: Option<String>, r#in: Option<&str>) -> Result<Vec<Request>, \
         AppError> { todo!() }\n\
         pub async fn get_one(store: &Store, r#ref: &str) -> Result<Request, AppError> { todo!() }\n",
    );
    let schema_dir = root.join("schema");
    write_synthetic_api(&schema_dir, "mod.rs", "");

    let entities = crate::schema::parse::parse_schema_source(&schema, std::path::Path::new("schema.rs")).unwrap();
    let mut config = if scoped { test_config_with_prefix(api_dir) } else { test_config(api_dir) };
    config.types_import_path = "crate::model".to_string();
    config.resources = crate::resource::ResourceModel::build(&entities, &config.naming);
    config.error_map = crate::servers::error_map::scan(&schema_dir).unwrap();
    config.pagination = Some(crate::servers::PaginationConfig { default_limit: 20, max_limit: 100 });
    config
}

#[test]
fn entities_named_after_runtime_items_or_keywords_keep_their_names_over_http() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let http = generate_http(tmp.path(), hostile_fixture(tmp.path(), scoped));
        let flat = compact(&http);

        // Each name is bound once, and the hostile ones by the consumer.
        let bindings = use_bindings(&http);
        let mut names: Vec<&str> = bindings.iter().map(|(_, name)| name.as_str()).collect();
        names.sort_unstable();
        let unique = names.len();
        names.dedup();
        assert_eq!(names.len(), unique, "a name is bound twice:\n{http}");
        for entity in ["Document", "Request", "Response", "Match", "State", "Method", "Links", "Relationship", "Event"]
        {
            assert!(
                bindings.contains(&(format!("crate::model::{entity}"), entity.to_string())),
                "`{entity}` is the consumer's:\n{http}"
            );
        }
        for module in ["request", "response", "r#match", "state", "event"] {
            assert!(
                bindings.contains(&(format!("crate::api::v1::{module}"), module.to_string())),
                "`{module}` is the API module:\n{http}"
            );
        }
        assert_http_names_nothing_bare(&http);

        // `Match`'s module is a keyword, written raw wherever it is code.
        assert!(flat.contains("r#match::get_by_id("), "{http}");
        assert!(!flat.contains("(match::") && !flat.contains("=match::"), "{http}");

        // Relationships named after keywords are raw idents in Rust and bare
        // on the wire.
        assert!(
            flat.contains(&compact(
                "struct OntogenDocumentLinkedIds { r#match: Option<OntogenLinkedId>, r#ref: Option<OntogenLinkedId>, \
                 r#loop: Vec<OntogenLinkedId>, }"
            )),
            "{http}"
        );
        for read in [
            "linked.r#match = id.map(|id| OntogenLinkedId { id, pointer: \"/data/relationships/match/data\".to_owned() });",
            "linked.r#loop = ids;",
            "for linked in &linked.r#loop {",
            "if let Some(linked) = &linked.r#ref {",
        ] {
            assert!(flat.contains(&compact(read)), "{read}\n{http}");
        }
        assert!(flat.contains("&[(\"match_id\",\"match\"),(\"ref_id\",\"ref\"),(\"loop\",\"loop\")]"), "{http}");
        assert!(flat.contains("linked.r#in=id.map("), "{http}");

        // Arguments written as raw idents are named without `r#` on the wire.
        assert!(!http.contains("\"r#"), "a wire key keeps `r#`:\n{http}");
        let find_docs = handler_body(&http, &format!("lookup_find_docs{}", if scoped { "_scoped" } else { "" }));
        for step in [
            "ontogen_jsonapi::request::check_op_arg_names(&ontogen_args, &[\"type\", \"in\"])?;",
            "let r#type = ontogen_jsonapi::request::op_arg::<Option<String>>(&ontogen_args, \"type\", false)?;",
            "let r#in = ontogen_jsonapi::request::op_arg::<Option<String>>(&ontogen_args, \"in\", false)?;",
            "lookup::find_docs(&ontogen_store, r#type, r#in)",
        ] {
            assert!(find_docs.contains(&compact(step)), "{step}\n{find_docs}");
        }
        let get_kinds = handler_body(&http, &format!("lookup_get_kinds{}", if scoped { "_scoped" } else { "" }));
        assert_in_order(
            "lookup_get_kinds",
            &get_kinds,
            &[
                "let r#in = ontogen_query.op_arg::<String>(\"in\")?;",
                "let r#type = ontogen_query.op_arg::<String>(\"type\")?;",
                "lookup::get_kinds(&ontogen_store, r#type, r#in.as_deref())",
            ],
        );
        assert!(flat.contains("op_args:&[\"type\",\"in\"]"), "{http}");
        assert!(flat.contains("/{ref}\""), "a path parameter is named as on the wire:\n{http}");
        assert!(flat.contains("OntogenPath(r#ref)") || flat.contains(",r#ref))"), "{http}");
        let list = handler_body(&http, &format!("request_list{}", if scoped { "_scoped" } else { "" }));
        assert!(
            list.contains(&compact("let ontogen_filter_type = query.filter_member::<String>(\"type\")?;")),
            "{list}"
        );
        assert!(flat.contains("filter:&[\"type\"]"), "{http}");

        // An attribute's schema type is named through `types_import_path`.
        assert!(
            flat.contains(&compact(
                "ontogen_jsonapi::request::attribute::<Option<crate::model::Kind>>(attributes, \"kind\", false)"
            )),
            "{http}"
        );
        assert!(!http.contains("crate::schema::"), "{http}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sort (wire contract §7.4, §15; ADR 0006 §1)
// ═══════════════════════════════════════════════════════════════════════════════

/// The one file `generator` writes for `config`.
fn generate_one(root: &std::path::Path, mut config: Config, generator: fn(PathBuf) -> ServerGenerator) -> String {
    let output = root.join("out.rs");
    config.generators = vec![generator(output.clone())];
    crate::servers::generate_transport(&config).unwrap_or_else(|e| panic!("generate_transport failed: {e}"));
    std::fs::read_to_string(output).unwrap()
}

fn http_gen(output: PathBuf) -> ServerGenerator {
    ServerGenerator::HttpAxum { output }
}

fn ipc_gen(output: PathBuf) -> ServerGenerator {
    ServerGenerator::TauriIpc { output }
}

fn mcp_gen(output: PathBuf) -> ServerGenerator {
    ServerGenerator::Mcp { output }
}

/// [`resource_fixture`] unpaginated, with three sorted lists: `task`'s
/// takes only its order, `epic`'s is the generated CRUD list with a page no
/// surface paginates, and `tag`'s is hand-written, filtered, and names its
/// order by full paths. With `scoped`, a route prefix serves them under
/// `projects/{project_id}`.
fn unpaginated_sort_fixture(root: &std::path::Path, scoped: bool) -> Config {
    let mut config = resource_fixture(root, true);
    config.pagination = None;
    let without_count = |source: String| -> String {
        source.lines().filter(|l| !l.contains("fn count(")).map(|l| format!("{l}\n")).collect()
    };
    write_synthetic_api(
        &config.api_dir,
        "task.rs",
        &without_count(sorted_crud_source("task")).replace(
            "order: &[OrderBy<TaskSortField>], limit: Option<u64>, offset: Option<u64>",
            "order: &[OrderBy<TaskSortField>]",
        ),
    );
    write_synthetic_api(
        &config.api_dir,
        "tag.rs",
        &without_count(app_error_crud_source("tag")).replace(
            "store: &Store, limit: Option<u64>, offset: Option<u64>",
            "store: &Store, query: ListTagsQuery, \
             order: &[ontogen_core::order::OrderBy<crate::store::tag::TagSortField>]",
        ),
    );
    if scoped {
        config.route_prefix = test_config_with_prefix(PathBuf::new()).route_prefix;
    }
    config
}

/// A list that takes an order reads `sort` after its filter and before
/// `include` and the page (§13.2 step 5), and passes the order after the
/// filter, whatever path names its type, paginated or not, scoped or not.
#[test]
fn an_unpaginated_sorted_list_reads_sort_and_passes_the_order() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let http = generate_one(tmp.path(), unpaginated_sort_fixture(tmp.path(), scoped), http_gen);
        let suffix = if scoped { "_scoped" } else { "" };
        let open = if scoped { "ontogen_state.store_for(&ontogen_scope)" } else { "ontogen_state.store().await" };
        assert_in_order(
            "task_list",
            &handler_body(&http, &format!("task_list{suffix}")),
            &[
                "let order = query.sort_order(\"tasks\")?;",
                "let include = query.include_paths(",
                "let link_query = query.link_query(include.as_deref())?;",
                open,
                "let items = task::list(&ontogen_store, &order).await.map_err(ontogen_app_error)?;",
                "let mut document = OntogenDocument::new(data, OntogenLinks::new(link_query.href(collection)));",
            ],
        );
        assert_in_order(
            "epic_list",
            &handler_body(&http, &format!("epic_list{suffix}")),
            &[
                "let order = query.sort_order(\"epics\")?;",
                "let items = epic::list(&ontogen_store, &order, None, None).await.map_err(ontogen_app_error)?;",
            ],
        );
        assert_in_order(
            "tag_list",
            &handler_body(&http, &format!("tag_list{suffix}")),
            &[
                "let ontogen_filter: ListTagsQuery = query.filter()?;",
                "let order = query.sort_order(\"tags\")?;",
                "let include = query.include_paths(\"tags\", &[], &[])?;",
                "let items = tag::list(&ontogen_store, ontogen_filter, &order).await.map_err(ontogen_app_error)?;",
            ],
        );
        assert!(!http.contains("refuse_sort"), "every resource list takes an order:\n{http}");
        assert!(!http.contains("SortField") && !http.contains("OrderBy"), "the order's types are inferred:\n{http}");
        assert!(!http.contains("OntogenErrorCode"), "nothing names an error code of its own:\n{http}");
    }
}

/// A list whose API fn takes no order answers any `sort` with `400
/// invalid_sort_field` (§7.4), through `ontogen_refuse_sort`, which is emitted only
/// beside such a list.
#[test]
fn refuse_sort_is_emitted_for_and_only_for_a_list_that_takes_no_order() {
    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), ops_fixture(tmp.path(), false));
    assert_eq!(http.matches("fn ontogen_refuse_sort(").count(), 1, "{http}");
    assert!(http.contains("/// The answer to `sort` on a list whose API fn takes no order"), "{http}");
    assert!(handler_body(&http, "tag_list").contains(&compact("ontogen_refuse_sort(&query, \"tags\")?;")), "{http}");
    assert!(!handler_body(&http, "tag_list").contains("sort_order"), "{http}");
    for sorted in ["task_list", "epic_list"] {
        assert!(!handler_body(&http, sorted).contains("refuse_sort"), "{sorted}:\n{http}");
    }

    let tmp = tempfile::tempdir().unwrap();
    let http = generate_http(tmp.path(), resource_fixture(tmp.path(), true));
    assert!(!http.contains("refuse_sort"), "no list refuses `sort`, so no helper:\n{http}");
}

/// The error generating `files` over [`resource_fixture`], paginated or not,
/// with every transport: the order rules hold for each of them.
fn order_error(files: &[(&str, &str)], paginated: bool) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    if !paginated {
        config.pagination = None;
    }
    for (file, source) in files {
        write_synthetic_api(&config.api_dir, file, source);
    }
    for generator in [http_gen, ipc_gen, mcp_gen] {
        let mut config = config.clone();
        config.generators = vec![generator(tmp.path().join("out.rs"))];
        let err = crate::servers::generate_transport(&config).expect_err("the order is refused");
        assert!(err.contains("takes the order"), "every transport refuses it: {err}");
    }
    config.generators = vec![http_gen(tmp.path().join("out.rs"))];
    crate::servers::generate_transport(&config).unwrap_err()
}

const TASK_REST: &str = "\
pub async fn get_by_id(store: &Store, id: &str) -> Result<Task, AppError> { todo!() }
pub async fn create(store: &Store, input: CreateTaskInput) -> Result<Task, AppError> { todo!() }
pub async fn update(store: &Store, id: &str, input: UpdateTaskInput) -> Result<Task, AppError> { todo!() }
pub async fn delete(store: &Store, id: &str) -> Result<(), AppError> { todo!() }
";

/// `task.rs` with `list` taking `params` after the store and `count`
/// taking `count`, beside the other CRUD ops.
fn task_with_list(params: &str, count: &str) -> String {
    format!(
        "pub async fn list(store: &Store, {params}) -> Result<Vec<Task>, AppError> {{ todo!() }}\n\
         pub async fn count(store: &Store{count}) -> Result<u64, AppError> {{ todo!() }}\n{TASK_REST}"
    )
}

const PAGE: &str = "limit: Option<u64>, offset: Option<u64>";

#[test]
fn a_list_takes_one_order_at_most() {
    let task =
        task_with_list(&format!("order: &[OrderBy<TaskSortField>], again: &[OrderBy<TaskSortField>], {PAGE}"), "");
    assert_eq!(
        order_error(&[("task.rs", &task)], true),
        "ontogen: `task::list` takes the order `order: &[OrderBy<TaskSortField>]` and a second one, \
         `again: &[OrderBy<TaskSortField>]`; a list takes at most one order, which `sort` is read into"
    );
}

#[test]
fn only_a_list_takes_an_order() {
    let task = format!(
        "{}pub async fn find_open(store: &Store, order: &[OrderBy<TaskSortField>]) -> Result<Vec<Task>, AppError> \
         {{ todo!() }}\n",
        sorted_crud_source("task")
    );
    assert_eq!(
        order_error(&[("task.rs", &task)], true),
        "ontogen: `task::find_open` takes the order `order: &[OrderBy<TaskSortField>]`, but only a module's `list`, \
         served as its collection, reads `sort` into an order; remove the parameter"
    );
}

/// The order follows the filter (§7.3): last before the page, or last.
#[test]
fn the_order_is_the_last_parameter_before_the_page() {
    let paged = task_with_list(&format!("order: &[OrderBy<TaskSortField>], status: &str, {PAGE}"), ", status: &str");
    assert_eq!(
        order_error(&[("task.rs", &paged)], true),
        "ontogen: `task::list` takes the order `order: &[OrderBy<TaskSortField>]`, which must be its last parameter \
         before `limit` and `offset`, after the filter: a list takes the store, its filter, its order, then its page"
    );
    let unpaged = format!(
        "pub async fn list(store: &Store, order: &[OrderBy<TaskSortField>], status: &str) -> Result<Vec<Task>, \
         AppError> {{ todo!() }}\n{TASK_REST}"
    );
    assert!(
        order_error(&[("task.rs", &unpaged)], false)
            .contains("`order: &[OrderBy<TaskSortField>]`, which must be its last parameter, after the filter"),
    );
    // Where the filter comes first, the same lists generate.
    for (source, paginated) in [
        (task_with_list(&format!("status: &str, order: &[OrderBy<TaskSortField>], {PAGE}"), ", status: &str"), true),
        (
            unpaged.replace(
                "order: &[OrderBy<TaskSortField>], status: &str",
                "status: &str, order: &[OrderBy<TaskSortField>]",
            ),
            false,
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = resource_fixture(tmp.path(), true);
        if !paginated {
            config.pagination = None;
        }
        write_synthetic_api(&config.api_dir, "task.rs", &source);
        let http = generate_one(tmp.path(), config, http_gen);
        assert!(handler_body(&http, "task_list").contains(&compact("&ontogen_filter_status, &order")), "{http}");
    }
}

/// The order sorts the entity its module serves, by its `{Entity}SortField`.
#[test]
fn the_order_sorts_the_modules_own_entity() {
    for field in ["EpicSortField", "TaskOrder", "crate::store::epic::EpicSortField"] {
        let task = task_with_list(&format!("order: &[OrderBy<{field}>], {PAGE}"), "");
        assert_eq!(
            order_error(&[("task.rs", &task)], true),
            format!(
                "ontogen: `task::list` takes the order `order: &[OrderBy<{field}>]`, but the module `task` lists \
                 `Task`, whose sort fields are `TaskSortField`; take `&[OrderBy<TaskSortField>]`"
            ),
            "{field}"
        );
    }
}

/// A list in a module with no entity behind it is a custom op (§10.4),
/// which takes no `sort`.
#[test]
fn a_list_with_no_entity_behind_it_takes_no_order() {
    let report = app_error_crud_source("report")
        .replace("list(store: &Store, limit", "list(store: &Store, order: &[OrderBy<ReportSortField>], limit");
    assert_eq!(
        order_error(&[("report.rs", &report)], true),
        "ontogen: `report::list` takes the order `order: &[OrderBy<ReportSortField>]`, but the module `report` has no \
         schema entity behind it, so its `list` is served as a custom op, which cannot be sorted; remove the parameter"
    );
}

/// The IPC list command takes the sort keys as `sort` (§15), parses them
/// with the parser the other transports use before it opens the store, and
/// passes the order after the filter.
#[test]
fn an_ipc_list_takes_its_sort_keys_as_sort() {
    for scoped in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let ipc = compact(&generate_one(tmp.path(), ops_fixture(tmp.path(), scoped), ipc_gen));
        let parse = "let ontogen_order = ::ontogen_core::order::parse_sort(sort.unwrap_or_default())\
                     .map_err(|ontogen_e| ontogen_e.to_string())?;";
        let open = if scoped {
            "let ontogen_store = if let Some(ref ontogen_pid) = project_id"
        } else {
            "let ontogen_store ="
        };
        assert_in_order(
            "epic_list",
            &ipc[ipc.find(&compact("pub async fn epic_list(")).unwrap()..],
            &[
                "query: ListEpicsQuery, title: Option<String>, owner: String, sort: Option<Vec<String>>, \
                 limit: Option<u32>, offset: Option<u32>,",
                parse,
                open,
                "epic::list(&ontogen_store, query.clone(), title.as_deref(), &owner, &ontogen_order, \
                 Some(u64::from(ontogen_limit)), Some(u64::from(ontogen_offset)))",
                "epic::count(&ontogen_store, query, title.as_deref(), &owner)",
            ],
        );
        assert_in_order(
            "task_list",
            &ipc[ipc.find(&compact("pub async fn task_list(")).unwrap()..],
            &[
                "sort: Option<Vec<String>>, limit: Option<u32>,",
                parse,
                "task::list(&ontogen_store, &ontogen_order, Some(u64::from(ontogen_limit))",
                "task::count(&ontogen_store)",
            ],
        );
        // The order's types are the store's: inferred, never imported.
        assert!(!ipc.contains("SortField") && !ipc.contains("OrderBy"), "{ipc}");
        let tag = &ipc[ipc.find(&compact("pub async fn tag_list(")).unwrap()..];
        let tag = &tag[..tag.find("#[::tauri::command]").unwrap()];
        assert!(
            !tag.contains("sort") && !tag.contains("ontogen_order"),
            "a list with no order takes no `sort`:\n{tag}"
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let ipc = compact(&generate_one(tmp.path(), unpaginated_sort_fixture(tmp.path(), false), ipc_gen));
    for call in [
        "task::list(&ontogen_store, &ontogen_order)",
        "epic::list(&ontogen_store, &ontogen_order, None, None)",
        "tag::list(&ontogen_store, query, &ontogen_order)",
    ] {
        assert!(ipc.contains(&compact(call)), "{call} in:\n{ipc}");
    }
}

/// A sorted list's command takes its sort keys as `sort`, so a filter named
/// `sort` fails the build (§15); on a list with no order it is a filter
/// like any other.
#[test]
fn an_ipc_sorted_list_refuses_a_filter_named_sort() {
    let sorted = task_with_list(
        &format!("sort: Option<String>, order: &[OrderBy<TaskSortField>], {PAGE}"),
        ", sort: Option<String>",
    );
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    write_synthetic_api(&config.api_dir, "task.rs", &sorted);
    config.generators = vec![ipc_gen(tmp.path().join("ipc.rs"))];
    assert_eq!(
        crate::servers::generate_transport(&config).unwrap_err(),
        "ontogen: the IPC command `task_list` cannot be generated: `task::list` takes an argument named `sort`, which \
         the command takes under the IPC wire key `sort`, the key it uses for the list's sort keys, so the two would \
         collide. Rename the argument."
    );
    // HTTP reads it from `filter[sort]`, which `sort` cannot collide with.
    config.generators = vec![http_gen(tmp.path().join("http.rs"))];
    crate::servers::generate_transport(&config).unwrap_or_else(|e| panic!("HTTP: {e}"));

    let unsorted = task_with_list(&format!("sort: Option<String>, {PAGE}"), ", sort: Option<String>");
    write_synthetic_api(&config.api_dir, "task.rs", &unsorted);
    let ipc = generate_one(tmp.path(), config, ipc_gen);
    assert!(compact(&ipc).contains(&compact("pub async fn task_list( sort: Option<String>,")), "{ipc}");
}

/// A sorted list's own keys are taken by what Tauri reads them under, the
/// parameter's name camelCased: a filter `sort_` takes the sort keys'
/// `sort` and a filter `limit_` the page's `limit`, and so does a route
/// prefix parameter named `sort_`. Unsorted, a filter `sort_` is a filter
/// like any other.
#[test]
fn an_ipc_sorted_list_refuses_a_filter_or_prefix_that_camelcases_to_its_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    config.generators = vec![ipc_gen(tmp.path().join("ipc.rs"))];
    let refused = |config: &Config, filter: &str| {
        let task =
            task_with_list(&format!("{filter}, order: &[OrderBy<TaskSortField>], {PAGE}"), &format!(", {filter}"));
        write_synthetic_api(&config.api_dir, "task.rs", &task);
        crate::servers::generate_transport(config).unwrap_err()
    };
    assert_eq!(
        refused(&config, "sort_: &str"),
        "ontogen: the IPC command `task_list` cannot be generated: `task::list` takes an argument named `sort_`, \
         which the command takes under the IPC wire key `sort`, the key it uses for the list's sort keys, so the two \
         would collide. Rename the argument."
    );
    assert_eq!(
        refused(&config, "limit_: &str"),
        "ontogen: the IPC command `task_list` cannot be generated: `task::list` takes an argument named `limit_`, \
         which the command takes under the IPC wire key `limit`, the key it uses for the page's `limit` and `offset`, \
         so the two would collide. Rename the argument."
    );

    // Every sorted list collides with it; `epic`'s is the first.
    write_synthetic_api(&config.api_dir, "task.rs", &sorted_crud_source("task"));
    config.route_prefix = Some(RoutePrefix {
        segments: "projects/:sort_".to_string(),
        state_accessor: "store_for".to_string(),
        params: vec![PrefixParam {
            name: "sort_".to_string(),
            rust_type: "uuid::Uuid".to_string(),
            ts_type: "string".to_string(),
        }],
    });
    assert_eq!(
        crate::servers::generate_transport(&config).unwrap_err(),
        "ontogen: the IPC command `epic_list` cannot be generated: `epic::list` is called with the route prefix \
         parameter `sort_`, which the command takes under the IPC wire key `sort`, the key it uses for the list's \
         sort keys, so the two would collide. Rename the route prefix parameter."
    );
    config.route_prefix = None;

    let unsorted = task_with_list(&format!("sort_: &str, {PAGE}"), ", sort_: &str");
    write_synthetic_api(&config.api_dir, "task.rs", &unsorted);
    let ipc = generate_one(tmp.path(), config, ipc_gen);
    assert!(compact(&ipc).contains(&compact("pub async fn task_list( sort_: String,")), "{ipc}");
}

/// The MCP list tool advertises `sort` as an array of the entity's sort
/// keys (§15, ADR 0006 §1), checks arguments against that schema, reads the
/// argument strictly and parses it with the shared parser.
#[test]
fn an_mcp_list_tool_advertises_and_reads_its_sort_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let mcp = generate_one(tmp.path(), ops_fixture(tmp.path(), false), mcp_gen);
    let flat = compact(&mcp);
    assert_eq!(mcp.matches("fn with_sort_schema(").count(), 1, "{mcp}");
    assert!(
        flat.contains(&compact(
            "::serde_json::json!({ \"type\": \"array\", \"items\": { \"type\": \"string\", \"enum\": keys }, \"description\": \
             \"Sort keys, applied in order: a field name sorts ascending, and `-` before it descending. The id is \
             the final tie-break.\" })"
        )),
        "{mcp}"
    );
    assert!(
        flat.contains(&compact(
            "fn sort_arg(args: &::serde_json::Value) -> Result<Vec<&str>, String> { match args.get(\"sort\") { None | \
             Some(::serde_json::Value::Null) => Ok(Vec::new()), Some(::serde_json::Value::Array(keys)) => keys .iter() .map(|key| \
             key.as_str().ok_or_else(|| format!(\"Invalid sort: expected a string, got {key}\"))) .collect(), \
             Some(other) => Err(format!(\"Invalid sort: expected an array of strings, got {other}\")), } }"
        )),
        "{mcp}"
    );

    let tool = |name: &str| {
        let at = flat.find(&compact(&format!("name: \"{name}\""))).unwrap_or_else(|| panic!("no {name}"));
        let tool = &flat[at..];
        tool[..tool.find("McpToolDef{").unwrap_or(tool.len())].to_string()
    };
    let task = tool("task_list");
    let schema = "with_sort_schema( with_pagination_schema(schema_for::<EmptyInput>()), \
                  &[\"id\", \"-id\", \"title\", \"-title\", \"notes\", \"-notes\", \"done\", \"-done\"], )";
    assert_in_order(
        "task_list",
        &task,
        &[
            &format!("schema_fn: || {{ {schema} }},"),
            &format!("refuse_unknown_args( ontogen_args, {schema}, )?;"),
            "let ontogen_order = ::ontogen_core::order::parse_sort(sort_arg(ontogen_args)?).map_err(|e| e.to_string())?;",
            "let ontogen_store =",
            "task::list(&ontogen_store, &ontogen_order, Some(ontogen_limit), Some(ontogen_offset))",
            "task::count(&ontogen_store)",
        ],
    );
    // The `*Query` struct is read without `sort`, which the tool reads itself.
    assert_in_order(
        "epic_list",
        &tool("epic_list"),
        &[
            "&[\"id\", \"-id\", \"title\", \"-title\"]",
            "args_without(ontogen_args, &[\"title\", \"owner\", \"sort\", \"limit\", \"offset\"])",
            "let owner = required_str(ontogen_args, \"owner\")?;",
            "let ontogen_order =",
            "epic::list( &ontogen_store, ontogen_filter.clone(), title.as_deref(), owner, &ontogen_order,",
        ],
    );
    let tag = tool("tag_list");
    assert!(!tag.contains("sort"), "a list with no order has no `sort`:\n{tag}");
    assert!(!mcp.contains("SortField") && !mcp.contains("OrderBy"), "the order's types are inferred:\n{mcp}");

    // No list takes an order: no helpers.
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(&api_dir, "gadget.rs", &crud_module_source("gadget", "Store"));
    let mcp = generate_one(tmp.path(), test_config(api_dir), mcp_gen);
    assert!(!mcp.contains("sort"), "{mcp}");
}

/// The sort keys come from the store's sortable fields (ADR 0006 §2): the
/// id under `id`, schema enums and integer primitives included, the body,
/// relations and lists left out.
#[test]
fn an_mcp_list_tool_enumerates_the_entitys_sort_keys() {
    let source = r#"
        #[serde(rename_all = "kebab-case")]
        pub enum Phase { Draft, InReview }

        #[derive(OntologyEntity)]
        #[ontology(entity)]
        pub struct Paper {
            #[ontology(id)]
            pub slug: String,
            pub title: String,
            pub phase: Phase,
            pub pages: Option<u32>,
            pub score: f64,
            pub labels: Vec<String>,
            #[ontology(body)]
            pub body: String,
        }
    "#;
    let path = std::path::Path::new("schema.rs");
    let entities = crate::schema::parse::parse_schema_source(source, path).unwrap();
    let enums = crate::schema::parse::parse_schema_enums_source(source, path).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let api_dir = tmp.path().join("api");
    write_synthetic_api(
        &api_dir,
        "paper.rs",
        "pub async fn list(store: &Store, order: &[OrderBy<PaperSortField>]) -> Result<Vec<Paper>, AppError> { todo!() }\n",
    );
    let mut config = test_config(api_dir);
    config.resources = crate::resource::ResourceModel::build(&entities, &config.naming);
    config.enums = enums;
    let mcp = generate_one(tmp.path(), config, mcp_gen);
    assert!(
        compact(&mcp).contains(&compact(
            "schema_fn: || { with_sort_schema(schema_for::<EmptyInput>(), &[\"id\", \"-id\", \"title\", \"-title\", \
             \"phase\", \"-phase\", \"pages\", \"-pages\", \"score\", \"-score\"]) },"
        )),
        "{mcp}"
    );
}

/// A sorted list tool reads its sort keys from `sort`, so a filter named
/// `sort` fails the build; on a list with no order it is a filter like any
/// other. A `*Query` field serialized as `sort` is not known here: the
/// tool reads the struct without `sort` (see the test above).
#[test]
fn an_mcp_sorted_list_refuses_a_filter_named_sort() {
    let sorted = task_with_list(
        &format!("sort: Option<String>, order: &[OrderBy<TaskSortField>], {PAGE}"),
        ", sort: Option<String>",
    );
    let tmp = tempfile::tempdir().unwrap();
    let mut config = resource_fixture(tmp.path(), true);
    write_synthetic_api(&config.api_dir, "task.rs", &sorted);
    config.generators = vec![mcp_gen(tmp.path().join("mcp.rs"))];
    assert_eq!(
        crate::servers::generate_transport(&config).unwrap_err(),
        "ontogen: the MCP tool `task_list` cannot be generated: `task::list` takes a filter named `sort`, which is \
         the argument the tool itself reads its sort keys from, so the two would share one key. Rename the argument."
    );
    let unsorted = task_with_list(&format!("sort: Option<String>, {PAGE}"), ", sort: Option<String>");
    write_synthetic_api(&config.api_dir, "task.rs", &unsorted);
    let mcp = generate_one(tmp.path(), config, mcp_gen);
    assert!(mcp.contains("let sort: Option<String> = ontogen_args"), "{mcp}");
}
