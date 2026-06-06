//! The vault store layer: [`VaultHandle`], the per-vault façade that
//! generated CRUD code (and hand-written consumers) operate through.
//!
//! A handle is cheap to clone; clones share one intra-process write lock so
//! two async tasks read-modify-writing the same vault serialize instead of
//! losing updates. That is the extent of the concurrency story by design:
//! the backend assumes a single process owns the vault (ADR 0001), and the
//! rename-based atomicity in [`crate::fsops`] protects readers, not
//! concurrent writers in other processes.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use crate::{
    error::Error,
    frontmatter::Document,
    fsops,
    id::IdStrategy,
    layout::VaultLayout,
    walk::{self, WalkOptions},
};

/// Default cap on records per `list` operation. ADR 0001 pins the markdown
/// backend's comfort zone at "N in the low thousands per entity"; the cap
/// makes exceeding it a loud error instead of a slow surprise.
pub const DEFAULT_LIST_CAP: usize = 10_000;

/// Handle to one markdown vault: root path, layout, id strategy, walk
/// options, list cap, and the shared write lock.
///
/// ```
/// use markdown_store::{Document, IdStrategy, VaultHandle, VaultLayout};
///
/// let dir = tempfile::tempdir().unwrap();
/// let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir, IdStrategy::Provided);
///
/// let mut doc = Document::new();
/// doc.set("title", "First note");
/// doc.set_body("Hello.\n");
/// vault.create_record("notes", "n-1", &doc)?;
///
/// let read = vault.read_record("notes", "n-1")?;
/// assert_eq!(read.get("title").and_then(|v| v.as_str()), Some("First note"));
/// assert_eq!(vault.list_ids("notes")?, vec!["n-1".to_string()]);
/// # Ok::<(), markdown_store::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct VaultHandle {
    root: PathBuf,
    layout: VaultLayout,
    id_strategy: IdStrategy,
    walk: WalkOptions,
    list_cap: usize,
    write_guard: Arc<Mutex<()>>,
}

impl VaultHandle {
    /// Create a handle. The root does not need to exist yet — it is created
    /// on first write.
    pub fn new(root: impl Into<PathBuf>, layout: VaultLayout, id_strategy: IdStrategy) -> Self {
        Self {
            root: root.into(),
            layout,
            id_strategy,
            walk: WalkOptions::default(),
            list_cap: DEFAULT_LIST_CAP,
            write_guard: Arc::new(Mutex::new(())),
        }
    }

    /// Override the per-list record cap.
    pub fn with_list_cap(mut self, cap: usize) -> Self {
        self.list_cap = cap;
        self
    }

    /// Override the walk options used for listing.
    pub fn with_walk_options(mut self, walk: WalkOptions) -> Self {
        self.walk = walk;
        self
    }

    /// The vault root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The configured layout.
    pub fn layout(&self) -> VaultLayout {
        self.layout
    }

    /// The configured id strategy.
    pub fn id_strategy(&self) -> &IdStrategy {
        &self.id_strategy
    }

    /// The configured per-list cap.
    pub fn list_cap(&self) -> usize {
        self.list_cap
    }

    // ── paths ───────────────────────────────────────────────────────────

    /// Resolve (and validate) the file path for a record.
    pub fn record_path(&self, dir_segment: &str, id: &str) -> Result<PathBuf, Error> {
        self.layout.record_path(&self.root, dir_segment, id)
    }

    /// Resolve the directory an entity's records live in.
    pub fn entity_dir(&self, dir_segment: &str) -> Result<PathBuf, Error> {
        self.layout.entity_dir(&self.root, dir_segment)
    }

    // ── single-record ops ───────────────────────────────────────────────

    /// Whether a record exists.
    pub fn record_exists(&self, dir_segment: &str, id: &str) -> Result<bool, Error> {
        Ok(fsops::exists(&self.record_path(dir_segment, id)?))
    }

    /// Read and parse one record. Missing record is [`Error::NotFound`].
    pub fn read_record(&self, dir_segment: &str, id: &str) -> Result<Document, Error> {
        let path = self.record_path(dir_segment, id)?;
        let raw = fsops::read(&path)?;
        Document::parse(&raw).map_err(|e| Error::parse_at(&path, e))
    }

    /// Read and parse one record; missing record is `Ok(None)`.
    pub fn read_record_opt(&self, dir_segment: &str, id: &str) -> Result<Option<Document>, Error> {
        match self.read_record(dir_segment, id) {
            Ok(doc) => Ok(Some(doc)),
            Err(Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Create a record. Fails with [`Error::AlreadyExists`] if the file is
    /// already present — creation never overwrites. The existence check and
    /// write happen under the vault's write lock.
    pub fn create_record(&self, dir_segment: &str, id: &str, doc: &Document) -> Result<(), Error> {
        let path = self.record_path(dir_segment, id)?;
        let _guard = self.lock();
        if fsops::exists(&path) {
            return Err(Error::AlreadyExists { path });
        }
        fsops::write_atomic(&path, &doc.render()?)
    }

    /// Write a record unconditionally (create-or-replace), atomically and
    /// under the write lock. Prefer [`create_record`](Self::create_record)
    /// for inserts so duplicate ids fail loudly.
    pub fn write_record(&self, dir_segment: &str, id: &str, doc: &Document) -> Result<(), Error> {
        let path = self.record_path(dir_segment, id)?;
        let _guard = self.lock();
        fsops::write_atomic(&path, &doc.render()?)
    }

    /// Read-modify-write one record under the write lock. The mutation
    /// closure receives the parsed [`Document`]; on `Ok` the document is
    /// re-rendered and atomically written back.
    pub fn modify_record<F>(&self, dir_segment: &str, id: &str, f: F) -> Result<(), Error>
    where
        F: FnOnce(&mut Document) -> Result<(), Error>,
    {
        let path = self.record_path(dir_segment, id)?;
        let _guard = self.lock();
        fsops::read_modify_write(&path, f)
    }

    /// Remove a record under the write lock. Missing record is
    /// [`Error::NotFound`].
    pub fn remove_record(&self, dir_segment: &str, id: &str) -> Result<(), Error> {
        let path = self.record_path(dir_segment, id)?;
        let _guard = self.lock();
        fsops::remove(&path)
    }

    // ── listing ─────────────────────────────────────────────────────────

    /// List record file paths for an entity, sorted lexicographically.
    /// Exceeding the configured cap is [`Error::ListCapExceeded`].
    pub fn list_paths(&self, dir_segment: &str) -> Result<Vec<PathBuf>, Error> {
        let dir = self.entity_dir(dir_segment)?;
        let paths = walk::list_record_paths(&dir, &self.walk)?;
        if paths.len() > self.list_cap {
            return Err(Error::ListCapExceeded { dir, count: paths.len(), cap: self.list_cap });
        }
        Ok(paths)
    }

    /// List record ids (file stems) for an entity, sorted.
    pub fn list_ids(&self, dir_segment: &str) -> Result<Vec<String>, Error> {
        Ok(self
            .list_paths(dir_segment)?
            .iter()
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
            .collect())
    }

    /// Read and parse every record of an entity, as sorted `(id, document)`
    /// pairs. This is the `list()` workhorse: parse errors fail the whole
    /// listing rather than silently hiding records (a vault is
    /// human-edited; hiding a broken file would misreport the dataset).
    pub fn read_all(&self, dir_segment: &str) -> Result<Vec<(String, Document)>, Error> {
        let mut out = Vec::new();
        for path in self.list_paths(dir_segment)? {
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            let raw = fsops::read(&path)?;
            let doc = Document::parse(&raw).map_err(|e| Error::parse_at(&path, e))?;
            out.push((id, doc));
        }
        Ok(out)
    }

    // ── id derivation ───────────────────────────────────────────────────

    /// Derive an id for a new record via the vault's [`IdStrategy`], then
    /// (for derived ids) de-duplicate against existing records by appending
    /// `-2`, `-3`, … . Caller-supplied ids are returned as-is — a duplicate
    /// surfaces later as [`Error::AlreadyExists`] from
    /// [`create_record`](Self::create_record), because silently renaming an
    /// explicit id would be worse than failing.
    pub fn make_record_id(
        &self,
        dir_segment: &str,
        provided: Option<&str>,
        source_value: Option<&str>,
    ) -> Result<String, Error> {
        let had_provided = provided.is_some_and(|p| !p.trim().is_empty());
        let base = self.id_strategy.make_id(provided, source_value)?;
        crate::layout::validate_id(&base)?;
        if had_provided {
            return Ok(base);
        }
        self.ensure_unique_id(dir_segment, &base)
    }

    /// Return `base` if no record with that id exists, otherwise the first
    /// free `base-2`, `base-3`, … .
    pub fn ensure_unique_id(&self, dir_segment: &str, base: &str) -> Result<String, Error> {
        if !self.record_exists(dir_segment, base)? {
            return Ok(base.to_string());
        }
        for n in 2.. {
            let candidate = format!("{base}-{n}");
            if !self.record_exists(dir_segment, &candidate)? {
                return Ok(candidate);
            }
        }
        unreachable!("suffix search is unbounded");
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        // A poisoned lock means another write panicked mid-flight; the
        // on-disk state is still consistent (atomic rename), so continuing
        // is safe and refusing all future writes would not be.
        self.write_guard.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(strategy: IdStrategy) -> (tempfile::TempDir, VaultHandle) {
        let dir = tempfile::tempdir().unwrap();
        let handle = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir, strategy);
        (dir, handle)
    }

    fn doc(title: &str) -> Document {
        let mut d = Document::new();
        d.set("title", title);
        d.set_body("body\n");
        d
    }

    #[test]
    fn crud_cycle() {
        let (_dir, vault) = vault(IdStrategy::Provided);

        vault.create_record("tasks", "t-1", &doc("one")).unwrap();
        assert!(vault.record_exists("tasks", "t-1").unwrap());

        let read = vault.read_record("tasks", "t-1").unwrap();
        assert_eq!(read.get("title").and_then(|v| v.as_str()), Some("one"));

        vault
            .modify_record("tasks", "t-1", |d| {
                d.set("title", "one, edited");
                Ok(())
            })
            .unwrap();
        let read = vault.read_record("tasks", "t-1").unwrap();
        assert_eq!(read.get("title").and_then(|v| v.as_str()), Some("one, edited"));

        vault.remove_record("tasks", "t-1").unwrap();
        assert!(matches!(vault.read_record("tasks", "t-1"), Err(Error::NotFound { .. })));
        assert_eq!(vault.read_record_opt("tasks", "t-1").unwrap(), None);
    }

    #[test]
    fn create_never_overwrites() {
        let (_dir, vault) = vault(IdStrategy::Provided);
        vault.create_record("tasks", "t-1", &doc("first")).unwrap();
        let err = vault.create_record("tasks", "t-1", &doc("second")).unwrap_err();
        assert!(matches!(err, Error::AlreadyExists { .. }));
        let read = vault.read_record("tasks", "t-1").unwrap();
        assert_eq!(read.get("title").and_then(|v| v.as_str()), Some("first"), "original untouched");
    }

    #[test]
    fn listing_is_sorted_and_capped() {
        let (_dir, vault) = vault(IdStrategy::Provided);
        for id in ["c", "a", "b"] {
            vault.create_record("tasks", id, &doc(id)).unwrap();
        }
        assert_eq!(vault.list_ids("tasks").unwrap(), vec!["a", "b", "c"]);
        let all = vault.read_all("tasks").unwrap();
        assert_eq!(all.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);

        let capped = vault.clone().with_list_cap(2);
        assert!(matches!(capped.list_paths("tasks"), Err(Error::ListCapExceeded { count: 3, cap: 2, .. })));
    }

    #[test]
    fn listing_missing_entity_dir_is_empty() {
        let (_dir, vault) = vault(IdStrategy::Provided);
        assert_eq!(vault.list_ids("never-written").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn read_all_fails_loudly_on_a_broken_record() {
        let (_dir, vault) = vault(IdStrategy::Provided);
        vault.create_record("tasks", "ok", &doc("fine")).unwrap();
        let bad = vault.record_path("tasks", "bad").unwrap();
        fsops::write_atomic(&bad, "---\n: : : broken\n---\n").unwrap();
        let err = vault.read_all("tasks").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn slug_ids_dedupe_with_suffixes() {
        let (_dir, vault) = vault(IdStrategy::SlugFromField("title".into()));
        let id1 = vault.make_record_id("tasks", None, Some("Same Title")).unwrap();
        vault.create_record("tasks", &id1, &doc("Same Title")).unwrap();
        let id2 = vault.make_record_id("tasks", None, Some("Same Title")).unwrap();
        vault.create_record("tasks", &id2, &doc("Same Title")).unwrap();
        let id3 = vault.make_record_id("tasks", None, Some("Same Title")).unwrap();
        assert_eq!((id1.as_str(), id2.as_str(), id3.as_str()), ("same-title", "same-title-2", "same-title-3"));
    }

    #[test]
    fn provided_ids_are_not_renamed() {
        let (_dir, vault) = vault(IdStrategy::SlugFromField("title".into()));
        vault.create_record("tasks", "fixed", &doc("x")).unwrap();
        // make_record_id with an explicit id must NOT silently dedupe…
        let id = vault.make_record_id("tasks", Some("fixed"), None).unwrap();
        assert_eq!(id, "fixed");
        // …the collision surfaces at create time instead.
        assert!(matches!(vault.create_record("tasks", &id, &doc("y")), Err(Error::AlreadyExists { .. })));
    }

    #[test]
    fn hostile_ids_cannot_escape_the_vault() {
        let (_dir, vault) = vault(IdStrategy::Provided);
        for bad in ["../../etc/passwd", "..", "a/b", ".hidden"] {
            assert!(vault.create_record("tasks", bad, &doc("x")).is_err(), "id {bad:?} must be rejected");
        }
    }

    #[test]
    fn clones_share_the_write_lock() {
        let (_dir, vault) = vault(IdStrategy::Provided);
        vault.create_record("tasks", "t", &doc("start")).unwrap();

        // Run two RMW storms over the same record from two clones; the
        // shared lock makes each increment atomic, so none are lost.
        let a = vault.clone();
        let b = vault.clone();
        let bump = |v: VaultHandle| {
            std::thread::spawn(move || {
                for _ in 0..50 {
                    v.modify_record("tasks", "t", |d| {
                        let n = d.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
                        d.set("count", n + 1);
                        Ok(())
                    })
                    .unwrap();
                }
            })
        };
        let (ta, tb) = (bump(a), bump(b));
        ta.join().unwrap();
        tb.join().unwrap();

        let read = vault.read_record("tasks", "t").unwrap();
        assert_eq!(read.get("count").and_then(|v| v.as_i64()), Some(100), "no lost updates");
    }
}
