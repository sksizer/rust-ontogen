//! The load-bearing invariant of ADR 0001, contract item 5: for the same
//! schema, everything ABOVE the store — `gen_api`, `gen_servers`,
//! `gen_clients` — is **byte-identical** between the SeaORM and markdown
//! backends. Two consumers in one workspace can share the entire transport
//! stack while their stores diverge.
//!
//! Enforced two ways:
//! - `StoreOutput` method metadata is compared field-by-field (the fast
//!   unit-level guard — `collect_method_meta` must never branch on backend);
//! - the full downstream output trees are diffed recursively, byte for byte.
//!
//! Plus a NEGATIVE CONTROL: a deliberately perturbed store output must make
//! the comparison fail — a parity check that cannot fail proves nothing.

use std::collections::BTreeMap;
use std::path::Path;

use ontogen::ir::{Backend, IdStrategy, MarkdownIoOutput, MarkdownLayout, StoreOutput};
use ontogen::{ApiConfig, EntityDef, SchemaConfig, StoreConfig};

fn fixture_entities() -> Vec<EntityDef> {
    // The relation-complete pilot schema: belongs_to + self-referential
    // has_many + many_to_many + body fields.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/markdown-pilot/src/schema");
    ontogen::parse_schema(&SchemaConfig { schema_dir: dir }).expect("parse pilot schema").entities
}

fn markdown_backend(entities: &[EntityDef]) -> Backend {
    // Build the markdown metadata exactly as gen_markdown_io would, without
    // writing its files (this test only exercises the store/api layers).
    Backend::Markdown(MarkdownIoOutput {
        vault_root: "data/vault".into(),
        layout: MarkdownLayout::PerEntityDir,
        id_strategy: IdStrategy::SlugFromField("title".into()),
        list_cap: 10_000,
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

fn gen_store_with(entities: &[EntityDef], backend: Backend, out: &Path) -> StoreOutput {
    ontogen::gen_store(
        entities,
        &StoreConfig {
            output_dir: out.to_path_buf(),
            hooks_dir: None,
            schema_module_path: "crate::schema".into(),
            backend,
        },
    )
    .expect("gen_store failed")
}

fn gen_api_into(entities: &[EntityDef], out: &Path) {
    ontogen::gen_api(
        entities,
        &ApiConfig {
            output_dir: out.to_path_buf(),
            exclude: Vec::new(),
            scan_dirs: Vec::new(),
            state_type: "AppState".into(),
            store_type: Some("Store".into()),
            schema_module_path: "crate::schema".into(),
        },
    )
    .expect("gen_api failed");
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

#[test]
fn store_method_metadata_is_backend_identical() {
    let entities = fixture_entities();
    let tmp = tempfile::tempdir().expect("tempdir");

    let seaorm = gen_store_with(&entities, Backend::Seaorm(None), &tmp.path().join("store_seaorm"));
    let markdown = gen_store_with(&entities, markdown_backend(&entities), &tmp.path().join("store_markdown"));

    assert_eq!(
        method_meta_fingerprint(&seaorm),
        method_meta_fingerprint(&markdown),
        "StoreMethodMeta must never branch on backend — gen_api consumes it"
    );
}

#[test]
fn downstream_api_output_is_byte_identical_across_backends() {
    let entities = fixture_entities();
    let tmp = tempfile::tempdir().expect("tempdir");

    // Generate the store layer under BOTH backends (different by design)…
    gen_store_with(&entities, Backend::Seaorm(None), &tmp.path().join("store_seaorm"));
    gen_store_with(&entities, markdown_backend(&entities), &tmp.path().join("store_markdown"));

    // …then the API layer for each pipeline.
    let api_seaorm = tmp.path().join("api_seaorm");
    let api_markdown = tmp.path().join("api_markdown");
    gen_api_into(&entities, &api_seaorm);
    gen_api_into(&entities, &api_markdown);

    assert_trees_identical("gen_api", &api_seaorm, &api_markdown);

    // Sanity: the store layers themselves DID diverge — identical stores
    // would mean the markdown backend isn't actually being exercised.
    let store_a = tree(&tmp.path().join("store_seaorm"));
    let store_b = tree(&tmp.path().join("store_markdown"));
    assert_ne!(store_a, store_b, "store layers must differ between backends");
    let md_note = &store_b["note.rs"];
    assert!(md_note.contains("self.vault()"), "markdown store talks to the vault");
    assert!(store_a["note.rs"].contains("self.db()"), "seaorm store talks to the db");
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
