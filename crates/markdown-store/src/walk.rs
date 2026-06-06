//! Record-file enumeration with a stable order.
//!
//! Listing is the read side of `list()`: enumerate the record files under an
//! entity directory, **sorted lexicographically by path**, so the backend's
//! documented "stable order" (ADR 0001 contract item 3) is a property of
//! this module rather than of filesystem iteration order, which guarantees
//! nothing.
//!
//! Walking is gitignore-aware by default and mirrors `markdown-vault`'s
//! `WalkOptions` semantics (same `ignore`-crate underpinnings) so the two
//! crates treat the same vault identically.

use std::path::{Path, PathBuf};

use crate::error::Error;

/// Traversal options for [`list_record_paths`].
///
/// Defaults: gitignore/ignore filters on (hidden entries skipped), symlinks
/// not followed, no depth limit, `.md`/`.markdown` files only.
#[derive(Debug, Clone)]
pub struct WalkOptions {
    /// Apply `.gitignore`, `.ignore`, parent ignores, and skip hidden
    /// entries. Off-spec content like `node_modules` drops out as a side
    /// effect. When `false`, the walk is a plain filesystem traversal that
    /// also visits hidden entries.
    pub respect_gitignore: bool,
    /// Follow symbolic links. Off by default to avoid cycles in vaults that
    /// link into themselves.
    pub follow_symlinks: bool,
    /// Maximum recursion depth. `None` means unlimited; `Some(1)` lists only
    /// the directory's direct children.
    pub max_depth: Option<usize>,
    /// File extensions to include (no leading dot, compared
    /// case-insensitively).
    pub extensions: Vec<String>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            follow_symlinks: false,
            max_depth: None,
            extensions: vec!["md".into(), "markdown".into()],
        }
    }
}

/// List record file paths under `dir`, honoring `opts`, sorted
/// lexicographically by **extension-stripped path** — i.e. by record id
/// within a directory. (Sorting raw paths would diverge from id order at
/// suffix boundaries: `x-2.md` < `x.md` because `-` < `.`, yet the ids sort
/// `x` < `x-2`.) Full path breaks ties.
///
/// A missing directory yields `Ok(vec![])` — a store whose entity directory
/// hasn't been created yet is empty, not broken.
pub fn list_record_paths(dir: &Path, opts: &WalkOptions) -> Result<Vec<PathBuf>, Error> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut builder = ignore::WalkBuilder::new(dir);
    builder.follow_links(opts.follow_symlinks).max_depth(opts.max_depth).standard_filters(opts.respect_gitignore);

    let mut paths = Vec::new();
    for entry in builder.build() {
        let entry =
            entry.map_err(|e| Error::Io { path: dir.to_path_buf(), source: std::io::Error::other(e.to_string()) })?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.into_path();
        let matches_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| opts.extensions.iter().any(|want| want.eq_ignore_ascii_case(ext)));
        if matches_ext {
            paths.push(path);
        }
    }
    paths.sort_by(|a, b| a.with_extension("").cmp(&b.with_extension("")).then_with(|| a.cmp(b)));
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }

    #[test]
    fn missing_dir_is_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert_eq!(list_record_paths(&missing, &WalkOptions::default()).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn lists_sorted_and_filters_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("b.md"));
        touch(&root.join("a.md"));
        touch(&root.join("c.markdown"));
        touch(&root.join("notes.txt"));
        touch(&root.join("nested/d.MD"));

        let paths = list_record_paths(root, &WalkOptions::default()).unwrap();
        let names: Vec<String> =
            paths.iter().map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["a.md", "b.md", "c.markdown", "nested/d.MD"]);
    }

    #[test]
    fn sort_order_matches_id_order_at_suffix_boundaries() {
        // Raw path order would put "x-2.md" before "x.md" ('-' < '.');
        // id order is "x" < "x-2". The listing must follow id order.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("x-2.md"));
        touch(&root.join("x.md"));
        touch(&root.join("x-10.md"));

        let paths = list_record_paths(root, &WalkOptions::default()).unwrap();
        let stems: Vec<&str> = paths.iter().filter_map(|p| p.file_stem().and_then(|s| s.to_str())).collect();
        assert_eq!(stems, vec!["x", "x-10", "x-2"], "lexicographic by id, not by raw path");
    }

    #[test]
    fn hidden_files_skipped_by_default_included_when_raw() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join(".hidden.md"));
        touch(&root.join("visible.md"));

        let default = list_record_paths(root, &WalkOptions::default()).unwrap();
        assert_eq!(default.len(), 1, "hidden file must be skipped: {default:?}");

        let raw = WalkOptions { respect_gitignore: false, ..WalkOptions::default() };
        let all = list_record_paths(root, &raw).unwrap();
        assert_eq!(all.len(), 2, "raw walk sees hidden files: {all:?}");
    }

    #[test]
    fn gitignore_respected_inside_git_repos() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // `ignore` applies .gitignore when the tree looks like a repo.
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "drafts/\n").unwrap();
        touch(&root.join("drafts/skipme.md"));
        touch(&root.join("keep.md"));

        let paths = list_record_paths(root, &WalkOptions::default()).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("keep.md"));
    }

    #[test]
    fn max_depth_limits_recursion() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("top.md"));
        touch(&root.join("sub/deep.md"));

        let shallow = WalkOptions { max_depth: Some(1), ..WalkOptions::default() };
        let paths = list_record_paths(root, &shallow).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("top.md"));
    }
}
