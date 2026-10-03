//! The load-bearing invariant of ADR 0001, contract item 5: for the same
//! schema, everything ABOVE the store — `gen_api`, `gen_servers`,
//! `gen_clients` — is **byte-identical** between the SeaORM and markdown
//! backends. Two consumers in one workspace can share the entire transport
//! stack while their stores diverge.
//!
//! Enforced three ways, under every `IdStrategy` (the strategy reaches the
//! store of either backend through `StoreConfig`, and must change nothing
//! above it):
//! - `StoreOutput` method metadata is compared field-by-field (the fast
//!   unit-level guard — `collect_method_meta` must never branch on backend);
//! - the public signatures of each generated store module, `count_*`
//!   included, are compared, since the API layer calls them by name;
//! - the full downstream output trees (API with a paginated module, the
//!   HTTP, IPC and MCP servers, the TS client, bindings and admin registry)
//!   are diffed recursively, byte for byte.
//!
//! Plus a NEGATIVE CONTROL: a deliberately perturbed store output must make
//! the comparison fail — a parity check that cannot fail proves nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ontogen::clients::ClientGenerator;
use ontogen::ir::{Backend, IdStrategy, MarkdownIoOutput, StoreOutput};
use ontogen::servers::{NamingConfig, ServerGenerator};
use ontogen::{ApiConfig, ApiOutput, ClientsConfig, EntityDef, SchemaConfig, ServersConfig, StoreConfig};
use quote::ToTokens;

fn fixture_entities() -> Vec<EntityDef> {
    // The relation-complete pilot schema: belongs_to + self-referential
    // has_many (optional and required foreign keys) + many_to_many + body
    // fields.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/markdown-pilot/src/schema");
    ontogen::parse_schema(&SchemaConfig { schema_dir: dir }).expect("parse pilot schema").entities
}

/// Every strategy the pilot schema admits (each entity has a `title`).
fn strategies() -> [IdStrategy; 3] {
    [IdStrategy::Provided, IdStrategy::SlugFromField("title".into()), IdStrategy::Uuid]
}

fn markdown_backend(entities: &[EntityDef]) -> Backend {
    // Build the markdown metadata exactly as gen_markdown_io would, without
    // writing its files (this test only exercises the store/api layers).
    Backend::Markdown(MarkdownIoOutput {
        module_path: "crate::persistence::markdown::generated".into(),
        entities: entities
            .iter()
            .map(|e| ontogen::ir::MarkdownEntityMeta {
                entity_name: e.name.clone(),
                type_name: e.type_name.clone(),
                dir_segment: e.directory.clone(),
                body_field: e.body_field().map(|f| f.name.clone()),
                authoritative_m2m: e.junction_relations().map(|(f, _)| f.name.clone()).collect(),
            })
            .collect(),
    })
}

fn gen_store_with(entities: &[EntityDef], backend: Backend, id_strategy: &IdStrategy, out: &Path) -> StoreOutput {
    ontogen::gen_store(
        entities,
        &StoreConfig {
            output_dir: out.to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".into(),
            backend,
            wikilink_policy: None,
            id_strategy: id_strategy.clone(),
        },
    )
    .expect("gen_store failed")
}

fn gen_api_into(entities: &[EntityDef], out: &Path) -> ApiOutput {
    ontogen::gen_api(
        entities,
        &ApiConfig {
            output_dir: out.to_path_buf(),
            exclude: Vec::new(),
            scan_dirs: Vec::new(),
            state_type: "AppState".into(),
            store_type: Some("Store".into()),
            schema_module_path: "crate::schema".into(),
            // Paged lists and counts, so the store's `count_*` is called and
            // the servers' paginated path is generated.
            paginated: entities.iter().map(|e| ontogen::to_snake_case(&e.name)).collect(),
        },
    )
    .expect("gen_api failed")
}

/// Generate one backend's whole stack under `root`: the store, then
/// everything above it. Returns the store's metadata.
fn gen_stack(entities: &[EntityDef], backend: Backend, id_strategy: &IdStrategy, root: &Path) -> StoreOutput {
    let store = gen_store_with(entities, backend, id_strategy, &root.join("store"));
    let above = root.join("above");
    let api = gen_api_into(entities, &above.join("api"));

    let servers = ServersConfig {
        api_dir: above.join("api"),
        state_type: "AppState".into(),
        service_import_path: "crate::api".into(),
        types_import_path: "crate::schema".into(),
        state_import: "crate::AppState".into(),
        naming: NamingConfig::default(),
        generators: vec![
            ServerGenerator::HttpAxum { output: above.join("servers/http.rs") },
            ServerGenerator::TauriIpc { output: above.join("servers/ipc.rs") },
            ServerGenerator::Mcp { output: above.join("servers/mcp.rs") },
        ],
        rustfmt_edition: "2024".into(),
        sse_route_overrides: Default::default(),
        route_prefix: None,
        store_type: Some("Store".into()),
        store_import: Some("crate::store::Store".into()),
        pagination: Some(ontogen::servers::PaginationConfig { default_limit: 20, max_limit: 100 }),
        extra_surfaces: vec![],
    };
    ontogen::gen_servers(Some(&api), &[], &servers).expect("gen_servers failed");

    std::fs::create_dir_all(above.join("clients")).expect("clients dir");
    let mut clients =
        ClientsConfig::new(above.join("api"), "AppState", "crate::api", "crate::schema", "crate::AppState");
    clients.generators = vec![
        ClientGenerator::HttpTs {
            output: above.join("clients/http.ts"),
            bindings_path: above.join("clients/bindings.ts"),
        },
        ClientGenerator::AdminRegistry { output: above.join("clients/admin-registry.ts") },
    ];
    clients.store_type = Some("Store".into());
    clients.store_import = Some("crate::store::Store".into());
    clients.schema_entities = entities.to_vec();
    ontogen::gen_clients(Some(&api), &[], &clients).expect("gen_clients failed");

    store
}

/// Read every file under `dir` into a path→content map.
fn tree(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(dir).expect("rel").to_string_lossy().into_owned();
                out.insert(rel, std::fs::read_to_string(&path).expect("read"));
            }
        }
    }
    out
}

fn assert_trees_identical(label: &str, a: &Path, b: &Path) {
    let (ta, tb) = (tree(a), tree(b));
    let keys_a: Vec<&String> = ta.keys().collect();
    let keys_b: Vec<&String> = tb.keys().collect();
    assert_eq!(keys_a, keys_b, "{label}: file sets differ between backends");
    for (path, content_a) in &ta {
        let content_b = &tb[path];
        assert_eq!(content_a, content_b, "{label}: {path} differs between backends (byte-identity violated)");
    }
}

fn method_meta_fingerprint(output: &StoreOutput) -> Vec<String> {
    output
        .methods
        .iter()
        .map(|m| {
            let params: Vec<String> = m.params.iter().map(|p| format!("{}: {}", p.name, p.param_type)).collect();
            format!("{}::{}({}) -> {}", m.entity_name, m.name, params.join(", "), m.return_type)
        })
        .collect()
}

/// The signatures of the non-private methods of every `impl` block in a
/// generated store module. Private helpers (`set_*_parent`,
/// `try_insert_*`) are the backend's own business.
fn store_signatures(file: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(file).expect("read store module");
    let parsed = syn::parse_file(&src).unwrap_or_else(|e| panic!("{} is not valid Rust: {e}", file.display()));
    let mut out = Vec::new();
    for item in parsed.items {
        if let syn::Item::Impl(block) = item {
            for impl_item in block.items {
                if let syn::ImplItem::Fn(f) = impl_item
                    && !matches!(f.vis, syn::Visibility::Inherited)
                {
                    out.push(format!("{} {}", f.vis.to_token_stream(), f.sig.to_token_stream()));
                }
            }
        }
    }
    out
}

#[test]
fn store_method_metadata_is_backend_identical() {
    let entities = fixture_entities();
    for strategy in strategies() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let seaorm = gen_store_with(&entities, Backend::Seaorm(None), &strategy, &tmp.path().join("store_seaorm"));
        let markdown =
            gen_store_with(&entities, markdown_backend(&entities), &strategy, &tmp.path().join("store_markdown"));

        assert_eq!(
            method_meta_fingerprint(&seaorm),
            method_meta_fingerprint(&markdown),
            "{strategy:?}: StoreMethodMeta must never branch on backend — gen_api consumes it"
        );
    }
}

#[test]
fn store_public_signatures_are_backend_identical() {
    let entities = fixture_entities();
    for strategy in strategies() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (a, b) = (tmp.path().join("store_seaorm"), tmp.path().join("store_markdown"));
        gen_store_with(&entities, Backend::Seaorm(None), &strategy, &a);
        gen_store_with(&entities, markdown_backend(&entities), &strategy, &b);

        for entity in &entities {
            let file = format!("{}.rs", ontogen::to_snake_case(&entity.name));
            let (sig_a, sig_b) = (store_signatures(&a.join(&file)), store_signatures(&b.join(&file)));
            assert!(sig_a.iter().any(|s| s.contains("fn count_")), "{file}: count is part of the surface");
            assert_eq!(sig_a, sig_b, "{strategy:?}: {file}: the store's callable surface differs between backends");
        }
    }
}

#[test]
fn downstream_output_is_byte_identical_across_backends() {
    let entities = fixture_entities();
    for strategy in strategies() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (seaorm, markdown): (PathBuf, PathBuf) = (tmp.path().join("seaorm"), tmp.path().join("markdown"));

        // The store layers differ by design; everything above them is
        // generated by each pipeline in turn…
        gen_stack(&entities, Backend::Seaorm(None), &strategy, &seaorm);
        gen_stack(&entities, markdown_backend(&entities), &strategy, &markdown);

        // …and must be the same bytes.
        assert_trees_identical(
            &format!("{strategy:?} above the store"),
            &seaorm.join("above"),
            &markdown.join("above"),
        );
        let above = tree(&seaorm.join("above"));
        for expected in ["api/task.rs", "servers/http.rs", "servers/ipc.rs", "servers/mcp.rs", "clients/http.ts"] {
            assert!(above.contains_key(expected), "{strategy:?}: {expected} was generated: {:?}", above.keys());
        }
        assert!(above["api/task.rs"].contains("count_tasks"), "the paginated module calls the store's count");

        // Sanity: the store layers themselves DID diverge — identical stores
        // would mean the markdown backend isn't actually being exercised.
        let store_a = tree(&seaorm.join("store"));
        let store_b = tree(&markdown.join("store"));
        assert_ne!(store_a, store_b, "store layers must differ between backends");
        assert!(store_b["note.rs"].contains("self.vault()"), "markdown store talks to the vault");
        assert!(store_a["note.rs"].contains("self.db()"), "seaorm store talks to the db");
    }
}

/// NEGATIVE CONTROL: prove the parity comparison can fail. A perturbed file
/// in one tree must be caught — otherwise the invariant is unfalsifiable.
#[test]
fn parity_comparison_detects_a_perturbation() {
    let entities = fixture_entities();
    let tmp = tempfile::tempdir().expect("tempdir");

    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    gen_api_into(&entities, &a);
    gen_api_into(&entities, &b);

    // Perturb one byte in one generated file of tree B.
    let victim = b.join("note.rs");
    let mut content = std::fs::read_to_string(&victim).expect("read victim");
    content.push_str("// perturbed\n");
    std::fs::write(&victim, content).expect("write victim");

    let result = std::panic::catch_unwind(|| assert_trees_identical("negative-control", &a, &b));
    assert!(result.is_err(), "the parity comparison failed to detect a one-line perturbation — it proves nothing");
}
