//! Golden conformance: the real generators, graded against the hand-authored
//! spec in `tests/golden/markdown-backend/`.
//!
//! The comparison is `rustfmt(generated)` — with the repo `rustfmt.toml` and
//! the pilot consumer's edition (2024), exactly what `write_and_format`
//! produces — against the golden bytes **as committed**. Nothing is stripped
//! or normalized at test time: the user's exact bytes are the assertion, so
//! editing a golden mechanically forces the emitter to follow.
//!
//! This file starts with the minimal-entity shape contract (`note.rs.golden`)
//! that the emitter PR must satisfy; the conformance PR extends it with the
//! generated-then-reviewed full entity set and the typed-write vault check.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn markdown_store_emission_matches_note_golden() {
    let entities =
        ontogen::parse_schema(&ontogen::SchemaConfig { schema_dir: repo().join("tests/fixtures/golden-note") })
            .expect("parse golden-note fixture")
            .entities;
    assert_eq!(entities.len(), 1, "the golden-note fixture holds exactly the Note entity");

    let tmp = tempfile::tempdir().expect("tempdir");
    let md = ontogen::gen_markdown_io(
        &entities,
        &ontogen::MarkdownIoConfig {
            output_dir: tmp.path().join("markdown"),
            vault_root: "data/vault".into(),
            layout: ontogen::MarkdownLayout::PerEntityDir,
            id_strategy: ontogen::IdStrategy::SlugFromField("title".into()),
            list_cap: 10_000,
        },
    )
    .expect("gen_markdown_io failed");

    ontogen::gen_store(
        &entities,
        &ontogen::StoreConfig {
            output_dir: tmp.path().join("store"),
            hooks_dir: None,
            schema_module_path: "crate::schema".into(),
            backend: ontogen::Backend::Markdown(md),
        },
    )
    .expect("gen_store(markdown) failed");

    let generated = read(&tmp.path().join("store/note.rs"));
    let golden = read(&repo().join("tests/golden/markdown-backend/store/note.rs.golden"));

    if generated != golden {
        let diff: Vec<String> = golden
            .lines()
            .zip(generated.lines())
            .enumerate()
            .filter(|(_, (g, e))| g != e)
            .take(20)
            .map(|(i, (g, e))| format!("line {}:\n  golden:    {g}\n  generated: {e}", i + 1))
            .collect();
        panic!(
            "generated markdown store diverges from the golden spec \
             (tests/golden/markdown-backend/store/note.rs.golden).\n\
             First differing lines:\n{}\n\n--- full generated ---\n{generated}",
            diff.join("\n")
        );
    }
}
