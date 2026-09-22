//! Two source roots merged into one pool (issue #84).
//!
//! The workspace shape this covers is the common one: a Tauri/Axum shell
//! crate next to a headless core crate, with the core's `src/` added via
//! `ClientsConfig::pool_extra_roots`.
//!
//! Both trees used to be keyed relative to their own `src/`, so the crate
//! boundary was lost in the merge. A local `lint::Severity` and a sibling
//! `lint::Severity` produced the *same* key — one silently displaced the
//! other — and a bare reference to either was reported as `Ambiguous`,
//! failing the build. These tests pin the behaviour now that every key names
//! the root it came from.

use std::collections::BTreeMap;
use std::fs;

use ontogen_ts::{
    ModuleImports, Resolution, TypePath, resolve_reference, scan_crate_root_with_imports, scan_src_dir_with_imports,
};

fn write_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (rel, content) in files {
        let abs = dir.path().join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(&abs, content).expect("write file");
    }
    dir
}

fn tp(segments: &[&str]) -> TypePath {
    TypePath::new(segments.iter().map(|s| (*s).to_string()).collect()).expect("non-empty")
}

/// The issue's shape: the shell defines a local `Severity` mirror, the core
/// crate defines its own, and both are merged into one pool exactly as
/// `generate_clients` does.
fn merged_pool() -> (BTreeMap<TypePath, syn::Item>, ModuleImports) {
    let local = write_tree(&[
        ("lib.rs", "pub mod api; pub mod schema;"),
        ("schema/mod.rs", "pub mod scan; pub use scan::*;"),
        ("schema/scan.rs", "pub enum Severity { Error, Warning, Info }"),
        // The API module reaches its local mirror through a glob re-export —
        // which is why resolution can't lean on the `use` table here.
        ("api/mod.rs", "pub mod scan;"),
        ("api/scan.rs", "use crate::schema::*;\npub struct Report { pub worst: Severity }"),
    ]);
    let sibling = write_tree(&[
        ("lib.rs", "pub mod lint;"),
        ("lint/mod.rs", "pub enum Severity { Error }\npub struct Finding { pub level: Severity }"),
    ]);

    let (mut pool, mut imports) = scan_src_dir_with_imports(local.path()).expect("scan local");
    let (extra, extra_imports) =
        scan_crate_root_with_imports(sibling.path(), "vaultpolish_core").expect("scan sibling");
    for (key, item) in extra {
        pool.entry(key).or_insert(item);
    }
    imports.merge(extra_imports);
    // Keep the tempdirs alive until after the scan.
    drop(local);
    drop(sibling);
    (pool, imports)
}

#[test]
fn both_definitions_survive_the_merge() {
    let (pool, _) = merged_pool();
    assert!(
        pool.contains_key(&tp(&["crate", "schema", "scan", "Severity"])),
        "keys: {:?}",
        pool.keys().collect::<Vec<_>>()
    );
    assert!(
        pool.contains_key(&tp(&["vaultpolish_core", "lint", "Severity"])),
        "keys: {:?}",
        pool.keys().collect::<Vec<_>>()
    );
}

#[test]
fn a_bare_reference_in_the_shell_resolves_to_the_shell_type() {
    // The reported failure. `Severity` arrives through a glob, so nothing
    // names it explicitly and resolution falls to terminal matching. A bare
    // ident can't reach a foreign crate's type unaided, so the local one is
    // the only answer that could be right — this used to be `Ambiguous` and
    // failed the build.
    let (pool, imports) = merged_pool();
    let r = resolve_reference(
        &["Severity".to_string()],
        &["crate".to_string(), "api".to_string(), "scan".to_string()],
        &pool,
        &imports,
    );
    assert_eq!(r, Resolution::Resolved(tp(&["crate", "schema", "scan", "Severity"])), "got {r:?}");
}

#[test]
fn a_bare_reference_inside_the_sibling_stays_in_the_sibling() {
    // Symmetric: `Finding { level: Severity }` is written in the core crate
    // and means the core crate's `Severity`, not the shell's mirror.
    let (pool, imports) = merged_pool();
    let r = resolve_reference(
        &["Severity".to_string()],
        &["vaultpolish_core".to_string(), "lint".to_string()],
        &pool,
        &imports,
    );
    assert_eq!(r, Resolution::Resolved(tp(&["vaultpolish_core", "lint", "Severity"])), "got {r:?}");
}

#[test]
fn the_shell_can_name_the_sibling_type_outright() {
    // The escape hatch the issue asked for: qualify the reference and get the
    // sibling type. Previously `absolutize` returned `None` for any non-local
    // first segment, so this fell through to terminal guessing.
    let (pool, imports) = merged_pool();
    let r = resolve_reference(
        &["vaultpolish_core".to_string(), "lint".to_string(), "Severity".to_string()],
        &["crate".to_string(), "api".to_string(), "scan".to_string()],
        &pool,
        &imports,
    );
    assert_eq!(r, Resolution::Resolved(tp(&["vaultpolish_core", "lint", "Severity"])), "got {r:?}");
}

#[test]
fn a_genuine_external_crate_still_resolves_to_nothing() {
    // Control: rooting must not turn every unknown path into a pool hit.
    let (pool, imports) = merged_pool();
    let r = resolve_reference(
        &["chrono".to_string(), "DateTime".to_string()],
        &["crate".to_string(), "api".to_string(), "scan".to_string()],
        &pool,
        &imports,
    );
    assert_eq!(r, Resolution::NotInPool, "got {r:?}");
}
