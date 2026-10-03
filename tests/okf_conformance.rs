//! OKF 0.2 §11 conformance: the checker in `support/okf.rs` against
//! hand-built good and bad bundles, then over every example's seed vault.

mod support;

use std::path::{Path, PathBuf};

use support::okf::check_bundle;

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn messages(root: &Path) -> Vec<(String, String)> {
    check_bundle(root)
        .into_iter()
        .map(|v| (v.path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), v.message))
        .collect()
}

#[test]
fn a_conformant_bundle_has_no_violations() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "index.md",
        "---\nokf_version: '0.2'\n---\n# Tasks\n\n* [Ship](tasks/ship.md) - ship it\n\n\
         ## Elsewhere\n* [Notes](notes/)\n* [Minimal](</notes/minimal.md>)\n* [A \\[draft\\]](tasks/my%20draft.md) - d\n",
    );
    write(root, "log.md", "# Update log\n\n## 2026-10-03\n* **Creation**: ship\n\n## 2026-09-30\n* **Update**: x\n");
    write(root, "tasks/index.md", "# Tasks\n\n* [Ship](ship.md)\n");
    write(root, "tasks/my draft.md", "---\ntype: Task\n---\n");
    write(root, "tasks/log.md", "# Tasks log\n");
    write(root, "tasks/ship.md", "---\ntype: Task\ntitle: Ship\n---\nBody.\n");
    write(root, "notes/minimal.md", "---\ntype: Some Unknown Type\n---\n");
    write(root, "tasks/notes.txt", "not markdown, not checked");
    write(root, ".obsidian/workspace.md", "hidden tool state, not part of the bundle");
    assert_eq!(messages(root), Vec::<(String, String)>::new());
}

#[test]
fn a_bad_bundle_reports_every_violation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "index.md", "---\nokf_version: '0.2'\ntitle: Root\n---\n# A\n\n* [A](a/)\n");
    write(root, "log.md", "---\ntype: Log\n---\n## 2026-10-03\n## October 3rd\n## 2026-13-01\n");
    write(root, "a/index.md", "---\nokf_version: '0.2'\n---\n# A\n\n* [No type](no-type.md)\n");
    write(root, "a/no-frontmatter.md", "# Just prose\n");
    write(root, "a/no-type.md", "---\ntitle: Untyped\n---\n");
    write(root, "a/empty-type.md", "---\ntype: ''\n---\n");
    write(root, "a/number-type.md", "---\ntype: 3\n---\n");
    write(root, "a/broken.md", "---\n: : : not yaml\n---\n");

    let found = messages(root);
    let expect = |path: &str, needle: &str| {
        assert!(
            found.iter().any(|(p, m)| p == path && m.contains(needle)),
            "expected {path}: {needle:?} in {found:#?}"
        );
    };
    expect("index.md", "may only carry `okf_version`, found `title`");
    expect("log.md", "must not have frontmatter");
    expect("log.md", "`## October 3rd`");
    expect("log.md", "`## 2026-13-01`");
    expect("a/index.md", "below the bundle root must not have frontmatter");
    expect("a/no-frontmatter.md", "no frontmatter block");
    expect("a/no-type.md", "no `type`");
    expect("a/empty-type.md", "non-empty string");
    expect("a/number-type.md", "non-empty string");
    expect("a/broken.md", "does not parse");
    assert_eq!(found.len(), 10, "{found:#?}");
}

#[test]
fn malformed_index_files_report_every_violation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "ok.md", "---\ntype: Note\n---\n");
    write(root, "index.md", "---\nokf_version: 0.2\n---\n");
    write(root, "empty/index.md", "\n");
    write(root, "empty/x.md", "---\ntype: Note\n---\n");
    write(
        root,
        "b/index.md",
        "* [Orphan](../ok.md)\n\
         # Section\n\
         * [Missing](missing.md)\n\
         * [Gone dir](gone/)\n\
         * [File as dir](../ok.md/)\n\
         - [Dash](../ok.md)\n\
         * [Spaced](../my file.md)\n\
         * [Trailing](../ok.md) and more\n\
         * [Bad escape](%zz.md)\n\
         Some prose.\n\
         # Glued\n\
         * [Fine](../ok.md) - described\n",
    );
    write(root, "b/x.md", "---\ntype: Note\n---\n");

    let found = messages(root);
    let expect = |path: &str, needle: &str| {
        assert!(
            found.iter().any(|(p, m)| p == path && m.contains(needle)),
            "expected {path}: {needle:?} in {found:#?}"
        );
    };
    expect("index.md", "`okf_version` must be a string");
    expect("index.md", "has no section");
    expect("empty/index.md", "has no section");
    expect("b/index.md", "`* [Orphan](../ok.md)` comes before any section heading");
    expect("b/index.md", "link `missing.md` does not resolve to an existing file");
    expect("b/index.md", "link `gone/` does not resolve to an existing directory");
    expect("b/index.md", "link `../ok.md/` does not resolve to an existing directory");
    expect("b/index.md", "`- [Dash](../ok.md)` is neither a heading nor");
    expect("b/index.md", "`* [Spaced](../my file.md)` is neither a heading nor");
    expect("b/index.md", "must end after the link or with ` - <description>`");
    expect("b/index.md", "link `%zz.md` has a malformed percent-escape");
    expect("b/index.md", "`Some prose.` is neither a heading nor");
    expect("b/index.md", "heading `# Glued` must follow a blank line");
    assert_eq!(found.len(), 13, "{found:#?}");
}

/// A vault written through the store with both OKF options on is a
/// conformant bundle, its index files included.
#[test]
fn a_vault_written_with_both_okf_options_is_conformant() {
    let dir = tempfile::tempdir().unwrap();
    let vault = markdown_store::VaultHandle::new(
        dir.path(),
        markdown_store::VaultLayout::PerEntityDir,
        markdown_store::IdStrategy::SlugFromField("title".into()),
    )
    .with_okf_index(true)
    .with_generated_by("ontogen-tests/1.0");

    let record = |title: &str, description: Option<&str>| {
        let mut doc = markdown_store::Document::new();
        doc.set("title", title);
        if let Some(d) = description {
            doc.set("description", d);
        }
        doc
    };
    let tasks = vault.entity("tasks", "Task");
    tasks.create(None, Some("Ship [it] (soon)"), record("Ship [it] (soon)", Some("multi\nline"))).unwrap();
    tasks.create(None, Some("Plan"), record("Plan", None)).unwrap();
    vault.entity("notes", "Note").create(None, Some("Idea"), record("Idea", Some("why"))).unwrap();
    std::fs::write(dir.path().join("notes/hand written (draft).md"), "---\ntype: Note\n---\n").unwrap();
    vault.rebuild_indexes().unwrap();
    tasks.remove("plan").unwrap();

    assert!(dir.path().join("index.md").is_file() && dir.path().join("tasks/index.md").is_file());
    assert_eq!(check_bundle(dir.path()), vec![], "a written vault must be an OKF 0.2 bundle");
}

/// Every example's checked-in seed vault is an OKF bundle as committed.
#[test]
fn example_seed_vaults_are_okf_conformant() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut vaults: Vec<PathBuf> = std::fs::read_dir(&examples)
        .expect("examples dir")
        .map(|e| e.expect("entry").path().join("data/vault"))
        .filter(|p| p.is_dir())
        .collect();
    vaults.sort();
    assert!(vaults.len() >= 3, "expected the iron-log-md, notes-kb and tasks-tracker seed vaults: {vaults:?}");

    let violations: Vec<String> = vaults.iter().flat_map(|v| check_bundle(v)).map(|v| v.to_string()).collect();
    assert!(violations.is_empty(), "seed vaults break OKF 0.2 §11:\n{}", violations.join("\n"));
}
