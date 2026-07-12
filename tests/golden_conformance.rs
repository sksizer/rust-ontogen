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

/// The full-entity-set conformance (generate-then-review, per G1): the
/// pilot's COMMITTED generated trees are the reviewed spec for the
/// relation-complete schema — regenerate from the same schema and assert
/// byte equality. Catches emitter drift against reviewed output AND stale
/// committed files in one direction-agnostic check, with no duplicated
/// golden tree to rot.
#[test]
fn pilot_committed_generated_trees_match_a_fresh_generation() {
    let pilot = repo().join("crates/markdown-pilot");
    let entities = ontogen::parse_schema(&ontogen::SchemaConfig { schema_dir: pilot.join("src/schema") })
        .expect("parse pilot schema")
        .entities;
    assert_eq!(entities.len(), 3, "pilot schema: Note, Tag, Task");

    let tmp = tempfile::tempdir().expect("tempdir");
    let md = ontogen::gen_markdown_io(
        &entities,
        &ontogen::MarkdownIoConfig {
            output_dir: tmp.path().join("persistence"),
            vault_root: "data/vault".into(),
            layout: ontogen::MarkdownLayout::PerEntityDir,
            id_strategy: ontogen::IdStrategy::SlugFromField("title".into()),
            list_cap: 10_000,
        },
    )
    .expect("gen_markdown_io");
    ontogen::gen_store(
        &entities,
        &ontogen::StoreConfig {
            output_dir: tmp.path().join("store"),
            hooks_dir: None,
            schema_module_path: "crate::schema".into(),
            backend: ontogen::Backend::Markdown(md),
        },
    )
    .expect("gen_store");

    for (fresh_dir, committed_dir) in [
        (tmp.path().join("persistence"), pilot.join("src/persistence/markdown/generated")),
        (tmp.path().join("store"), pilot.join("src/store/generated")),
    ] {
        for entry in std::fs::read_dir(&fresh_dir).expect("read_dir") {
            let path = entry.expect("entry").path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let fresh = read(&path);
            let committed = read(&committed_dir.join(&name));
            assert_eq!(
                fresh, committed,
                "{name}: pilot's committed generated file diverges from a fresh generation \
                 (re-run the pilot build and review the diff)"
            );
        }
    }
}

/// The typed-write vault exemplar: building the seeded record through the
/// runtime's typed path must reproduce `seeded-by-writer.md.golden` byte for
/// byte — pinning the emitter-side scalar cosmetics (unquoted date-like
/// strings, single-quoted wikilinks, block lists).
#[test]
fn typed_write_reproduces_the_seeded_vault_golden() {
    #[derive(serde::Serialize)]
    struct SeededFm {
        title: String,
        status: String,
        created: String,
        epic: String,
        tags: Vec<String>,
    }

    let mut doc = markdown_store::Document::new();
    doc.merge_serialize(
        &SeededFm {
            title: "Seeded by the typed writer".into(),
            status: "open".into(),
            created: "2026-06-06".into(),
            epic: markdown_store::wikilink::encode("markdown-backend"),
            tags: vec![markdown_store::wikilink::encode("codegen")],
        },
        &["title", "status", "created", "epic", "tags"],
    )
    .expect("merge");
    doc.set_body(
        "This record pins the TYPED-WRITE shape: what generated create/update code\n\
         produces when it serializes entity values through the emitter (note the\n\
         date is unquoted — a String value that happens to look like a date emits\n\
         as a bare scalar; it round-trips as a string regardless). The sibling\n\
         `ship-the-emitter` golden pins the HAND-AUTHORED parse shape instead;\n\
         the two roles are deliberately separate files.\n",
    );

    let rendered = doc.render().expect("render");
    let golden = read(&repo().join("tests/golden/markdown-backend/vault/tasks/seeded-by-writer.md.golden"));
    assert_eq!(rendered, golden, "typed-write output must match the seeded vault golden byte for byte");
}
