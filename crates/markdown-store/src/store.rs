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
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::SystemTime,
};

use crate::{
    error::Error,
    frontmatter::Document,
    fsops,
    id::IdStrategy,
    layout::VaultLayout,
    names::NameCheck,
    okf,
    walk::{self, WalkOptions},
};

/// Default cap on records per `list` operation. ADR 0001 pins the markdown
/// backend's comfort zone at "N in the low thousands per entity"; the cap
/// makes exceeding it a loud error instead of a slow surprise.
pub const DEFAULT_LIST_CAP: usize = 10_000;

/// The OKF 0.2 artifacts a [`VaultHandle`] writes beside its records, set
/// with [`VaultHandle::with_okf`]. The default writes neither; the vault is
/// an OKF 0.2 bundle either way.
///
/// ```
/// use std::{sync::Arc, time::{Duration, SystemTime}};
/// use markdown_store::{Document, IdStrategy, OkfPolicy, VaultHandle, VaultLayout};
///
/// let dir = tempfile::tempdir().unwrap();
/// let fixed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_036_309);
/// let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir).with_okf(OkfPolicy {
///     index: true,
///     generated_by: Some("my-app/1.0.0".into()),
///     clock: Arc::new(move || fixed),
/// });
/// vault.entity("notes", "Note").create(&IdStrategy::Provided, Some("n-1"), None, Document::new())?;
/// let read = |p: &str| std::fs::read_to_string(dir.path().join(p)).unwrap();
/// assert_eq!(read("notes/n-1.md"), "---\ntype: Note\ngenerated:\n  by: my-app/1.0.0\n  at: 2026-10-03T14:05:09Z\n---\n");
/// assert_eq!(read("notes/index.md"), "# Note\n\n* [n-1](n-1.md)\n");
/// # Ok::<(), markdown_store::Error>(())
/// ```
#[derive(Clone)]
pub struct OkfPolicy {
    /// Keep an `index.md` (OKF §8) in the vault root and in every directory
    /// that holds records at any depth.
    ///
    /// Each lists that directory's records under one `# <type>` heading per
    /// OKF `type` (sorted; records without a string `type` last, under
    /// `# Untyped`) as `* [<title>](<file>) - <description>`, then its
    /// subdirectories that hold records under `# Directories`. A type that
    /// would read as one of those two headings is headed `<type> (type)`.
    /// Titles, descriptions and types are written as they are, except that
    /// `` \ ` * _ [ ] < & # ~ $ % = ^ `` are backslash-escaped, so a
    /// record's text never turns into markup (CommonMark or Obsidian's)
    /// while an agent reading the file raw sees it nearly verbatim. The
    /// root index carries `okf_version: "0.2"` as its only frontmatter.
    ///
    /// After every real write (a create, an update that changed something,
    /// a delete) the record's directory and each ancestor up to the root are
    /// regenerated under the write lock, each written atomically and only
    /// when its bytes would change. The store owns the `index.md` of every
    /// directory that holds records and overwrites whatever is there, a
    /// hand-written one included. When a directory loses its last record its
    /// index is removed if it has the store's shape and every link in it
    /// dangles; any other `index.md` is left alone (the exact rule is under
    /// [`VaultHandle::rebuild_indexes`]). Turning the option on does not
    /// touch the vault by itself: `rebuild_indexes` writes the indexes of
    /// records that already exist.
    ///
    /// Regenerating one index parses the records directly in its directory
    /// and walks each subdirectory only until its first record. A write
    /// regenerates its directory and every ancestor, so it parses the
    /// records that sit directly in each directory on that path; under
    /// [`VaultLayout::Flat`] that is every record in the vault. That fits
    /// the backend's small-N stance.
    pub index: bool,
    /// Stamp `generated: { by: <actor>, at: <now> }` (OKF §5.2) on every
    /// real write: every create, and every update that changes the record.
    /// The stamp replaces an existing `generated` value in place, or is
    /// appended after the other keys; an update that changes nothing leaves
    /// the file and its stamp alone.
    ///
    /// The actor is written verbatim. It should be an OKF §7 actor naming a
    /// program, `<producer>/<version>` or `process:<id>`, never
    /// `human:<id>`, which trust tiers reserve for people; ontogen's
    /// generator validates the value it emits into `open_vault`.
    pub generated_by: Option<String>,
    /// The time source for `generated.at`, [`SystemTime::now`] by default.
    /// Stamps are UTC with second precision. A test fixes it to make stamps
    /// deterministic.
    pub clock: Arc<dyn Fn() -> SystemTime + Send + Sync>,
}

impl Default for OkfPolicy {
    fn default() -> Self {
        Self { index: false, generated_by: None, clock: Arc::new(SystemTime::now) }
    }
}

impl std::fmt::Debug for OkfPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OkfPolicy")
            .field("index", &self.index)
            .field("generated_by", &self.generated_by)
            .finish_non_exhaustive()
    }
}

/// Handle to one markdown vault: root path, layout, walk options, list cap,
/// the [`OkfPolicy`], and the shared write lock.
///
/// The id strategy is not part of the handle: each create names the
/// [`IdStrategy`] it derives ids by, so a code generator's build-time choice
/// is the only one there is.
///
/// # Index files and failures
///
/// With [`OkfPolicy::index`] on, the record and its indexes are separate
/// atomic renames, not one transaction. Once the record is written the
/// write has succeeded: a failure to refresh an index afterwards does not
/// fail it, because reporting an error for a committed record invites a
/// retry that would create the record twice. The index is left stale but
/// still valid and listed by [`stale_indexes`](Self::stale_indexes) until
/// the next real write in that directory, or
/// [`rebuild_indexes`](Self::rebuild_indexes), brings it current. A crash
/// between the two renames leaves the same stale-but-valid index, which is
/// not listed (the list lives in memory).
///
/// # Lookups match the stored name exactly
///
/// A record's id is its file stem, byte for byte. Every lookup (read,
/// [`record_exists`](Self::record_exists), modify, remove, and the
/// [`EntityRecords`] wrappers of each) finds a record only when its file is
/// stored under exactly `<id>.md`, on every filesystem. macOS and Windows
/// filesystems resolve `KEPT.md` to a stored `kept.md` (and macOS resolves
/// one Unicode normalization form of a name to a file stored under the
/// other); there a lookup of `KEPT` is [`Error::NotFound`] and nothing is
/// read, rewritten or removed.
///
/// A create counts an id as taken when the filesystem resolves its file
/// name to any existing file, since writing it would replace that file: on
/// macOS, creating `a` beside a hand-named `A.md` is
/// [`Error::AlreadyExists`], and a derived id probes on to `a-2`.
///
/// The handle learns whether the filesystem aliases names on the first
/// lookup, by creating and removing a hidden probe file in the record's
/// directory, and keeps the answer (clones share it); a vault it cannot
/// write to is treated as aliasing. Where names do not alias, a lookup
/// costs what the open costs. Where they do, a lookup that opens a file
/// also lists that file's directory until it finds the exact name.
///
/// # Device names on Windows
///
/// On Windows, a lookup whose id is a device name, whole or before its
/// first `.` ([`crate::layout::is_device_name`]: `con`, `nul.x`, `COM1`), is
/// [`Error::NotFound`] without opening anything: Windows opens the device
/// for `con.md`, and no record can be stored under such a name there.
/// Elsewhere such a record (written by hand, or before the create rule
/// refused the name) stays reachable.
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
    okf: OkfPolicy,
    write_guard: Arc<Mutex<()>>,
    /// Whether a lookup of a device name is NotFound unopened (see
    /// [device names](Self#device-names-on-windows)): `cfg!(windows)`,
    /// settable in tests so every platform exercises it.
    devices_unopenable: bool,
    /// Whether lookups must confirm the stored name; probed once, shared by
    /// clones.
    names: Arc<NameCheck>,
    /// Directories whose index refresh failed after a committed write and
    /// has not succeeded since. Shared by clones, like the write lock.
    stale: Arc<Mutex<BTreeSet<PathBuf>>>,
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
            okf: OkfPolicy::default(),
            write_guard: Arc::new(Mutex::new(())),
            devices_unopenable: cfg!(windows),
            names: Arc::default(),
            stale: Arc::default(),
        }
    }

    /// Override the per-list record cap.
    pub fn with_list_cap(mut self, cap: usize) -> Self {
        self.list_cap = cap;
        self
    }

    /// Override the walk options used for listing. Index files see the
    /// vault through the same options.
    pub fn with_walk_options(mut self, walk: WalkOptions) -> Self {
        self.walk = walk;
        self
    }

    /// Write the OKF artifacts `policy` asks for on every real write: index
    /// files, `generated` stamps, or both (see [`OkfPolicy`]). With
    /// [`OkfPolicy::index`] on, the store owns the `index.md` of every
    /// directory holding records and overwrites a hand-written one there.
    pub fn with_okf(mut self, policy: OkfPolicy) -> Self {
        self.okf = policy;
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

    /// The OKF artifacts writes produce.
    pub fn okf(&self) -> &OkfPolicy {
        &self.okf
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

    /// The path a lookup of `id` reads, rewrites or removes, or
    /// [`Error::NotFound`] when the id names a device on this platform
    /// (see [device names](Self#device-names-on-windows)).
    fn lookup_path(&self, dir_segment: &str, id: &str) -> Result<PathBuf, Error> {
        let path = self.record_path(dir_segment, id)?;
        if lookup_is_device(id, self.devices_unopenable) {
            return Err(Error::NotFound { path });
        }
        Ok(path)
    }

    /// The records of one entity: its directory segment plus the OKF `type`
    /// its records carry. Generated store code goes through this view so
    /// every write is typed and, under [`VaultLayout::Flat`], every read is
    /// filtered to the entity's own records.
    pub fn entity<'a>(&'a self, dir_segment: &'a str, type_name: &'a str) -> EntityRecords<'a> {
        EntityRecords { vault: self, dir_segment, type_name }
    }

    // ── single-record ops ───────────────────────────────────────────────

    /// Whether a record is stored under exactly `id` (see
    /// [lookups](Self#lookups-match-the-stored-name-exactly)).
    pub fn record_exists(&self, dir_segment: &str, id: &str) -> Result<bool, Error> {
        let path = match self.lookup_path(dir_segment, id) {
            Ok(path) => path,
            Err(Error::NotFound { .. }) => return Ok(false),
            Err(e) => return Err(e),
        };
        Ok(fsops::exists(&path) && self.names.exact(&path)?)
    }

    /// Read and parse one record. A record not stored under exactly `id` is
    /// [`Error::NotFound`] (see
    /// [lookups](Self#lookups-match-the-stored-name-exactly)).
    pub fn read_record(&self, dir_segment: &str, id: &str) -> Result<Document, Error> {
        let path = self.lookup_path(dir_segment, id)?;
        let raw = fsops::read(&path)?;
        self.require_exact(&path)?;
        Document::parse(&raw).map_err(|e| Error::parse_at(&path, e))
    }

    /// Read and parse one record; a missing record, or one not stored under
    /// exactly `id`, is `Ok(None)`.
    pub fn read_record_opt(&self, dir_segment: &str, id: &str) -> Result<Option<Document>, Error> {
        match self.read_record(dir_segment, id) {
            Ok(doc) => Ok(Some(doc)),
            Err(Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Create a record. `id` must pass the create rule
    /// ([`crate::layout::validate_id`]), or it is [`Error::InvalidId`].
    /// Fails with [`Error::AlreadyExists`] if the file is already present —
    /// creation never overwrites, so a file the filesystem resolves the name
    /// to under another spelling counts as present (see
    /// [lookups](Self#lookups-match-the-stored-name-exactly)). The existence
    /// check and write happen under
    /// the vault's write lock. (The check is racy against writers in *other
    /// processes*; single-process ownership of a vault is the documented
    /// stance.)
    pub fn create_record(&self, dir_segment: &str, id: &str, doc: &Document) -> Result<(), Error> {
        crate::layout::validate_id(id)?;
        let path = self.record_path(dir_segment, id)?;
        let _guard = self.lock();
        if self.taken(&path) {
            return Err(Error::AlreadyExists { path });
        }
        self.write_new(&path, doc)
    }

    /// Create a record whose id is derived by `strategy`, atomically: id
    /// derivation, slug de-duplication, and the write all happen under one
    /// hold of the write lock, so two concurrent creates of the same slug
    /// yield `base` and `base-2` instead of racing into
    /// [`Error::AlreadyExists`]. **This is the create path generated store
    /// code uses.** Returns the id the record was created under.
    ///
    /// `provided` follows [`IdStrategy::make_id`] semantics: a non-empty
    /// caller-supplied id always wins and is *not* de-duplicated — an
    /// explicit duplicate fails with [`Error::AlreadyExists`], because
    /// silently renaming an explicit id would be worse than failing. For
    /// the same reason an explicit id that breaks the create rule
    /// ([`crate::layout::validate_id`]), a reserved one (`index`, `log`)
    /// included, is [`Error::InvalidId`], while a *derived* reserved id is
    /// treated as taken and becomes `index-2`. `source_value` feeds
    /// [`IdStrategy::SlugFromField`].
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
        if self.taken(&path) {
            return Err(Error::AlreadyExists { path });
        }
        self.write_new(&path, doc)?;
        Ok(id)
    }

    /// Read-modify-write one record under the write lock. The mutation
    /// closure receives the parsed [`Document`]; on `Ok` the document is
    /// re-rendered and atomically written back. A record not stored under
    /// exactly `id` is [`Error::NotFound`] and nothing is written.
    pub fn modify_record<F>(&self, dir_segment: &str, id: &str, f: F) -> Result<(), Error>
    where
        F: FnOnce(&mut Document) -> Result<(), Error>,
    {
        let path = self.lookup_path(dir_segment, id)?;
        let _guard = self.lock();
        self.require_exact(&path)?;
        self.rewrite(&path, f)
    }

    /// Remove a record under the write lock. A missing record, or one not
    /// stored under exactly `id`, is [`Error::NotFound`] and nothing is
    /// removed.
    pub fn remove_record(&self, dir_segment: &str, id: &str) -> Result<(), Error> {
        let path = self.lookup_path(dir_segment, id)?;
        let _guard = self.lock();
        self.require_exact(&path)?;
        self.delete(&path)
    }

    /// [`Error::NotFound`] when `path` would reach a file stored under
    /// another spelling. Every lookup runs this before it uses what it
    /// opened, and a write runs it under the write lock before touching the
    /// file: an atomic rename onto a variant would replace the other
    /// record's content and keep its old name.
    fn require_exact(&self, path: &Path) -> Result<(), Error> {
        if self.names.exact(path)? {
            Ok(())
        } else {
            Err(Error::NotFound { path: path.to_path_buf() })
        }
    }

    /// Whether a create at `path` would land on an existing file under any
    /// spelling the filesystem resolves to it.
    fn taken(&self, path: &Path) -> bool {
        fsops::exists(path)
    }

    // ── the write paths every mutation funnels through ─────────────────
    //
    // Callers hold the write lock. Keeping provenance stamping and index
    // upkeep here, and nowhere else, is what keeps the typed and untyped
    // paths from drifting apart.

    /// Write a new record: always a real write, so always stamped.
    fn write_new(&self, path: &Path, doc: &Document) -> Result<(), Error> {
        let rendered = match &self.okf.generated_by {
            Some(by) => {
                let mut doc = doc.clone();
                doc.set(okf::GENERATED_KEY, okf::generated_stamp(by, (self.okf.clock)()));
                doc.render()?
            }
            None => doc.render()?,
        };
        fsops::write_atomic(path, &rendered)?;
        self.refresh_indexes(path);
        Ok(())
    }

    /// Read-modify-write a record. `f` runs any typed stamping itself;
    /// only a document still dirty afterwards is stamped and written, so a
    /// no-op leaves the record and every index untouched.
    fn rewrite<F>(&self, path: &Path, f: F) -> Result<(), Error>
    where
        F: FnOnce(&mut Document) -> Result<(), Error>,
    {
        let mut written = false;
        fsops::read_modify_write(path, |doc| {
            f(doc)?;
            if doc.is_dirty() {
                if let Some(by) = &self.okf.generated_by {
                    doc.set(okf::GENERATED_KEY, okf::generated_stamp(by, (self.okf.clock)()));
                }
                written = true;
            }
            Ok(())
        })?;
        if written {
            self.refresh_indexes(path);
        }
        Ok(())
    }

    fn delete(&self, path: &Path) -> Result<(), Error> {
        fsops::remove(path)?;
        self.refresh_indexes(path);
        Ok(())
    }

    /// Regenerate the index of the record's directory and of each ancestor
    /// up to the root, when indexes are on. Runs after the record is
    /// committed, so a failure only marks that directory's index stale (see
    /// [`stale_indexes`](Self::stale_indexes)) and the remaining ancestors
    /// are still tried.
    fn refresh_indexes(&self, record: &Path) {
        if !self.okf.index {
            return;
        }
        let mut dir = record.parent();
        while let Some(current) = dir.filter(|d| d.starts_with(&self.root)) {
            let is_root = current == self.root;
            let _ = self.sync_index(current);
            if is_root {
                break;
            }
            dir = current.parent();
        }
    }

    /// Bring one directory's index current, keeping the stale list in step
    /// with the outcome.
    fn sync_index(&self, dir: &Path) -> Result<(), Error> {
        let result = okf::sync_index(dir, dir == self.root, &self.walk);
        let mut stale = self.stale.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if result.is_ok() {
            stale.remove(dir);
        } else {
            stale.insert(dir.to_path_buf());
        }
        result
    }

    // ── OKF indexes ─────────────────────────────────────────────────────

    /// Regenerate every OKF `index.md` in the vault from the records on
    /// disk, and remove each stale one the store wrote for a directory that
    /// no longer holds a record. Indexes whose bytes would not change are
    /// not rewritten.
    ///
    /// A directory that holds records at any depth gets the store's index
    /// whatever its `index.md` held before, so a hand-written list of the
    /// records beside it is replaced. In a directory without records an
    /// `index.md` is removed only when both hold:
    ///
    /// - It has exactly the shape the store writes: optionally the root
    ///   frontmatter (`okf_version: "0.2"` and nothing else), then one or
    ///   more sections, each a `# <heading>` line, a blank line and one or
    ///   more `* [<text>](<link>)` entries, each optionally followed by
    ///   ` - <description>`, with one blank line between sections and a
    ///   single trailing newline; every link a percent-encoded record file
    ///   name (an extension from the walk options) or a percent-encoded
    ///   directory name ending in `/`.
    /// - Every link in it dangles: a file link names a file that does not
    ///   exist, and a directory link names a directory that does not exist
    ///   or in which the walk finds no file but `index.md` files.
    ///
    /// A store-shaped file whose links all dangle is treated as the store's
    /// and removed. Anything else stays: an Obsidian folder note, a
    /// hand-kept listing of attachments, or a list in the store's shape
    /// that links a folder of images or another file that is still there.
    /// The same rule applies when a write empties a directory.
    ///
    /// This is the repair path after a crash or an edit made outside the
    /// handle, and how a seed vault's indexes are produced. It runs
    /// whether or not [`OkfPolicy::index`] is on: it is
    /// an explicit request, though without the option later writes will
    /// not keep the indexes current. Reads every record in the vault.
    /// Unlike a record write it fails on the first index it cannot write;
    /// on success [`stale_indexes`](Self::stale_indexes) is empty.
    ///
    /// ```
    /// use markdown_store::{Document, IdStrategy, VaultHandle, VaultLayout};
    ///
    /// let dir = tempfile::tempdir().unwrap();
    /// let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir);
    /// let mut doc = Document::new();
    /// doc.set("title", "First note");
    /// doc.set("description", "Where it starts.");
    /// vault.entity("notes", "Note").create(&IdStrategy::Provided, Some("first"), None, doc)?;
    ///
    /// vault.rebuild_indexes()?;
    /// let read = |p: &str| std::fs::read_to_string(dir.path().join(p)).unwrap();
    /// assert_eq!(read("index.md"), "---\nokf_version: \"0.2\"\n---\n\n# Directories\n\n* [notes](notes/)\n");
    /// assert_eq!(read("notes/index.md"), "# Note\n\n* [First note](first.md) - Where it starts.\n");
    /// # Ok::<(), markdown_store::Error>(())
    /// ```
    pub fn rebuild_indexes(&self) -> Result<(), Error> {
        let _guard = self.lock();
        let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
        for record in walk::list_record_paths(&self.root, &self.walk)? {
            let mut dir = record.parent();
            while let Some(current) = dir.filter(|d| d.starts_with(&self.root)) {
                if !dirs.insert(current.to_path_buf()) || current == self.root {
                    break;
                }
                dir = current.parent();
            }
        }
        for dir in &dirs {
            self.sync_index(dir)?;
        }
        for index in walk::list_index_paths(&self.root, &self.walk)? {
            if index.parent().is_some_and(|dir| !dirs.contains(dir)) {
                okf::remove_if_dangling(&index, &self.walk)?;
            }
        }
        self.stale.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
        Ok(())
    }

    /// The `index.md` files whose refresh failed (an I/O error, say) after
    /// a write had committed its record, sorted. Each stays listed until a
    /// later real write in its directory, or
    /// [`rebuild_indexes`](Self::rebuild_indexes), refreshes it. The list
    /// lives in memory and is shared by clones; it knows nothing of edits
    /// made outside the handle.
    pub fn stale_indexes(&self) -> Vec<PathBuf> {
        let stale = self.stale.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        stale.iter().map(|dir| dir.join(okf::INDEX_FILE)).collect()
    }

    // ── listing ─────────────────────────────────────────────────────────

    /// List record file paths for an entity, sorted lexicographically.
    /// A file is a record only when its stem is an id a lookup accepts
    /// (see [`walk::list_record_paths`]); other files are skipped.
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
    /// free `base-2`, `base-3`, … . An id whose file name the filesystem
    /// resolves to an existing file under another spelling is not free. Same non-atomicity caveat as
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
        let id = if had_provided { base } else { self.next_free_id(dir_segment, &base)? };
        crate::layout::validate_id(&id)?;
        Ok(id)
    }

    /// Suffix search shared by the public previews and the locked create
    /// path. Named for how
    /// [`create_record_derived`](Self::create_record_derived) uses it — the
    /// *caller* is responsible for holding the lock when atomicity matters;
    /// the probe itself is just existence checks. The probes are
    /// [`crate::id::candidates`]: a reserved base counts as taken, so a
    /// title that slugs to `index` lands on `index-2`.
    fn next_free_id(&self, dir_segment: &str, base: &str) -> Result<String, Error> {
        for candidate in crate::id::candidates(base) {
            if !self.taken(&self.record_path(dir_segment, &candidate)?) {
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

/// Whether a lookup of `id` must not open its file: `windows` is set and
/// the id, or its part before the first `.`, is a Windows device name,
/// superscript `COM¹`-`LPT³` forms included.
fn lookup_is_device(id: &str, windows: bool) -> bool {
    windows && crate::layout::is_windows_device_name(id)
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

    /// Read one record; missing, not stored under exactly `id` (see
    /// [`VaultHandle`]'s lookups), or another entity's under
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
    /// written at all. A record not stored under exactly `id` is
    /// [`Error::NotFound`].
    pub fn modify<F>(&self, id: &str, f: F) -> Result<(), Error>
    where
        F: FnOnce(&mut Document) -> Result<(), Error>,
    {
        let path = self.vault.lookup_path(self.dir_segment, id)?;
        let _guard = self.vault.lock();
        self.vault.require_exact(&path)?;
        self.vault.rewrite(&path, |doc| {
            if !self.admits(doc) {
                return Err(Error::NotFound { path: path.clone() });
            }
            f(doc)?;
            doc.stamp_type(self.type_name);
            Ok(())
        })
    }

    /// Remove one record under the write lock. Missing, not stored under
    /// exactly `id`, or another entity's under [`VaultLayout::Flat`], is
    /// [`Error::NotFound`].
    ///
    /// Under [`VaultLayout::Flat`] a record whose frontmatter does not parse
    /// cannot be removed here ([`Error::Parse`]): without its `type` there is
    /// no telling whose it is, and deleting another entity's file is worse
    /// than refusing. Fix the frontmatter or delete the file by hand.
    pub fn remove(&self, id: &str) -> Result<(), Error> {
        let path = self.vault.lookup_path(self.dir_segment, id)?;
        let _guard = self.vault.lock();
        self.vault.require_exact(&path)?;
        if self.vault.layout == VaultLayout::Flat {
            let doc = Document::parse(&fsops::read(&path)?).map_err(|e| Error::parse_at(&path, e))?;
            if !self.admits(&doc) {
                return Err(Error::NotFound { path });
            }
        }
        self.vault.delete(&path)
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
        for id in ["z", "a-b", "a"] {
            vault.create_record("tasks", id, &doc(id)).unwrap();
        }
        // Stems no create may use (written by hand, or under an older rule)
        // still list, in the same order.
        for id in ["é", "B"] {
            seed(&vault, "tasks", id, &format!("---\ntitle: {id}\n---\n"));
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
    fn creates_take_only_ids_the_create_rule_accepts() {
        let (_dir, vault) = vault();
        let tasks = vault.entity("tasks", "Task");
        let too_long = "a".repeat(crate::layout::MAX_ID_LEN + 1);
        for id in ["Draft", "café", "a b", "a+b", too_long.as_str()] {
            let invalid = |r: Result<_, Error>| matches!(r, Err(Error::InvalidId { .. }));
            assert!(invalid(vault.create_record("tasks", id, &doc("x")).map(|_| ())), "{id:?}");
            assert!(invalid(vault.create_record_derived("tasks", &slug(), Some(id), None, &doc("x")).map(|_| ())));
            assert!(invalid(tasks.create(&IdStrategy::Provided, Some(id), None, doc("x")).map(|_| ())), "{id:?}");
            assert!(invalid(vault.make_record_id("tasks", &slug(), Some(id), None).map(|_| ())), "{id:?}");
        }
        assert_eq!(vault.list_ids("tasks").unwrap(), Vec::<String>::new(), "nothing was written");
        let longest = "a".repeat(crate::layout::MAX_ID_LEN);
        assert_eq!(tasks.create(&IdStrategy::Provided, Some(&longest), None, doc("x")).unwrap(), longest);
    }

    #[test]
    fn a_long_title_derives_a_cut_slug_and_probes_within_the_limit() {
        let (_dir, vault) = vault();
        let title = "Word ".repeat(100);
        let first = vault.create_record_derived("tasks", &slug(), None, Some(&title), &doc("x")).unwrap();
        let second = vault.create_record_derived("tasks", &slug(), None, Some(&title), &doc("x")).unwrap();
        assert_eq!(first.len(), crate::id::SLUG_MAX_LEN - 1);
        assert_eq!(second, format!("{first}-2"));
        assert_eq!(vault.list_ids("tasks").unwrap(), vec![first, second]);
    }

    #[test]
    fn records_the_create_rule_refuses_stay_reachable() {
        let (_dir, vault) = vault();
        let long = "l".repeat(crate::layout::MAX_STEM_LEN);
        for id in ["Draft", "café", "a b", long.as_str()] {
            seed(&vault, "tasks", id, "---\ntitle: old\n---\n");
        }
        let tasks = vault.entity("tasks", "Task");
        for id in ["Draft", "café", "a b", long.as_str()] {
            assert_eq!(vault.read_record("tasks", id).unwrap().get("title").and_then(|v| v.as_str()), Some("old"));
            tasks
                .modify(id, |d| {
                    d.set("title", "new");
                    Ok(())
                })
                .unwrap();
            assert_eq!(tasks.read_opt(id).unwrap().unwrap().get("title").and_then(|v| v.as_str()), Some("new"));
        }
        assert_eq!(vault.list_ids("tasks").unwrap().len(), 4);
        for id in ["Draft", "café", "a b", long.as_str()] {
            tasks.remove(id).unwrap();
            assert!(matches!(vault.read_record("tasks", id), Err(Error::NotFound { .. })), "{id:?}");
        }
        assert!(matches!(
            vault.read_record("tasks", &"l".repeat(crate::layout::MAX_STEM_LEN + 1)),
            Err(Error::InvalidId { .. })
        ));
    }

    #[test]
    fn only_a_windows_lookup_of_a_device_name_skips_the_file() {
        for device in ["con", "NUL", "nul.x", "Com0", "lpt9.md", "aux.a.b", "com¹", "COM²", "lpt³.x", "Lpt¹"] {
            assert!(lookup_is_device(device, true), "{device:?}");
            assert!(!lookup_is_device(device, false), "{device:?}");
        }
        for fine in ["console", "con-2", "a.con", "com10", "Draft", "com⁴", "lpt¹x", "x.com¹"] {
            assert!(!lookup_is_device(fine, true), "{fine:?}");
        }
    }

    #[test]
    fn a_device_name_lookup_on_windows_is_not_found_and_touches_nothing() {
        let (_dir, mut vault) = vault();
        let devices = ["con", "nul.x", "COM1"];
        // Windows cannot hold these files; elsewhere they stand in for the
        // device the guard must not open.
        if !cfg!(windows) {
            for id in devices {
                seed(&vault, "tasks", id, "---\ntitle: kept\n---\n");
                assert!(vault.record_exists("tasks", id).unwrap(), "{id:?} is reachable off Windows");
            }
        }
        vault.devices_unopenable = true;
        let tasks = vault.entity("tasks", "Task");
        for id in devices {
            assert!(!vault.record_exists("tasks", id).unwrap(), "{id:?}");
            assert_not_found(vault.read_record("tasks", id), id);
            assert!(vault.read_record_opt("tasks", id).unwrap().is_none(), "{id:?}");
            assert!(tasks.read_opt(id).unwrap().is_none(), "{id:?}");
            assert_not_found(vault.modify_record("tasks", id, |_| panic!("{id:?} was opened")), id);
            assert_not_found(tasks.modify(id, |_| panic!("{id:?} was opened")), id);
            assert_not_found(vault.remove_record("tasks", id), id);
            assert_not_found(tasks.remove(id), id);
            if !cfg!(windows) {
                assert_eq!(raw(&vault, "tasks", id), "---\ntitle: kept\n---\n", "{id:?} is untouched");
            }
        }
        tasks.create(&IdStrategy::Provided, Some("console"), None, doc("x")).unwrap();
        assert!(tasks.read_opt("console").unwrap().is_some(), "a near miss is looked up as usual");
    }

    /// Whether the test machine's filesystem would open `variant` in the
    /// entity's directory when only another spelling is stored there.
    fn resolves(vault: &VaultHandle, dir: &str, variant: &str) -> bool {
        vault.record_path(dir, variant).unwrap().exists()
    }

    fn assert_not_found<T: std::fmt::Debug>(result: Result<T, Error>, what: &str) {
        assert!(matches!(result, Err(Error::NotFound { .. })), "{what}: {result:?}");
    }

    /// Every lookup of `variant` finds nothing, through the handle and the
    /// typed view, and leaves `stored`'s file byte for byte as it was.
    fn assert_variant_misses(vault: &VaultHandle, dir: &str, stored: &str, variant: &str) {
        let before = raw(vault, dir, stored);
        let tasks = vault.entity(dir, "Task");
        let edit = |d: &mut Document| {
            d.set("title", "overwritten");
            Ok(())
        };
        assert!(!vault.record_exists(dir, variant).unwrap(), "exists {variant:?}");
        assert_not_found(vault.read_record(dir, variant), &format!("read {variant:?}"));
        assert_eq!(vault.read_record_opt(dir, variant).unwrap(), None, "read_opt {variant:?}");
        assert_eq!(tasks.read_opt(variant).unwrap(), None, "typed read_opt {variant:?}");
        assert_not_found(vault.modify_record(dir, variant, edit), &format!("modify {variant:?}"));
        assert_not_found(tasks.modify(variant, edit), &format!("typed modify {variant:?}"));
        assert_not_found(vault.remove_record(dir, variant), &format!("remove {variant:?}"));
        assert_not_found(tasks.remove(variant), &format!("typed remove {variant:?}"));
        assert_eq!(raw(vault, dir, stored), before, "{stored:?} is untouched by lookups of {variant:?}");
        assert!(vault.record_exists(dir, stored).unwrap(), "{stored:?} is still there");
    }

    #[test]
    fn a_case_variant_of_an_id_finds_nothing_and_changes_nothing() {
        for layout in [VaultLayout::PerEntityDir, VaultLayout::Flat] {
            let dir = tempfile::tempdir().unwrap();
            let vault = VaultHandle::new(dir.path(), layout);
            vault.entity("tasks", "Task").create(&IdStrategy::Provided, Some("kept"), None, doc("kept")).unwrap();
            // On macOS and Windows the variants open kept.md; elsewhere they
            // name no file. Either way they are not the record.
            for variant in ["KEPT", "Kept"] {
                assert_variant_misses(&vault, "tasks", "kept", variant);
            }
            assert_eq!(vault.list_ids("tasks").unwrap(), ["kept"], "{layout:?}");
            assert_eq!(vault.read_record("tasks", "kept").unwrap().get("title").and_then(|v| v.as_str()), Some("kept"));
        }
    }

    #[test]
    fn a_normalization_variant_of_an_id_finds_nothing_and_changes_nothing() {
        let (_dir, vault) = vault();
        seed(&vault, "tasks", "caf\u{e9}", "---\ntitle: hand-named\n---\n");
        // HFS+ stores names decomposed; take the name as the directory has it.
        let stored = vault.list_ids("tasks").unwrap().remove(0);
        let variant = if stored.contains('\u{e9}') {
            stored.replace('\u{e9}', "e\u{301}")
        } else {
            stored.replace("e\u{301}", "\u{e9}")
        };
        assert_ne!(stored, variant);
        assert_variant_misses(&vault, "tasks", &stored, &variant);
        assert_eq!(
            vault.read_record("tasks", &stored).unwrap().get("title").and_then(|v| v.as_str()),
            Some("hand-named"),
            "the stored spelling still reads"
        );
        assert_eq!(vault.list_ids("tasks").unwrap(), [stored]);
    }

    #[test]
    fn a_create_never_lands_on_a_file_stored_under_another_spelling() {
        let (_dir, vault) = vault();
        let tasks = vault.entity("tasks", "Task");
        let (draft, ship) = ("---\ntitle: Draft\n---\n", "---\ntitle: Ship It\n---\n");
        seed(&vault, "tasks", "Draft", draft);
        seed(&vault, "tasks", "Ship-It", ship);
        let folds = resolves(&vault, "tasks", "draft");

        let explicit = tasks.create(&IdStrategy::Provided, Some("draft"), None, doc("new draft"));
        let derived = tasks.create(&slug(), None, Some("Ship It"), doc("Ship It")).unwrap();
        if folds {
            // draft.md would open Draft.md, so `draft` is taken.
            assert!(matches!(explicit, Err(Error::AlreadyExists { .. })), "{explicit:?}");
            assert_eq!(derived, "ship-it-2", "a derived id probes past the taken spelling");
            assert_eq!(vault.ensure_unique_id("tasks", "draft").unwrap(), "draft-2");
        } else {
            assert_eq!(explicit.unwrap(), "draft", "a distinct file on a case-sensitive filesystem");
            assert_eq!(derived, "ship-it");
            assert_eq!(
                vault.read_record("tasks", "draft").unwrap().get("title").and_then(|v| v.as_str()),
                Some("new draft")
            );
        }
        assert_eq!(raw(&vault, "tasks", "Draft"), draft, "the hand-named file is untouched");
        assert_eq!(raw(&vault, "tasks", "Ship-It"), ship, "the hand-named file is untouched");
        assert!(vault.list_ids("tasks").unwrap().iter().any(|id| id == "Draft"));
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
    fn files_no_lookup_can_reach_are_not_records() {
        let mut stems = vec!["trail.", "trail ", "   "];
        if cfg!(not(windows)) {
            stems.extend(["a:b", "back\\slash"]);
        }
        for layout in [VaultLayout::PerEntityDir, VaultLayout::Flat] {
            let dir = tempfile::tempdir().unwrap();
            let okf = OkfPolicy { index: true, ..OkfPolicy::default() };
            let vault = VaultHandle::new(dir.path(), layout).with_okf(okf);
            let tasks = vault.entity("tasks", "Task");
            tasks.create(&IdStrategy::Provided, Some("ok"), None, doc("ok")).unwrap();
            let entity_dir = vault.entity_dir("tasks").unwrap();
            for stem in &stems {
                assert!(vault.record_path("tasks", stem).is_err(), "{layout:?}: {stem:?} is no lookup id");
                std::fs::write(entity_dir.join(format!("{stem}.md")), "---\ntype: Task\ntitle: stray\n---\n").unwrap();
            }

            assert_eq!(vault.list_ids("tasks").unwrap(), ["ok"], "{layout:?}");
            assert_eq!(vault.list_paths("tasks").unwrap().len(), 1, "{layout:?}");
            assert_eq!(vault.read_all("tasks").unwrap().len(), 1, "{layout:?}");
            assert_eq!(tasks.read_all().unwrap().len(), 1, "{layout:?}");
            assert_eq!(tasks.count().unwrap(), 1, "{layout:?}");
            vault.rebuild_indexes().unwrap();
            let index = std::fs::read_to_string(entity_dir.join(okf::INDEX_FILE)).unwrap();
            assert_eq!(index.matches("* [").count(), 1, "{layout:?}: only `ok` is indexed:\n{index}");
        }
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

    // ── OKF index files ─────────────────────────────────────────────────

    fn indexed(layout: VaultLayout) -> (tempfile::TempDir, VaultHandle) {
        let dir = tempfile::tempdir().unwrap();
        let handle = VaultHandle::new(dir.path(), layout).with_okf(OkfPolicy { index: true, ..OkfPolicy::default() });
        (dir, handle)
    }

    fn file(root: &Path, rel: &str) -> Option<String> {
        fsops::read_opt(&root.join(rel)).unwrap()
    }

    fn titled(title: &str, description: Option<&str>) -> Document {
        let mut d = Document::new();
        d.set("title", title);
        if let Some(description) = description {
            d.set("description", description);
        }
        d
    }

    #[cfg(unix)]
    fn stamp_of(path: &Path) -> (u64, i64, i64) {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(path).unwrap();
        (meta.ino(), meta.mtime(), meta.mtime_nsec())
    }

    #[test]
    fn indexes_group_records_by_type_and_list_subdirectories() {
        let (dir, vault) = indexed(VaultLayout::PerEntityDir);
        let root = dir.path();
        let tasks = vault.entity("tasks", "Task");
        tasks
            .create(&IdStrategy::Provided, Some("b-ship"), None, titled("Ship [v2]", Some("Cut the\n  release.")))
            .unwrap();
        tasks.create(&IdStrategy::Provided, Some("a-plan"), None, titled("Plan", None)).unwrap();
        vault.entity("tasks", "Chore").create(&IdStrategy::Provided, Some("c-sweep"), None, Document::new()).unwrap();
        vault.create_record("tasks", "d-loose", &titled("Loose", Some("no type"))).unwrap();
        vault.entity("notes", "Note").create(&IdStrategy::Provided, Some("n"), None, titled("A note", None)).unwrap();

        assert_eq!(
            file(root, "index.md").unwrap(),
            "---\nokf_version: \"0.2\"\n---\n\n# Directories\n\n* [notes](notes/)\n* [tasks](tasks/)\n"
        );
        assert_eq!(
            file(root, "tasks/index.md").unwrap(),
            "# Chore\n\n\
             * [c-sweep](c-sweep.md)\n\
             \n\
             # Task\n\n\
             * [Plan](a-plan.md)\n\
             * [Ship \\[v2\\]](b-ship.md) - Cut the release.\n\
             \n\
             # Untyped\n\n\
             * [Loose](d-loose.md) - no type\n",
            "sections sorted by type, untyped last; entries by id; titles fall back to the id; text is escaped"
        );
        assert_eq!(file(root, "notes/index.md").unwrap(), "# Note\n\n* [A note](n.md)\n");
        assert_eq!(
            vault.list_ids("tasks").unwrap(),
            ["a-plan", "b-ship", "c-sweep", "d-loose"],
            "index.md is no record"
        );
    }

    #[test]
    fn index_text_is_inert_markdown_and_types_never_take_the_stores_own_headings() {
        let (dir, vault) = indexed(VaultLayout::PerEntityDir);
        let mk = |type_name: &str, id: &str, title: &str, description: Option<&str>| {
            vault
                .entity("things", type_name)
                .create(&IdStrategy::Provided, Some(id), None, titled(title, description))
                .unwrap();
        };
        mk("Directories", "a", "A", None);
        mk("Untyped", "b", "B", None);
        mk("C# *notes*", "c", "<!-- hidden", Some("<b>bold</b> & `code` | _x_ $y$ %%z%% ==w== #tag"));
        vault.create_record("things", "d", &titled("D", None)).unwrap();
        fsops::write_atomic(&dir.path().join("things/sub/e.md"), "---\ntype: Note\n---\n").unwrap();
        vault.rebuild_indexes().unwrap();

        assert_eq!(
            file(dir.path(), "things/index.md").unwrap(),
            "# C\\# \\*notes\\*\n\n\
             * [\\<!-- hidden](c.md) - \\<b>bold\\</b> \\& \\`code\\` | \\_x\\_ \\$y\\$ \\%\\%z\\%\\% \\=\\=w\\=\\= \\#tag\n\
             \n\
             # Directories (type)\n\n\
             * [A](a.md)\n\
             \n\
             # Untyped (type)\n\n\
             * [B](b.md)\n\
             \n\
             # Untyped\n\n\
             * [D](d.md)\n\
             \n\
             # Directories\n\n\
             * [sub](sub/)\n"
        );
        let index = file(dir.path(), "things/index.md").unwrap();
        assert!(okf::store_links(&index, &WalkOptions::default()).is_some(), "escaped text keeps the store's shape");
    }

    #[test]
    fn writes_keep_indexes_current_and_empty_directories_lose_theirs() {
        let (dir, vault) = indexed(VaultLayout::PerEntityDir);
        let root = dir.path();
        let (tasks, notes) = (vault.entity("tasks", "Task"), vault.entity("notes", "Note"));
        tasks.create(&IdStrategy::Provided, Some("t"), None, titled("Before", None)).unwrap();
        notes.create(&IdStrategy::Provided, Some("n"), None, titled("Note", None)).unwrap();

        tasks
            .modify("t", |d| {
                d.set("title", "After");
                Ok(())
            })
            .unwrap();
        assert_eq!(file(root, "tasks/index.md").unwrap(), "# Task\n\n* [After](t.md)\n");

        tasks.create(&IdStrategy::Provided, Some("u"), None, titled("Second", None)).unwrap();
        tasks.remove("t").unwrap();
        assert_eq!(file(root, "tasks/index.md").unwrap(), "# Task\n\n* [Second](u.md)\n");

        tasks.remove("u").unwrap();
        assert_eq!(file(root, "tasks/index.md"), None, "a directory without records has no index");
        assert_eq!(
            file(root, "index.md").unwrap(),
            "---\nokf_version: \"0.2\"\n---\n\n# Directories\n\n* [notes](notes/)\n",
            "and the root stops linking it"
        );

        vault.remove_record("notes", "n").unwrap();
        assert_eq!(file(root, "index.md"), None, "an empty vault has no index");
    }

    #[test]
    fn the_store_overwrites_any_index_beside_records_and_removes_only_its_own() {
        let (dir, vault) = indexed(VaultLayout::PerEntityDir);
        let root = dir.path();
        let tasks = vault.entity("tasks", "Task");
        let note = "# My tasks\n\nA folder note, kept by hand.\n";
        fsops::write_atomic(&root.join("tasks/index.md"), note).unwrap();
        tasks.create(&IdStrategy::Provided, Some("t"), None, titled("T", None)).unwrap();
        assert_eq!(
            file(root, "tasks/index.md").unwrap(),
            "# Task\n\n* [T](t.md)\n",
            "beside records the store owns it"
        );

        fsops::write_atomic(&root.join("tasks/index.md"), note).unwrap();
        tasks.remove("t").unwrap();
        assert_eq!(file(root, "tasks/index.md").unwrap(), note, "without records a hand-written index stays");
        assert_eq!(file(root, "index.md"), None, "the root's generated index goes with its last record");

        tasks.create(&IdStrategy::Provided, Some("t"), None, titled("T", None)).unwrap();
        fsops::write_atomic(&root.join("tasks/diagrams/flow.png"), "png").unwrap();
        let listing = "# Files\n\n* [Diagrams](diagrams/)\n* [T](t.md)\n";
        fsops::write_atomic(&root.join("tasks/index.md"), listing).unwrap();
        tasks.remove("t").unwrap();
        assert_eq!(file(root, "tasks/index.md").unwrap(), listing, "a store-shaped list stays while a link resolves");
    }

    #[cfg(unix)]
    #[test]
    fn writes_leave_unchanged_indexes_and_noop_updates_leave_everything_alone() {
        let (dir, vault) = indexed(VaultLayout::PerEntityDir);
        let root = dir.path();
        let tasks = vault.entity("tasks", "Task");
        tasks.create(&IdStrategy::Provided, Some("t"), None, titled("Title", None)).unwrap();
        let root_index = stamp_of(&root.join("index.md"));
        let task_index = stamp_of(&root.join("tasks/index.md"));
        let record = stamp_of(&root.join("tasks/t.md"));

        tasks
            .modify("t", |d| {
                d.set("title", "Title");
                Ok(())
            })
            .unwrap();
        assert_eq!(stamp_of(&root.join("tasks/t.md")), record, "a no-op update writes no record");
        assert_eq!(stamp_of(&root.join("tasks/index.md")), task_index, "a no-op update writes no index");

        tasks
            .modify("t", |d| {
                d.set_body("A new body.\n");
                Ok(())
            })
            .unwrap();
        assert_ne!(stamp_of(&root.join("tasks/t.md")), record, "a real write rewrites the record");
        assert_eq!(
            stamp_of(&root.join("tasks/index.md")),
            task_index,
            "an index whose bytes stay the same is not rewritten"
        );
        assert_eq!(stamp_of(&root.join("index.md")), root_index);
    }

    #[cfg(unix)]
    #[test]
    fn rebuilds_are_deterministic_and_rewrite_nothing_that_is_current() {
        let (dir, vault) = indexed(VaultLayout::PerEntityDir);
        let root = dir.path();
        vault.entity("tasks", "Task").create(&IdStrategy::Provided, Some("t"), None, titled("T", Some("d"))).unwrap();
        let before = (file(root, "index.md"), file(root, "tasks/index.md"));
        let stamps = (stamp_of(&root.join("index.md")), stamp_of(&root.join("tasks/index.md")));

        vault.rebuild_indexes().unwrap();
        vault.rebuild_indexes().unwrap();
        assert_eq!((file(root, "index.md"), file(root, "tasks/index.md")), before);
        assert_eq!((stamp_of(&root.join("index.md")), stamp_of(&root.join("tasks/index.md"))), stamps);
    }

    #[test]
    fn rebuild_indexes_nested_directories_and_removes_only_dangling_store_shaped_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let vault = VaultHandle::new(root, VaultLayout::PerEntityDir);
        let put = |rel: &str, content: &str| fsops::write_atomic(&root.join(rel), content).unwrap();
        seed(&vault, "notes", "top", "---\ntype: Note\ntitle: Top\n---\n");
        put("notes/deep/er/leaf.md", "---\ntype: Note\n---\n");
        put("notes/.hidden/secret.md", "---\ntype: Note\n---\n");
        put("notes/my notes (old).md", "no frontmatter at all\n");
        // Store-shaped, every link dangling: a stale index of the store's.
        put("gone/index.md", "# Note\n\n* [Old](old.md)\n");
        put("emptied/index.md", "# Note\n\n* [Old](old.md)\n\n# Directories\n\n* [old](old/)\n");
        put("emptied/old/index.md", "# Note\n\n* [Old](old.md)\n");
        put("emptied/old/.DS_Store", "hidden: the walk does not see it");
        // Not the store's shape.
        let folder_note = "# Attachments\n\nDiagrams for the notes.\n";
        put("attachments/index.md", folder_note);
        let listing = "# Attachments\n\n* [Diagram](diagram.png)\n";
        put("figures/index.md", listing);
        // The store's shape, but a link still resolves to something that is no record.
        let folders = "# Folders\n\n* [Logos](logos/)\n";
        put("assets/index.md", folders);
        put("assets/logos/logo.png", "png");
        let reading = "# Reading list\n\n* [Changes](log.md) - read first\n* [Spec](spec.md)\n";
        put("reading/index.md", reading);
        put("reading/log.md", "# Log\n");
        // Beside a record the index is the store's, whatever it held.
        put("shelf/index.md", "# Reading list\n\n* [Spec](spec.md) - read first\n");
        put("shelf/spec.md", "---\ntype: Doc\ntitle: Spec\n---\n");
        assert_eq!(file(root, "index.md"), None, "indexes off: a seeded vault gets none on its own");

        vault.rebuild_indexes().unwrap();
        assert_eq!(
            file(root, "notes/index.md").unwrap(),
            "# Note\n\n* [Top](top.md)\n\n# Untyped\n\n* [my notes (old)](my%20notes%20%28old%29.md)\n\n\
             # Directories\n\n* [deep](deep/)\n"
        );
        assert_eq!(file(root, "notes/deep/index.md").unwrap(), "# Directories\n\n* [er](er/)\n");
        assert_eq!(file(root, "notes/deep/er/index.md").unwrap(), "# Note\n\n* [leaf](leaf.md)\n");
        assert_eq!(file(root, "notes/.hidden/index.md"), None, "walk options apply: hidden directories are skipped");
        assert_eq!(file(root, "gone/index.md"), None, "a store-shaped index whose links all dangle is removed");
        assert_eq!(file(root, "emptied/old/index.md"), None);
        assert_eq!(
            file(root, "emptied/index.md"),
            None,
            "a directory holding only an index and hidden files is no link target"
        );
        assert_eq!(file(root, "attachments/index.md").unwrap(), folder_note, "a hand-written note is not the store's");
        assert_eq!(file(root, "figures/index.md").unwrap(), listing, "nor is a listing of non-records");
        assert_eq!(file(root, "assets/index.md").unwrap(), folders, "a linked directory still holds a file");
        assert_eq!(file(root, "reading/index.md").unwrap(), reading, "one link still resolves");
        assert_eq!(file(root, "shelf/index.md").unwrap(), "# Doc\n\n* [Spec](spec.md)\n");
        assert_eq!(
            file(root, "index.md").unwrap(),
            "---\nokf_version: \"0.2\"\n---\n\n# Directories\n\n* [notes](notes/)\n* [shelf](shelf/)\n"
        );
    }

    #[test]
    fn an_unreadable_record_never_blocks_a_write_and_is_listed_by_its_id() {
        // Under Flat every record shares the root, so one bad file there
        // used to fail every write in the vault after it was committed.
        let (dir, vault) = indexed(VaultLayout::Flat);
        std::fs::write(dir.path().join("latin1.md"), b"---\ntitle: caf\xe9\n---\n").unwrap();
        let tasks = vault.entity("tasks", "Task");
        tasks.create(&IdStrategy::Provided, Some("t"), None, titled("Ship", None)).unwrap();
        tasks
            .modify("t", |d| {
                d.set("title", "Shipped");
                Ok(())
            })
            .unwrap();
        assert_eq!(
            file(dir.path(), "index.md").unwrap(),
            "---\nokf_version: \"0.2\"\n---\n\n# Task\n\n* [Shipped](t.md)\n\n# Untyped\n\n* [latin1](latin1.md)\n"
        );
        tasks.remove("t").unwrap();
        assert_eq!(vault.stale_indexes(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_failed_index_refresh_leaves_the_write_committed_and_reports_the_stale_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let vault = VaultHandle::new(root, VaultLayout::PerEntityDir)
            .with_okf(OkfPolicy { index: true, ..OkfPolicy::default() });
        let tasks = vault.entity("tasks", "Task");
        // A directory where the index file belongs makes every write of it fail.
        let blocker = root.join("tasks/index.md");
        std::fs::create_dir_all(&blocker).unwrap();

        assert_eq!(tasks.create(&slug(), None, Some("Ship"), titled("Ship", None)).unwrap(), "ship");
        assert_eq!(vault.list_ids("tasks").unwrap(), ["ship"], "one record: nothing invites a retry");
        assert_eq!(vault.stale_indexes(), vec![blocker.clone()]);
        assert!(
            file(root, "index.md").unwrap().ends_with("# Directories\n\n* [tasks](tasks/)\n"),
            "the other directories on the path are still refreshed"
        );

        std::fs::remove_dir(&blocker).unwrap();
        tasks
            .modify("ship", |d| {
                d.set("title", "Shipped");
                Ok(())
            })
            .unwrap();
        assert_eq!(vault.stale_indexes(), Vec::<PathBuf>::new(), "the next real write repairs it");
        assert_eq!(file(root, "tasks/index.md").unwrap(), "# Task\n\n* [Shipped](ship.md)\n");

        std::fs::remove_file(&blocker).unwrap();
        std::fs::create_dir_all(&blocker).unwrap();
        tasks.create(&slug(), None, Some("Plan"), titled("Plan", None)).unwrap();
        assert_eq!(vault.clone().stale_indexes(), vec![blocker.clone()], "clones share the list");
        assert!(vault.rebuild_indexes().is_err(), "a rebuild reports what it cannot write");
        std::fs::remove_dir(&blocker).unwrap();
        vault.rebuild_indexes().unwrap();
        assert_eq!(vault.stale_indexes(), Vec::<PathBuf>::new(), "a rebuild repairs it too");
        assert_eq!(file(root, "tasks/index.md").unwrap(), "# Task\n\n* [Plan](plan.md)\n* [Shipped](ship.md)\n");
    }

    #[test]
    fn a_flat_vault_indexes_its_root_by_type() {
        let (dir, vault) = indexed(VaultLayout::Flat);
        vault.entity("tasks", "Task").create(&IdStrategy::Provided, Some("t-1"), None, titled("Ship", None)).unwrap();
        vault
            .entity("notes", "Note")
            .create(&IdStrategy::Provided, Some("n-1"), None, titled("Idea", Some("why")))
            .unwrap();
        assert_eq!(
            file(dir.path(), "index.md").unwrap(),
            "---\nokf_version: \"0.2\"\n---\n\n# Note\n\n* [Idea](n-1.md) - why\n\n# Task\n\n* [Ship](t-1.md)\n"
        );
        assert_eq!(vault.entity("tasks", "Task").count().unwrap(), 1, "the index is no record");
    }

    #[test]
    fn indexes_and_stamps_are_off_by_default() {
        let (dir, vault) = vault();
        vault.entity("tasks", "Task").create(&IdStrategy::Provided, Some("t"), None, titled("T", None)).unwrap();
        assert_eq!(file(dir.path(), "index.md"), None);
        assert_eq!(file(dir.path(), "tasks/index.md"), None);
        assert_eq!(raw(&vault, "tasks", "t"), "---\ntype: Task\ntitle: T\n---\n");
    }

    // ── OKF generated stamps ────────────────────────────────────────────

    /// A vault stamped by `app/1.0` whose clock reads the returned cell.
    fn stamped() -> (tempfile::TempDir, VaultHandle, Arc<std::sync::atomic::AtomicU64>) {
        let dir = tempfile::tempdir().unwrap();
        let now = Arc::new(std::sync::atomic::AtomicU64::new(1_791_036_309));
        let clock = Arc::clone(&now);
        let handle = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir).with_okf(OkfPolicy {
            generated_by: Some("app/1.0".into()),
            clock: Arc::new(move || {
                SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(clock.load(std::sync::atomic::Ordering::SeqCst))
            }),
            ..OkfPolicy::default()
        });
        (dir, handle, now)
    }

    #[test]
    fn creates_stamp_and_real_updates_restamp_after_type() {
        let (_dir, vault, now) = stamped();
        let tasks = vault.entity("tasks", "Task");
        tasks.create(&IdStrategy::Provided, Some("t"), None, titled("T", None)).unwrap();
        assert_eq!(
            raw(&vault, "tasks", "t"),
            "---\ntype: Task\ntitle: T\ngenerated:\n  by: app/1.0\n  at: 2026-10-03T14:05:09Z\n---\n"
        );

        now.store(1_791_036_309 + 3_600, std::sync::atomic::Ordering::SeqCst);
        tasks
            .modify("t", |d| {
                d.set("title", "T2");
                d.set("extra", 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            raw(&vault, "tasks", "t"),
            "---\ntype: Task\ntitle: T2\ngenerated:\n  by: app/1.0\n  at: 2026-10-03T15:05:09Z\nextra: 1\n---\n",
            "the stamp is replaced in place with the new time"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_noop_update_keeps_the_old_stamp_and_the_file() {
        let (dir, vault, now) = stamped();
        let tasks = vault.entity("tasks", "Task");
        tasks.create(&IdStrategy::Provided, Some("t"), None, titled("T", None)).unwrap();
        let before = (raw(&vault, "tasks", "t"), stamp_of(&dir.path().join("tasks/t.md")));

        now.store(1_791_036_309 + 60, std::sync::atomic::Ordering::SeqCst);
        tasks
            .modify("t", |d| {
                d.set("title", "T");
                Ok(())
            })
            .unwrap();
        assert_eq!((raw(&vault, "tasks", "t"), stamp_of(&dir.path().join("tasks/t.md"))), before);
    }

    #[test]
    fn a_stamp_on_a_hand_authored_record_lands_after_type_and_its_keys() {
        let (_dir, vault, _now) = stamped();
        seed(&vault, "tasks", "legacy", "---\ntitle: Legacy\nstatus: open\n---\nBody.\n");
        vault
            .entity("tasks", "Task")
            .modify("legacy", |d| {
                d.set("status", "done");
                Ok(())
            })
            .unwrap();
        assert_eq!(
            raw(&vault, "tasks", "legacy"),
            "---\ntype: Task\ntitle: Legacy\nstatus: done\ngenerated:\n  by: app/1.0\n  at: 2026-10-03T14:05:09Z\n---\nBody.\n"
        );
    }

    #[test]
    fn untyped_writes_stamp_too() {
        let (_dir, vault, _now) = stamped();
        vault.create_record("notes", "a", &titled("A", None)).unwrap();
        let id =
            vault.create_record_derived("notes", &IdStrategy::Provided, Some("b"), None, &titled("B", None)).unwrap();
        vault
            .modify_record("notes", "a", |d| {
                d.set("title", "A2");
                Ok(())
            })
            .unwrap();
        for id in ["a", id.as_str()] {
            let doc = vault.read_record("notes", id).unwrap();
            let stamp = doc.get("generated").and_then(|v| v.as_mapping()).expect("stamped");
            assert_eq!(stamp.get("by").and_then(|v| v.as_str()), Some("app/1.0"), "{id}");
            assert_eq!(stamp.get("at").and_then(|v| v.as_str()), Some("2026-10-03T14:05:09Z"), "{id}");
        }
    }
}
