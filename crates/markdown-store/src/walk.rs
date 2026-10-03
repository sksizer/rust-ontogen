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
/// OKF's reserved `index` and `log` files (any configured extension, any
/// depth, any ASCII case) are skipped: they are directory listings and
/// update logs, not records, and their ids could never be read back anyway
/// (see [`crate::layout::validate_id`]).
///
/// A missing directory yields `Ok(vec![])` — a store whose entity directory
/// hasn't been created yet is empty, not broken.
pub fn list_record_paths(dir: &Path, opts: &WalkOptions) -> Result<Vec<PathBuf>, Error> {
    let mut paths = walk_files(dir, opts, |path| is_record_file(path, opts))?;
    sort_by_id(&mut paths);
    Ok(paths)
}

fn is_record_file(path: &Path, opts: &WalkOptions) -> bool {
    let matches_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| opts.extensions.iter().any(|want| want.eq_ignore_ascii_case(ext)));
    let reserved = path.file_stem().and_then(|s| s.to_str()).is_some_and(crate::layout::is_reserved_id);
    matches_ext && !reserved
}

fn sort_by_id(paths: &mut [PathBuf]) {
    paths.sort_by(|a, b| a.with_extension("").cmp(&b.with_extension("")).then_with(|| a.cmp(b)));
}

/// The record files directly in `dir`, sorted as [`list_record_paths`]
/// sorts them, and its direct subdirectories, sorted by path, both seen
/// through `opts` the way a listing of `dir` sees them.
#[cfg(feature = "store")]
pub(crate) fn list_children(dir: &Path, opts: &WalkOptions) -> Result<(Vec<PathBuf>, Vec<PathBuf>), Error> {
    let (mut records, mut subdirs) = (Vec::new(), Vec::new());
    if !dir.exists() {
        return Ok((records, subdirs));
    }
    let depth = opts.max_depth.map_or(1, |d| d.min(1));
    for entry in builder(dir, opts, Some(depth)).build() {
        let entry = entry.map_err(|e| walk_error(dir, e))?;
        if entry.depth() == 0 {
            continue;
        }
        match entry.file_type() {
            Some(t) if t.is_dir() => subdirs.push(entry.into_path()),
            Some(t) if t.is_file() && is_record_file(entry.path(), opts) => records.push(entry.into_path()),
            _ => {}
        }
    }
    sort_by_id(&mut records);
    subdirs.sort();
    Ok((records, subdirs))
}

/// Whether a listing of `dir` limited to `max_depth` would find a record,
/// stopping at the first one rather than walking the whole tree.
#[cfg(feature = "store")]
pub(crate) fn contains_record(dir: &Path, opts: &WalkOptions, max_depth: Option<usize>) -> Result<bool, Error> {
    if !dir.exists() {
        return Ok(false);
    }
    for entry in builder(dir, opts, max_depth).build() {
        let entry = entry.map_err(|e| walk_error(dir, e))?;
        if entry.file_type().is_some_and(|t| t.is_file()) && is_record_file(entry.path(), opts) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the walk finds a file anywhere under `dir`, at any depth, other
/// than an `index.md`, stopping at the first one.
#[cfg(feature = "store")]
pub(crate) fn holds_non_index_file(dir: &Path, opts: &WalkOptions) -> Result<bool, Error> {
    if !dir.is_dir() {
        return Ok(false);
    }
    for entry in builder(dir, opts, None).build() {
        let entry = entry.map_err(|e| walk_error(dir, e))?;
        if entry.file_type().is_some_and(|t| t.is_file())
            && entry.file_name() != std::ffi::OsStr::new(crate::okf::INDEX_FILE)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Every `index.md` the store may have written under `dir`, sorted, walked
/// with the same options as records so a rebuild sees the vault the way a
/// listing does.
#[cfg(feature = "store")]
pub(crate) fn list_index_paths(dir: &Path, opts: &WalkOptions) -> Result<Vec<PathBuf>, Error> {
    let mut paths = walk_files(dir, opts, |path| path.file_name().is_some_and(|n| n == crate::okf::INDEX_FILE))?;
    paths.sort();
    Ok(paths)
}

fn walk_files(dir: &Path, opts: &WalkOptions, keep: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>, Error> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in builder(dir, opts, opts.max_depth).build() {
        let entry = entry.map_err(|e| walk_error(dir, e))?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.into_path();
        if keep(&path) {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn builder(dir: &Path, opts: &WalkOptions, max_depth: Option<usize>) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(dir);
    builder.follow_links(opts.follow_symlinks).max_depth(max_depth).standard_filters(opts.respect_gitignore);
    builder
}

fn walk_error(dir: &Path, e: ignore::Error) -> Error {
    Error::Io { path: dir.to_path_buf(), source: std::io::Error::other(e.to_string()) }
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
    fn okf_index_and_log_files_are_never_listed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("index.md"));
        touch(&root.join("log.md"));
        touch(&root.join("record.md"));
        touch(&root.join("nested/Index.markdown"));
        touch(&root.join("nested/LOG.md"));
        touch(&root.join("nested/changelog.md"));
        touch(&root.join("log/entry.md"));

        let paths = list_record_paths(root, &WalkOptions::default()).unwrap();
        let names: Vec<String> =
            paths.iter().map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["log/entry.md", "nested/changelog.md", "record.md"]);
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

    #[cfg(feature = "store")]
    #[test]
    fn children_and_record_probes_see_what_a_listing_sees() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("x-2.md"));
        touch(&root.join("x.md"));
        touch(&root.join("index.md"));
        touch(&root.join("notes.txt"));
        touch(&root.join("full/deep/er/leaf.md"));
        touch(&root.join("bare/notes.txt"));
        touch(&root.join("bare/index.md"));
        touch(&root.join(".hidden/secret.md"));

        let opts = WalkOptions::default();
        let (records, subdirs) = list_children(root, &opts).unwrap();
        let names = |paths: &[PathBuf]| -> Vec<String> {
            paths.iter().map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned()).collect()
        };
        assert_eq!(names(&records), ["x.md", "x-2.md"], "direct records only, in id order");
        assert_eq!(names(&subdirs), ["bare", "full"], "hidden directories are skipped");

        assert!(contains_record(&root.join("full"), &opts, None).unwrap());
        assert!(!contains_record(&root.join("full"), &opts, Some(2)).unwrap(), "the depth limit applies");
        assert!(!contains_record(&root.join("bare"), &opts, None).unwrap(), "an index is no record");
        assert!(!contains_record(&root.join("missing"), &opts, None).unwrap());

        assert!(holds_non_index_file(&root.join("bare"), &opts).unwrap(), "any file but an index counts");
        assert!(holds_non_index_file(&root.join("full"), &opts).unwrap(), "at any depth");
        touch(&root.join("only-index/sub/index.md"));
        touch(&root.join("only-hidden/.DS_Store"));
        for empty in ["only-index", "only-hidden", "missing", "x.md"] {
            assert!(!holds_non_index_file(&root.join(empty), &opts).unwrap(), "{empty}");
        }
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
