//! Guard for the markdown-backend golden spec tree.
//!
//! Until the conformance harness lands (which turns the goldens into
//! executable assertions against the real generators), this test only
//! pins their existence — a silent deletion or rename of the spec must
//! fail CI, because later PRs in the ADR 0001 campaign are graded
//! against these files.

use std::path::Path;

#[test]
fn golden_spec_tree_is_present_and_nonempty() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/markdown-backend");
    let expected = [
        "README.md",
        "build.rs.golden",
        "store/note.rs.golden",
        "vault/notes/welcome.md.golden",
        "vault/tasks/ship-the-emitter.md.golden",
        "vault/epics/markdown-backend.md.golden",
    ];
    for rel in expected {
        let path = root.join(rel);
        let meta = std::fs::metadata(&path).unwrap_or_else(|_| panic!("golden spec file missing: {}", path.display()));
        assert!(meta.len() > 0, "golden spec file is empty: {}", path.display());
    }
}

#[test]
fn vault_goldens_parse_with_markdown_store_and_roundtrip_verbatim() {
    // The vault goldens must be valid records under the real runtime crate,
    // and — per the byte-stability contract — parse→render must reproduce
    // them exactly.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/markdown-backend/vault");
    let mut seen = 0;
    for entry in walkdir(&root) {
        let src = std::fs::read_to_string(&entry).unwrap();
        let doc = markdown_store::Document::parse(&src)
            .unwrap_or_else(|e| panic!("golden {} must parse: {e}", entry.display()));
        assert!(!doc.mapping().is_empty(), "golden {} has empty frontmatter", entry.display());
        assert_eq!(doc.render().unwrap(), src, "golden {} must round-trip verbatim", entry.display());
        seen += 1;
    }
    assert!(seen >= 3, "expected at least the three seed vault goldens, found {seen}");
}

fn walkdir(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "golden") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
