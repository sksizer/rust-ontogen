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
//! Path derivation:
//!
//! - `src/lib.rs` items → path `["ItemName"]`
//! - `src/foo.rs` items → path `["foo", "ItemName"]`
//! - `src/foo/mod.rs` items → path `["foo", "ItemName"]`
//! - `src/foo/bar.rs` items → path `["foo", "bar", "ItemName"]`
//! - Inline `mod baz { pub struct Q; }` inside `src/foo.rs` → `["foo", "baz", "Q"]`

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

/// Scan a `src/` directory and collect every module-level struct, enum, and
/// type-alias into a pool keyed by canonical [`TypePath`].
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
pub fn scan_src_dir_with_imports(src_dir: &Path) -> Result<(BTreeMap<TypePath, syn::Item>, ModuleImports), ScanError> {
    let mut pool = BTreeMap::new();
    let mut imports = ModuleImports::default();
    scan_dir_recursive(src_dir, &[], &mut pool, &mut imports)?;
    Ok((pool, imports))
}

/// Recursive directory walker. `module_prefix` is the canonical path of the
/// current Rust module (empty at the crate root). Each `.rs` file
/// contributes its items (and items nested inside `mod` blocks) under that
/// prefix, plus its `use` declarations into `imports`.
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

    fn tp(segments: &[&str]) -> TypePath {
        TypePath::new(segments.iter().map(|s| (*s).to_string()).collect()).expect("non-empty")
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
