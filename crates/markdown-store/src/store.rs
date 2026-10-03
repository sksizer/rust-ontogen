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

/// Handle to one markdown vault: root path, layout, walk options, list cap,
/// and the shared write lock.
///
/// The id strategy is not part of the handle: each create names the
/// [`IdStrategy`] it derives ids by, so a code generator's build-time choice
/// is the only one there is.
///
/// ```
/// use markdown_store::{Document, VaultHandle, VaultLayout};
///
/// let dir = tempfile::tempdir().unwrap();
/// let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir);
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
    walk: WalkOptions,
    list_cap: usize,
    write_guard: Arc<Mutex<()>>,
}

impl VaultHandle {
    /// Create a handle. The root does not need to exist yet — it is created
    /// on first write.
    pub fn new(root: impl Into<PathBuf>, layout: VaultLayout) -> Self {
        Self {
            root: root.into(),
            layout,
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

    /// The records of one entity: its directory segment plus the OKF `type`
    /// its records carry. Generated store code goes through this view so
    /// every write is typed and, under [`VaultLayout::Flat`], every read is
    /// filtered to the entity's own records.
    pub fn entity<'a>(&'a self, dir_segment: &'a str, type_name: &'a str) -> EntityRecords<'a> {
        EntityRecords { vault: self, dir_segment, type_name }
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
    /// write happen under the vault's write lock. (The check is racy against
    /// writers in *other processes*; single-process ownership of a vault is
    /// the documented stance.)
    pub fn create_record(&self, dir_segment: &str, id: &str, doc: &Document) -> Result<(), Error> {
        let path = self.record_path(dir_segment, id)?;
        let _guard = self.lock();
        if fsops::exists(&path) {
            return Err(Error::AlreadyExists { path });
        }
        fsops::write_atomic(&path, &doc.render()?)
    }

    /// Create a record whose id is derived by `strategy`, atomically: id derivation, slug de-duplication, and the write all
    /// happen under one hold of the write lock, so two concurrent creates of
    /// the same slug yield `base` and `base-2` instead of racing into
    /// [`Error::AlreadyExists`]. **This is the create path generated store
    /// code uses.** Returns the id the record was created under.
    ///
    /// `provided` follows [`IdStrategy::make_id`] semantics: a non-empty
    /// caller-supplied id always wins and is *not* de-duplicated — an
    /// explicit duplicate fails with [`Error::AlreadyExists`], because
    /// silently renaming an explicit id would be worse than failing. For
    /// the same reason an explicit reserved id (`index`, `log`) is
    /// [`Error::InvalidId`], while a *derived* one is treated as taken and
    /// becomes `index-2`. `source_value` feeds [`IdStrategy::SlugFromField`].
    pub fn create_record_derived(
        &self,
        dir_segment: &str,
        strategy: &IdStrategy,
        provided: Option<&str>,
        source_value: Option<&str>,
        doc: &Document,
    ) -> Result<String, Error> {
        let _guard = self.lock();
        let id = self.derive_id(dir_segment, strategy, provided, source_value)?;
        let path = self.record_path(dir_segment, &id)?;
        if fsops::exists(&path) {
            return Err(Error::AlreadyExists { path });
        }
        fsops::write_atomic(&path, &doc.render()?)?;
        Ok(id)
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

    /// List record ids (file stems) for an entity, in id order: ascending
    /// by the ids' UTF-8 bytes, which is Unicode code point order (`"B"`
    /// before `"a"`, `"z"` before `"é"`). That order is the contract,
    /// whatever the layout: ontogen's SQL backend lists in the same order
    /// (`ORDER BY id` under binary collation), so the two agree.
    pub fn list_ids(&self, dir_segment: &str) -> Result<Vec<String>, Error> {
        Ok(self.ids_and_paths(dir_segment)?.into_iter().map(|(id, _)| id).collect())
    }

    /// Read and parse every record of an entity, as `(id, document)` pairs in
    /// id order (see [`list_ids`](Self::list_ids)). This is the `list()`
    /// workhorse: parse errors fail the whole listing rather than silently
    /// hiding records (a vault is human-edited; hiding a broken file would
    /// misreport the dataset).
    pub fn read_all(&self, dir_segment: &str) -> Result<Vec<(String, Document)>, Error> {
        let mut out = Vec::new();
        for (id, path) in self.ids_and_paths(dir_segment)? {
            let raw = fsops::read(&path)?;
            let doc = Document::parse(&raw).map_err(|e| Error::parse_at(&path, e))?;
            out.push((id, doc));
        }
        Ok(out)
    }

    /// Record ids with their paths, sorted by id bytes. The walk's own order
    /// is by path, which differs from id order once records sit in nested
    /// directories; the path breaks ties between equal stems.
    fn ids_and_paths(&self, dir_segment: &str) -> Result<Vec<(String, PathBuf)>, Error> {
        let mut out: Vec<(String, PathBuf)> = self
            .list_paths(dir_segment)?
            .into_iter()
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string).map(|id| (id, p)))
            .collect();
        out.sort();
        Ok(out)
    }

    // ── id derivation ───────────────────────────────────────────────────

    /// Preview the id a new record would get via `strategy`,
    /// de-duplicating derived ids (`base-2`, `base-3`, …) against existing
    /// records. Caller-supplied ids are returned as-is.
    ///
    /// **Not atomic with a subsequent create**: between this call and
    /// [`create_record`](Self::create_record), another task can take the id.
    /// Use [`create_record_derived`](Self::create_record_derived) — which
    /// holds the write lock across derivation *and* write — whenever the
    /// record is actually being created; this method is for previews and
    /// dry runs.
    pub fn make_record_id(
        &self,
        dir_segment: &str,
        strategy: &IdStrategy,
        provided: Option<&str>,
        source_value: Option<&str>,
    ) -> Result<String, Error> {
        self.derive_id(dir_segment, strategy, provided, source_value)
    }

    /// Return `base` if no record with that id exists, otherwise the first
    /// free `base-2`, `base-3`, … . Same non-atomicity caveat as
    /// [`make_record_id`](Self::make_record_id): pair with
    /// [`create_record_derived`](Self::create_record_derived) for the
    /// race-free create path.
    pub fn ensure_unique_id(&self, dir_segment: &str, base: &str) -> Result<String, Error> {
        self.next_free_id(dir_segment, base)
    }

    /// Id derivation shared by [`make_record_id`](Self::make_record_id) and
    /// the locked create path.
    fn derive_id(
        &self,
        dir_segment: &str,
        strategy: &IdStrategy,
        provided: Option<&str>,
        source_value: Option<&str>,
    ) -> Result<String, Error> {
        let had_provided = provided.is_some_and(|p| !p.trim().is_empty());
        let base = strategy.make_id(provided, source_value)?;
        if had_provided {
            crate::layout::validate_id(&base)?;
            return Ok(base);
        }
        self.next_free_id(dir_segment, &base)
    }

    /// Suffix search shared by the public previews and the locked create
    /// path. Named for how [`create_record_derived`] uses it — the *caller*
    /// is responsible for holding the lock when atomicity matters; the
    /// probe itself is just existence checks. A reserved base counts as
    /// taken, so a title that slugs to `index` lands on `index-2`.
    fn next_free_id(&self, dir_segment: &str, base: &str) -> Result<String, Error> {
        if !crate::layout::is_reserved_id(base) && !self.record_exists(dir_segment, base)? {
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

/// One entity's records within a vault, from [`VaultHandle::entity`].
///
/// Writes stamp the OKF `type` (see [`Document::stamp_type`]): a create
/// always carries it as the first key, and an update that changes something
/// adds a missing `type` or corrects a different one. An update that
/// changes nothing writes nothing, so untyped legacy records are only
/// typed when they are next really edited.
///
/// Reads never require `type`: an untyped record belongs to whichever
/// entity looks it up. What a record whose `type` *differs* means depends on
/// the layout:
///
/// - [`VaultLayout::PerEntityDir`]: the directory decides what a record is.
///   A differing `type` is tolerated (OKF consumers must not reject unknown
///   types) and normalized on the next real write. Nothing is parsed to
///   decide membership, so [`count`](Self::count) stays a directory walk.
/// - [`VaultLayout::Flat`]: every entity shares the root, so `type` is the
///   only discriminator. A record typed as another entity is invisible:
///   excluded from [`read_all`](Self::read_all) and [`count`](Self::count).
///   [`read_opt`](Self::read_opt) returns `None`; [`modify`](Self::modify)
///   and [`remove`](Self::remove) return [`Error::NotFound`], so one entity
///   can never rewrite or delete another's record.
///
/// ```
/// use markdown_store::{Document, IdStrategy, VaultHandle, VaultLayout};
///
/// let dir = tempfile::tempdir().unwrap();
/// let vault = VaultHandle::new(dir.path(), VaultLayout::Flat);
/// let (tasks, notes) = (vault.entity("tasks", "Task"), vault.entity("notes", "Note"));
///
/// let mut doc = Document::new();
/// doc.set("title", "Ship it");
/// tasks.create(&IdStrategy::Provided, Some("t-1"), None, doc)?;
///
/// assert!(vault.read_record("tasks", "t-1")?.render()?.starts_with("---\ntype: Task\n"));
/// assert_eq!(tasks.count()?, 1);
/// assert_eq!(notes.count()?, 0, "a flat vault filters by type");
/// assert!(notes.read_opt("t-1")?.is_none());
/// # Ok::<(), markdown_store::Error>(())
/// ```
#[derive(Debug, Clone, Copy)]
pub struct EntityRecords<'a> {
    vault: &'a VaultHandle,
    dir_segment: &'a str,
    type_name: &'a str,
}

impl<'a> EntityRecords<'a> {
    /// The entity's directory segment.
    pub fn dir_segment(&self) -> &'a str {
        self.dir_segment
    }

    /// The OKF `type` this entity's records carry.
    pub fn type_name(&self) -> &'a str {
        self.type_name
    }

    /// Whether a parsed record belongs to this entity under the vault's
    /// layout: always under [`VaultLayout::PerEntityDir`]; under
    /// [`VaultLayout::Flat`], when it has no `type` or this entity's.
    fn admits(&self, doc: &Document) -> bool {
        match self.vault.layout {
            VaultLayout::PerEntityDir => true,
            VaultLayout::Flat => match doc.get(crate::frontmatter::TYPE_KEY) {
                None | Some(serde_norway::Value::Null) => true,
                Some(_) => doc.type_name() == Some(self.type_name),
            },
        }
    }

    /// Every record of this entity, as `(id, document)` pairs in id order.
    /// Same order and failure semantics as [`VaultHandle::read_all`].
    pub fn read_all(&self) -> Result<Vec<(String, Document)>, Error> {
        let mut all = self.vault.read_all(self.dir_segment)?;
        if self.vault.layout == VaultLayout::Flat {
            all.retain(|(_, doc)| self.admits(doc));
        }
        Ok(all)
    }

    /// How many records this entity has. Under
    /// [`VaultLayout::PerEntityDir`] this walks the directory without
    /// reading a file; under [`VaultLayout::Flat`] it must parse each one to
    /// see its `type`.
    pub fn count(&self) -> Result<usize, Error> {
        match self.vault.layout {
            VaultLayout::PerEntityDir => Ok(self.vault.list_paths(self.dir_segment)?.len()),
            VaultLayout::Flat => Ok(self.read_all()?.len()),
        }
    }

    /// Read one record; missing, or another entity's under
    /// [`VaultLayout::Flat`], is `Ok(None)`.
    pub fn read_opt(&self, id: &str) -> Result<Option<Document>, Error> {
        Ok(self.vault.read_record_opt(self.dir_segment, id)?.filter(|doc| self.admits(doc)))
    }

    /// Create a record with [`VaultHandle::create_record_derived`]
    /// semantics, typed: `type` becomes the document's first key.
    pub fn create(
        &self,
        strategy: &IdStrategy,
        provided: Option<&str>,
        source_value: Option<&str>,
        mut doc: Document,
    ) -> Result<String, Error> {
        doc.ensure_type(self.type_name);
        self.vault.create_record_derived(self.dir_segment, strategy, provided, source_value, &doc)
    }

    /// Read-modify-write one record under the write lock, then stamp its
    /// `type` if the mutation changed anything. A clean document is not
    /// written at all.
    pub fn modify<F>(&self, id: &str, f: F) -> Result<(), Error>
    where
        F: FnOnce(&mut Document) -> Result<(), Error>,
    {
        let path = self.vault.record_path(self.dir_segment, id)?;
        let _guard = self.vault.lock();
        fsops::read_modify_write(&path, |doc| {
            if !self.admits(doc) {
                return Err(Error::NotFound { path: path.clone() });
            }
            f(doc)?;
            doc.stamp_type(self.type_name);
            Ok(())
        })
    }

    /// Remove one record under the write lock. Missing, or another
    /// entity's under [`VaultLayout::Flat`], is [`Error::NotFound`].
    ///
    /// Under [`VaultLayout::Flat`] a record whose frontmatter does not parse
    /// cannot be removed here ([`Error::Parse`]): without its `type` there is
    /// no telling whose it is, and deleting another entity's file is worse
    /// than refusing. Fix the frontmatter or delete the file by hand.
    pub fn remove(&self, id: &str) -> Result<(), Error> {
        let path = self.vault.record_path(self.dir_segment, id)?;
        let _guard = self.vault.lock();
        if self.vault.layout == VaultLayout::Flat {
            let doc = Document::parse(&fsops::read(&path)?).map_err(|e| Error::parse_at(&path, e))?;
            if !self.admits(&doc) {
                return Err(Error::NotFound { path });
            }
        }
        fsops::remove(&path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, VaultHandle) {
        let dir = tempfile::tempdir().unwrap();
        let handle = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir);
        (dir, handle)
    }

    fn slug() -> IdStrategy {
        IdStrategy::SlugFromField("title".into())
    }

    fn doc(title: &str) -> Document {
        let mut d = Document::new();
        d.set("title", title);
        d.set_body("body\n");
        d
    }

    #[test]
    fn crud_cycle() {
        let (_dir, vault) = vault();

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
        let (_dir, vault) = vault();
        vault.create_record("tasks", "t-1", &doc("first")).unwrap();
        let err = vault.create_record("tasks", "t-1", &doc("second")).unwrap_err();
        assert!(matches!(err, Error::AlreadyExists { .. }));
        let read = vault.read_record("tasks", "t-1").unwrap();
        assert_eq!(read.get("title").and_then(|v| v.as_str()), Some("first"), "original untouched");
    }

    #[test]
    fn listing_is_sorted_and_capped() {
        let (_dir, vault) = vault();
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
    fn listing_is_in_id_byte_order_even_across_nested_directories() {
        let (_dir, vault) = vault();
        for id in ["z", "é", "a-b", "B", "a"] {
            vault.create_record("tasks", id, &doc(id)).unwrap();
        }
        // A walk sorts by path, which puts `e` before `nested/d`; id order
        // does not care where a record sits.
        seed(&vault, "tasks", "e", "---\ntitle: e\n---\n");
        let nested = vault.entity_dir("tasks").unwrap().join("nested/d.md");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, "---\ntitle: d\n---\n").unwrap();

        let expected = vec!["B", "a", "a-b", "d", "e", "z", "é"];
        assert_eq!(vault.list_ids("tasks").unwrap(), expected);
        let all = vault.read_all("tasks").unwrap();
        assert_eq!(all.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), expected);
        let typed = vault.entity("tasks", "Task").read_all().unwrap();
        assert_eq!(typed.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn a_create_with_no_id_to_derive_is_id_required() {
        let (_dir, vault) = vault();
        let err = vault.create_record_derived("tasks", &IdStrategy::Provided, None, None, &doc("x")).unwrap_err();
        assert!(matches!(&err, Error::IdRequired { reason } if reason.contains("supply an id")), "{err}");
        let err = vault.entity("tasks", "Task").create(&slug(), Some(" "), Some("!!!"), doc("!!!")).unwrap_err();
        assert!(matches!(&err, Error::IdRequired { reason } if reason.contains("empty slug")), "{err}");
        assert_eq!(vault.list_ids("tasks").unwrap(), Vec::<String>::new(), "nothing was written");
    }

    #[test]
    fn whitespace_only_ids_are_invalid() {
        let (_dir, vault) = vault();
        for id in ["\t", "\n", " \t"] {
            assert!(matches!(vault.create_record("tasks", id, &doc("x")), Err(Error::InvalidId { .. })), "{id:?}");
            assert!(matches!(vault.read_record_opt("tasks", id), Err(Error::InvalidId { .. })), "{id:?}");
        }
    }

    #[test]
    fn listing_missing_entity_dir_is_empty() {
        let (_dir, vault) = vault();
        assert_eq!(vault.list_ids("never-written").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn read_all_fails_loudly_on_a_broken_record() {
        let (_dir, vault) = vault();
        vault.create_record("tasks", "ok", &doc("fine")).unwrap();
        let bad = vault.record_path("tasks", "bad").unwrap();
        fsops::write_atomic(&bad, "---\n: : : broken\n---\n").unwrap();
        let err = vault.read_all("tasks").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn concurrent_same_slug_creates_dedupe_atomically() {
        // Regression: derivation + dedup + write must share one lock hold.
        // The pre-fix shape (make_record_id outside the lock, then
        // create_record) lost the race every time: both threads derived the
        // same id and one got AlreadyExists instead of `-2`.
        for _ in 0..25 {
            let (_dir, vault) = vault();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let spawn = |v: VaultHandle, b: std::sync::Arc<std::sync::Barrier>| {
                std::thread::spawn(move || {
                    let mut d = Document::new();
                    d.set("title", "Same Title");
                    b.wait();
                    v.create_record_derived("tasks", &slug(), None, Some("Same Title"), &d).unwrap()
                })
            };
            let h1 = spawn(vault.clone(), barrier.clone());
            let h2 = spawn(vault.clone(), barrier.clone());
            let mut ids = vec![h1.join().unwrap(), h2.join().unwrap()];
            ids.sort();
            assert_eq!(ids, vec!["same-title", "same-title-2"], "both racers must land, deduped");
        }
    }

    #[test]
    fn create_record_derived_with_explicit_id_never_renames() {
        let (_dir, vault) = vault();
        let id = vault.create_record_derived("tasks", &slug(), Some("fixed"), Some("ignored"), &doc("x")).unwrap();
        assert_eq!(id, "fixed");
        // An explicit duplicate fails rather than silently suffixing.
        let err = vault.create_record_derived("tasks", &slug(), Some("fixed"), None, &doc("y")).unwrap_err();
        assert!(matches!(err, Error::AlreadyExists { .. }));
    }

    #[test]
    fn slug_ids_dedupe_with_suffixes() {
        let (_dir, vault) = vault();
        let id1 = vault.make_record_id("tasks", &slug(), None, Some("Same Title")).unwrap();
        vault.create_record("tasks", &id1, &doc("Same Title")).unwrap();
        let id2 = vault.make_record_id("tasks", &slug(), None, Some("Same Title")).unwrap();
        vault.create_record("tasks", &id2, &doc("Same Title")).unwrap();
        let id3 = vault.make_record_id("tasks", &slug(), None, Some("Same Title")).unwrap();
        assert_eq!((id1.as_str(), id2.as_str(), id3.as_str()), ("same-title", "same-title-2", "same-title-3"));
    }

    #[test]
    fn derived_reserved_ids_dedupe_like_a_collision() {
        let (_dir, vault) = vault();
        assert_eq!(vault.make_record_id("tasks", &slug(), None, Some("Index")).unwrap(), "index-2");
        assert_eq!(
            vault.create_record_derived("tasks", &slug(), None, Some("Index"), &doc("Index")).unwrap(),
            "index-2"
        );
        assert_eq!(vault.create_record_derived("tasks", &slug(), None, Some("LOG"), &doc("LOG")).unwrap(), "log-2");
        assert_eq!(
            vault.create_record_derived("tasks", &slug(), None, Some("index"), &doc("index")).unwrap(),
            "index-3"
        );
        assert_eq!(vault.ensure_unique_id("tasks", "log").unwrap(), "log-3");
        assert_eq!(vault.list_ids("tasks").unwrap(), vec!["index-2", "index-3", "log-2"]);
    }

    #[test]
    fn explicit_reserved_ids_are_invalid() {
        let (_dir, vault) = vault();
        for id in ["index", "log", "Index"] {
            let err = vault.create_record_derived("tasks", &slug(), Some(id), Some("ignored"), &doc("x")).unwrap_err();
            assert!(matches!(err, Error::InvalidId { .. }), "{id}: {err}");
            assert!(matches!(vault.make_record_id("tasks", &slug(), Some(id), None), Err(Error::InvalidId { .. })));
            assert!(matches!(vault.create_record("tasks", id, &doc("x")), Err(Error::InvalidId { .. })));
        }
    }

    #[test]
    fn index_and_log_files_never_surface_as_records() {
        let (_dir, vault) = vault();
        vault.create_record("tasks", "real", &doc("real")).unwrap();
        let tasks = vault.entity_dir("tasks").unwrap();
        fsops::write_atomic(&tasks.join("index.md"), "# Tasks\n\n* [Real](real.md)\n").unwrap();
        fsops::write_atomic(&tasks.join("log.md"), "# Log\n\n## 2026-10-03\n* **Creation**: real\n").unwrap();
        assert_eq!(vault.list_ids("tasks").unwrap(), vec!["real"]);
        assert_eq!(vault.read_all("tasks").unwrap().len(), 1);
    }

    #[test]
    fn provided_ids_are_not_renamed() {
        let (_dir, vault) = vault();
        vault.create_record("tasks", "fixed", &doc("x")).unwrap();
        // make_record_id with an explicit id must NOT silently dedupe…
        let id = vault.make_record_id("tasks", &slug(), Some("fixed"), None).unwrap();
        assert_eq!(id, "fixed");
        // …the collision surfaces at create time instead.
        assert!(matches!(vault.create_record("tasks", &id, &doc("y")), Err(Error::AlreadyExists { .. })));
    }

    #[test]
    fn hostile_ids_cannot_escape_the_vault() {
        let (_dir, vault) = vault();
        for bad in ["../../etc/passwd", "..", "a/b", ".hidden"] {
            assert!(vault.create_record("tasks", bad, &doc("x")).is_err(), "id {bad:?} must be rejected");
        }
    }

    // ── EntityRecords: OKF type stamping and flat-layout filtering ──────

    fn raw(vault: &VaultHandle, dir: &str, id: &str) -> String {
        fsops::read(&vault.record_path(dir, id).unwrap()).unwrap()
    }

    fn seed(vault: &VaultHandle, dir: &str, id: &str, src: &str) {
        fsops::write_atomic(&vault.record_path(dir, id).unwrap(), src).unwrap();
    }

    #[test]
    fn typed_create_puts_type_first() {
        let (_dir, vault) = vault();
        let id = vault.entity("tasks", "Task").create(&slug(), None, Some("Ship it"), doc("Ship it")).unwrap();
        assert_eq!(id, "ship-it");
        assert_eq!(raw(&vault, "tasks", &id), "---\ntype: Task\ntitle: Ship it\n---\nbody\n");
    }

    #[test]
    fn typed_create_of_a_parsed_document_still_carries_the_type() {
        let (_dir, vault) = vault();
        let parsed = Document::parse("---\ntitle: copied\n---\n").unwrap();
        vault.entity("tasks", "Task").create(&IdStrategy::Provided, Some("copy"), None, parsed).unwrap();
        assert_eq!(raw(&vault, "tasks", "copy"), "---\ntype: Task\ntitle: copied\n---\n");
    }

    #[test]
    fn per_entity_dir_tolerates_a_foreign_type_and_normalizes_it_on_a_real_write() {
        let (_dir, vault) = vault();
        let tasks = vault.entity("tasks", "Task");
        let src = "---\ntype: task\ntitle: legacy\n---\n";
        seed(&vault, "tasks", "t", src);

        assert!(tasks.read_opt("t").unwrap().is_some(), "the directory decides membership");
        assert_eq!(tasks.read_all().unwrap().len(), 1);
        assert_eq!(tasks.count().unwrap(), 1);

        tasks.modify("t", |_| Ok(())).unwrap();
        assert_eq!(raw(&vault, "tasks", "t"), src, "a no-op cycle neither writes nor normalizes");

        tasks
            .modify("t", |d| {
                d.set("title", "edited");
                Ok(())
            })
            .unwrap();
        assert_eq!(raw(&vault, "tasks", "t"), "---\ntype: Task\ntitle: edited\n---\n");
    }

    #[test]
    fn untyped_records_read_fine_and_are_typed_on_their_next_real_write() {
        let (_dir, vault) = vault();
        let tasks = vault.entity("tasks", "Task");
        let src = "---\ntitle: legacy\nstatus: open\n---\nbody\n";
        seed(&vault, "tasks", "t", src);

        assert_eq!(tasks.read_opt("t").unwrap().unwrap().get("title").and_then(|v| v.as_str()), Some("legacy"));
        tasks
            .modify("t", |d| {
                d.set("status", "open");
                Ok(())
            })
            .unwrap();
        assert_eq!(raw(&vault, "tasks", "t"), src, "an unchanged record is not rewritten just to type it");

        tasks
            .modify("t", |d| {
                d.set("status", "closed");
                Ok(())
            })
            .unwrap();
        assert_eq!(raw(&vault, "tasks", "t"), "---\ntype: Task\ntitle: legacy\nstatus: closed\n---\nbody\n");
    }

    #[test]
    fn per_entity_dir_count_walks_without_parsing() {
        let (_dir, vault) = vault();
        let tasks = vault.entity("tasks", "Task");
        tasks.create(&IdStrategy::Provided, Some("ok"), None, doc("fine")).unwrap();
        seed(&vault, "tasks", "broken", "---\n: : : not yaml\n---\n");
        assert_eq!(tasks.count().unwrap(), 2, "an unparseable file still counts: nothing was read");
        assert!(matches!(tasks.read_all(), Err(Error::Parse { .. })));
    }

    fn flat_vault() -> (tempfile::TempDir, VaultHandle) {
        let dir = tempfile::tempdir().unwrap();
        let handle = VaultHandle::new(dir.path(), VaultLayout::Flat);
        (dir, handle)
    }

    #[test]
    fn flat_vault_lists_counts_and_gets_by_type() {
        let (_dir, vault) = flat_vault();
        let (tasks, notes) = (vault.entity("tasks", "Task"), vault.entity("notes", "Note"));
        tasks.create(&slug(), None, Some("Alpha"), doc("Alpha")).unwrap();
        tasks.create(&slug(), None, Some("Beta"), doc("Beta")).unwrap();
        notes.create(&slug(), None, Some("Gamma"), doc("Gamma")).unwrap();
        seed(&vault, "", "untyped", "---\ntitle: legacy\n---\n");

        let ids = |records: &EntityRecords<'_>| -> Vec<String> {
            records.read_all().unwrap().into_iter().map(|(id, _)| id).collect()
        };
        assert_eq!(ids(&tasks), vec!["alpha", "beta", "untyped"]);
        assert_eq!(ids(&notes), vec!["gamma", "untyped"]);
        assert_eq!((tasks.count().unwrap(), notes.count().unwrap()), (3, 2));

        assert!(tasks.read_opt("alpha").unwrap().is_some());
        assert!(notes.read_opt("alpha").unwrap().is_none(), "another entity's record is not found");
        assert!(notes.read_opt("untyped").unwrap().is_some(), "an untyped record belongs to anyone");
        assert!(tasks.read_opt("missing").unwrap().is_none());
    }

    #[test]
    fn flat_vault_never_rewrites_or_deletes_another_entitys_record() {
        let (_dir, vault) = flat_vault();
        let (tasks, notes) = (vault.entity("tasks", "Task"), vault.entity("notes", "Note"));
        let id = notes.create(&slug(), None, Some("Gamma"), doc("Gamma")).unwrap();
        let before = raw(&vault, "", &id);

        let err = tasks
            .modify(&id, |d| {
                d.set("title", "hijacked");
                Ok(())
            })
            .unwrap_err();
        assert!(matches!(err, Error::NotFound { .. }), "{err}");
        assert!(matches!(tasks.remove(&id), Err(Error::NotFound { .. })));
        assert_eq!(raw(&vault, "", &id), before, "the note is untouched");

        notes.remove(&id).unwrap();
        assert!(matches!(notes.remove(&id), Err(Error::NotFound { .. })));
    }

    #[test]
    fn flat_vault_refuses_to_remove_a_record_it_cannot_parse() {
        let (_dir, vault) = flat_vault();
        let broken = "---\n: : : not yaml\n---\n";
        seed(&vault, "", "broken", broken);

        let err = vault.entity("tasks", "Task").remove("broken").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }), "{err}");
        assert_eq!(raw(&vault, "", "broken"), broken, "a file of unknown type is left alone");

        let per_dir = VaultHandle::new(vault.root(), VaultLayout::PerEntityDir);
        seed(&per_dir, "tasks", "broken", broken);
        per_dir.entity("tasks", "Task").remove("broken").unwrap();
        assert!(!per_dir.record_exists("tasks", "broken").unwrap(), "the directory says whose it is");
    }

    #[test]
    fn flat_vault_ids_dedupe_across_entities() {
        let (_dir, vault) = flat_vault();
        let a = vault.entity("tasks", "Task").create(&slug(), None, Some("Same"), doc("Same")).unwrap();
        let b = vault.entity("notes", "Note").create(&slug(), None, Some("Same"), doc("Same")).unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("same", "same-2"), "flat entities share one id space");
    }

    #[test]
    fn clones_share_the_write_lock() {
        let (_dir, vault) = vault();
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
