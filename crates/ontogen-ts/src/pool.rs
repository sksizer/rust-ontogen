//! Type-pool walker — scans a user crate's `src/` for module-level structs,
//! enums, and type aliases, keys them by canonical [`TypePath`], and returns
//! the populated pool.
//!
//! Phase-1 rules (matching the OF-015 design pass):
//!
//! - Walk `src/` recursively. `examples/`, `benches/`, `tests/`, and
//!   `build.rs` are out of scope — those don't ship wire code.
//! - Parse each `.rs` via `syn::parse_file`. The result is raw AST without
//!   cfg-eval; cfg-gated types live in the pool like any other.
//! - Collect every `ItemStruct` / `ItemEnum` / `ItemType` at module level,
//!   regardless of visibility (`pub(crate)` types reachable from a `pub`
//!   API still flow over the wire).
//! - Function-local and impl-block-nested types are excluded — they can't
//!   appear as plain return-type idents in a public API signature.
//! - Inline `mod foo { ... }` blocks are walked recursively, contributing
//!   their module name to each contained item's canonical path.
//!
//! Path derivation — every key begins with the root the tree was scanned as
//! ([`LOCAL_CRATE_ROOT`] for the consuming crate, the package name for a
//! `pool_extra_roots` sibling):
//!
//! - `src/lib.rs` items → path `["crate", "ItemName"]`
//! - `src/foo.rs` items → path `["crate", "foo", "ItemName"]`
//! - `src/foo/mod.rs` items → path `["crate", "foo", "ItemName"]`
//! - `src/foo/bar.rs` items → path `["crate", "foo", "bar", "ItemName"]`
//! - Inline `mod baz { pub struct Q; }` inside `src/foo.rs` → `["crate", "foo", "baz", "Q"]`
//!
//! Naming the root in the key is what keeps a workspace sibling's types
//! distinguishable from the consuming crate's own once the two pools are
//! merged. Before this, both trees were keyed relative to their own `src/`,
//! so a sibling's `lint::Severity` and a local `lint::Severity` produced the
//! same key and one silently displaced the other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::resolve::{ModuleImports, collect_module_imports};
use crate::types::TypePath;

/// Failure modes for [`scan_src_dir`].
#[derive(Debug)]
pub enum ScanError {
    /// I/O error reading a file or directory.
    Io {
        /// The path the error happened at.
        path: PathBuf,
        /// The underlying OS error message.
        message: String,
    },
    /// `syn::parse_file` failed on a `.rs` file.
    Parse {
        /// The path of the unparseable file.
        path: PathBuf,
        /// The syn parser's error message.
        message: String,
    },
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "I/O error reading `{}`: {message}", path.display()),
            Self::Parse { path, message } => write!(f, "syn parse error in `{}`: {message}", path.display()),
        }
    }
}

impl std::error::Error for ScanError {}

/// First key segment for types scanned from the consuming crate's own `src/`.
///
/// Every pool key names the root it came from, so a key carries its crate
/// boundary rather than losing it in a flat merge. The local root uses the
/// literal `crate`, which is what a user writes in source and — being a
/// keyword — is a segment no real crate name can ever occupy. Additional
/// roots merged in via `pool_extra_roots` use their package name, so
/// `crate::schema::Severity` and `vaultpolish_core::lint::Severity` stay
/// distinguishable after the merge.
pub const LOCAL_CRATE_ROOT: &str = "crate";

/// Scan a `src/` directory and collect every module-level struct, enum, and
/// type-alias into a pool keyed by canonical [`TypePath`], rooted at
/// [`LOCAL_CRATE_ROOT`].
///
/// This discards the per-module `use` tables. Callers that need bare
/// single-segment references resolved through their defining module's
/// imports (the dep extractor in `order`) should use
/// [`scan_src_dir_with_imports`] instead.
pub fn scan_src_dir(src_dir: &Path) -> Result<BTreeMap<TypePath, syn::Item>, ScanError> {
    scan_src_dir_with_imports(src_dir).map(|(pool, _imports)| pool)
}

/// Scan a `src/` directory, returning both the type pool and the per-module
/// [`ModuleImports`] tables built from each module's `use` declarations.
///
/// The imports table lets the dependency extractor resolve a bare
/// single-segment reference (`BackupManifest`) through the actual `use` that
/// brought it into scope, instead of guessing by terminal segment — which is
/// ambiguous when two modules define same-named types.
///
/// Keys are rooted at [`LOCAL_CRATE_ROOT`]; use [`scan_crate_root_with_imports`]
/// to scan a workspace sibling under its own package name.
pub fn scan_src_dir_with_imports(src_dir: &Path) -> Result<(BTreeMap<TypePath, syn::Item>, ModuleImports), ScanError> {
    scan_crate_root_with_imports(src_dir, LOCAL_CRATE_ROOT)
}

/// Scan a `src/` directory as the crate named `crate_root`, so every pool key
/// and every module-imports key begins with that segment.
///
/// This is what keeps a workspace sibling's types distinguishable from the
/// consuming crate's own after the two pools are merged. The pool and the
/// imports table MUST be rooted identically — a mismatch doesn't fail loudly,
/// it just makes `ModuleImports::get` miss and silently degrades resolution
/// to terminal-segment guessing.
pub fn scan_crate_root_with_imports(
    src_dir: &Path,
    crate_root: &str,
) -> Result<(BTreeMap<TypePath, syn::Item>, ModuleImports), ScanError> {
    let mut pool = BTreeMap::new();
    let mut imports = ModuleImports::default();
    let root = [crate_root.to_string()];
    scan_dir_recursive(src_dir, &root, &mut pool, &mut imports)?;
    Ok((pool, imports))
}

/// Recursive directory walker. `module_prefix` is the canonical path of the
/// current Rust module, starting at `[crate_root]` for the scanned tree's
/// own root. Each `.rs` file contributes its items (and items nested inside
/// `mod` blocks) under that prefix, plus its `use` declarations into
/// `imports` — under the *same* prefix, which is what lets the resolver look
/// a referencing module's imports up by its pool key minus the terminal.
fn scan_dir_recursive(
    dir: &Path,
    module_prefix: &[String],
    pool: &mut BTreeMap<TypePath, syn::Item>,
    imports: &mut ModuleImports,
) -> Result<(), ScanError> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| ScanError::Io { path: dir.to_path_buf(), message: e.to_string() })?;

    // Sort entries for deterministic walking — file system iteration order
    // isn't guaranteed and we don't want pool key ordering to depend on it.
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    sorted.sort();

    for path in sorted {
        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        if path.is_dir() {
            // Recurse into the directory, prepending its name to the module
            // prefix. We skip the recursion if there's no `mod.rs` AND the
            // directory contains no `.rs` files (defensive; real crates
            // always have one or the other).
            let mut next_prefix = module_prefix.to_vec();
            next_prefix.push(file_name);
            scan_dir_recursive(&path, &next_prefix, pool, imports)?;
            continue;
        }

        // Skip anything that's not a `.rs` file.
        if !file_name.ends_with(".rs") {
            continue;
        }

        // Skip the `build.rs` if it somehow lands inside `src/`.
        if file_name == "build.rs" {
            continue;
        }

        // Determine the module prefix this file contributes to. `mod.rs` and
        // `lib.rs` / `main.rs` don't extend the prefix — they ARE the
        // current module.
        let file_prefix: Vec<String> = if matches!(file_name.as_str(), "lib.rs" | "main.rs" | "mod.rs") {
            module_prefix.to_vec()
        } else {
            // `foo.rs` extends the prefix by `foo`.
            let stem = file_name.trim_end_matches(".rs");
            let mut p = module_prefix.to_vec();
            p.push(stem.to_string());
            p
        };

        let src =
            std::fs::read_to_string(&path).map_err(|e| ScanError::Io { path: path.clone(), message: e.to_string() })?;
        let parsed: syn::File =
            syn::parse_file(&src).map_err(|e| ScanError::Parse { path: path.clone(), message: e.to_string() })?;

        collect_items(&parsed.items, &file_prefix, pool);
        collect_module_imports(&parsed, &file_prefix, imports);
    }

    Ok(())
}

/// Walk a slice of `syn::Item`s, inserting structs / enums / type aliases
/// into the pool and recursing into inline `mod foo { ... }` blocks.
fn collect_items(items: &[syn::Item], module_prefix: &[String], pool: &mut BTreeMap<TypePath, syn::Item>) {
    for item in items {
        match item {
            syn::Item::Struct(s) => insert(pool, module_prefix, &s.ident, item.clone()),
            syn::Item::Enum(e) => insert(pool, module_prefix, &e.ident, item.clone()),
            syn::Item::Type(t) => insert(pool, module_prefix, &t.ident, item.clone()),
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    let mut sub_prefix = module_prefix.to_vec();
                    sub_prefix.push(m.ident.to_string());
                    collect_items(inner_items, &sub_prefix, pool);
                }
                // Module declarations without inline content (`mod foo;`) are
                // resolved by the file-system walker — the corresponding
                // `foo.rs` or `foo/mod.rs` is scanned separately.
            }
            _ => {} // ignore fns, impls, statics, consts, use, etc.
        }
    }
}

fn insert(pool: &mut BTreeMap<TypePath, syn::Item>, prefix: &[String], ident: &syn::Ident, item: syn::Item) {
    let mut segments = prefix.to_vec();
    segments.push(ident.to_string());
    if let Ok(path) = TypePath::new(segments) {
        pool.insert(path, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a temporary directory with `files` written into it (each entry
    /// is `(relative_path, contents)`). Returns a guard that cleans up on
    /// drop.
    fn make_tempdir(files: &[(&str, &str)]) -> tempfile::TempDir {
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

    /// A pool key for the local crate — `segments` with [`LOCAL_CRATE_ROOT`]
    /// prepended, since every key names the root it came from.
    fn tp(segments: &[&str]) -> TypePath {
        let mut all = vec![LOCAL_CRATE_ROOT.to_string()];
        all.extend(segments.iter().map(|s| (*s).to_string()));
        TypePath::new(all).expect("non-empty")
    }

    /// A pool key with an explicit root, for extra-root scans.
    fn rooted(segments: &[&str]) -> TypePath {
        TypePath::new(segments.iter().map(|s| (*s).to_string()).collect()).expect("non-empty")
    }

    #[test]
    fn keys_are_rooted_at_the_local_crate() {
        // Spelled out rather than going through `tp`, so the key convention
        // itself is pinned somewhere obvious.
        let dir = make_tempdir(&[("lib.rs", ""), ("models.rs", "pub struct Workout { pub id: u64 }")]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert!(
            pool.contains_key(&rooted(&["crate", "models", "Workout"])),
            "pool keys: {:?}",
            pool.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn extra_root_keys_are_rooted_at_their_package_name() {
        // The whole point: a sibling's types stay distinguishable from the
        // consuming crate's after a merge, even when the module path and the
        // type name both match.
        let dir = make_tempdir(&[("lint/mod.rs", "pub enum Severity { Error, Warning }")]);
        let (pool, imports) = scan_crate_root_with_imports(dir.path(), "vaultpolish_core").unwrap();
        assert!(
            pool.contains_key(&rooted(&["vaultpolish_core", "lint", "Severity"])),
            "pool keys: {:?}",
            pool.keys().collect::<Vec<_>>()
        );
        // Pool and imports must be rooted identically, or `ModuleImports::get`
        // misses and resolution silently degrades to terminal guessing.
        assert!(
            imports.get(&["vaultpolish_core".to_string(), "lint".to_string()]).is_some(),
            "imports table must be rooted the same way as the pool"
        );
    }

    #[test]
    fn a_local_and_a_sibling_type_no_longer_share_a_key() {
        // Before rooting, both keyed as ["lint", "Severity"] and the merge's
        // `or_insert` silently dropped one.
        let local = make_tempdir(&[("lint/mod.rs", "pub enum Severity { Error }")]);
        let sibling = make_tempdir(&[("lint/mod.rs", "pub enum Severity { Error, Warning, Info }")]);
        let local_pool = scan_src_dir(local.path()).unwrap();
        let (sibling_pool, _) = scan_crate_root_with_imports(sibling.path(), "vaultpolish_core").unwrap();

        let mut merged = local_pool;
        for (key, item) in sibling_pool {
            merged.entry(key).or_insert(item);
        }
        assert_eq!(merged.len(), 2, "both definitions survive the merge: {:?}", merged.keys().collect::<Vec<_>>());
    }

    #[test]
    fn scans_lib_rs_top_level_struct() {
        let dir = make_tempdir(&[("lib.rs", "pub struct Foo { pub bar: u32 }")]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert_eq!(pool.len(), 1);
        assert!(pool.contains_key(&tp(&["Foo"])));
        // The stored item is the struct.
        match pool.get(&tp(&["Foo"])).unwrap() {
            syn::Item::Struct(s) => assert_eq!(s.ident.to_string(), "Foo"),
            other => panic!("expected ItemStruct, got {other:?}"),
        }
    }

    #[test]
    fn scans_module_file_paths() {
        let dir = make_tempdir(&[("lib.rs", ""), ("models.rs", "pub struct Workout { pub id: u64 }")]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert!(pool.contains_key(&tp(&["models", "Workout"])));
    }

    #[test]
    fn scans_nested_directory_paths() {
        let dir = make_tempdir(&[
            ("lib.rs", "pub mod outer;"),
            ("outer/mod.rs", "pub mod inner;"),
            ("outer/inner.rs", "pub enum Status { Live, Dead }"),
        ]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert!(
            pool.contains_key(&tp(&["outer", "inner", "Status"])),
            "pool keys: {:?}",
            pool.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn collects_all_three_item_kinds() {
        let dir = make_tempdir(&[(
            "lib.rs",
            r#"
            pub struct S { pub x: u32 }
            pub enum E { A, B }
            pub type T = u32;
            "#,
        )]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert!(pool.contains_key(&tp(&["S"])));
        assert!(pool.contains_key(&tp(&["E"])));
        assert!(pool.contains_key(&tp(&["T"])));
    }

    #[test]
    fn ignores_functions_and_impls() {
        let dir = make_tempdir(&[(
            "lib.rs",
            r#"
            pub struct S { pub x: u32 }
            pub fn unrelated() {}
            impl S {
                pub fn method(&self) {}
            }
            "#,
        )]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert_eq!(pool.len(), 1);
        assert!(pool.contains_key(&tp(&["S"])));
    }

    #[test]
    fn collects_pub_crate_types() {
        // Visibility doesn't matter — pub(crate) types reachable from a pub
        // API still flow over the wire.
        let dir = make_tempdir(&[(
            "lib.rs",
            r#"
            pub(crate) struct Internal { pub x: u32 }
            "#,
        )]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert!(pool.contains_key(&tp(&["Internal"])));
    }

    #[test]
    fn collects_inline_module_blocks() {
        let dir = make_tempdir(&[(
            "lib.rs",
            r#"
            pub mod nested {
                pub struct Inner { pub x: u32 }
                pub enum Sub { A }
            }
            "#,
        )]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert!(pool.contains_key(&tp(&["nested", "Inner"])));
        assert!(pool.contains_key(&tp(&["nested", "Sub"])));
    }

    #[test]
    fn parse_error_surfaces_with_path() {
        let dir = make_tempdir(&[("lib.rs", "pub struct Broken { this is not valid rust")]);
        let err = scan_src_dir(dir.path()).unwrap_err();
        match err {
            ScanError::Parse { path, .. } => {
                assert!(path.to_string_lossy().ends_with("lib.rs"));
            }
            other => panic!("expected Parse error, got {other:?}"),
        }
    }

    #[test]
    fn missing_directory_yields_io_error() {
        let dir = make_tempdir(&[]);
        let phantom = dir.path().join("does_not_exist");
        let err = scan_src_dir(&phantom).unwrap_err();
        assert!(matches!(err, ScanError::Io { .. }));
    }

    #[test]
    fn skips_non_rust_files() {
        let dir = make_tempdir(&[
            ("lib.rs", "pub struct S { pub x: u32 }"),
            ("README.md", "# unrelated"),
            ("data.json", "{}"),
        ]);
        let pool = scan_src_dir(dir.path()).unwrap();
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn deterministic_ordering_via_btreemap() {
        // The pool is keyed by BTreeMap so iteration order is the natural
        // canonical-path order. Two scans of the same tree yield the same
        // key vector.
        let files: &[(&str, &str)] =
            &[("lib.rs", ""), ("z.rs", "pub struct Zee;"), ("a.rs", "pub struct Aye;"), ("m.rs", "pub struct Em;")];
        let dir1 = make_tempdir(files);
        let dir2 = make_tempdir(files);
        let pool1 = scan_src_dir(dir1.path()).unwrap();
        let pool2 = scan_src_dir(dir2.path()).unwrap();
        let keys1: Vec<_> = pool1.keys().collect();
        let keys2: Vec<_> = pool2.keys().collect();
        assert_eq!(keys1, keys2);
        // Plus: explicitly sorted by canonical path.
        let names: Vec<&str> = keys1.iter().map(|p| p.terminal()).collect();
        // Note: "Aye" < "Em" < "Zee" but pool key paths are ["a", "Aye"] etc.
        // — sorted lexicographically by full path.
        assert_eq!(names, vec!["Aye", "Em", "Zee"]);
    }
}
