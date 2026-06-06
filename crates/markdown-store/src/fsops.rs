//! Atomic file primitives for single-record operations.
//!
//! ADR 0001's atomicity promise for the markdown backend is exactly this
//! module: a single record's write is all-or-nothing because the content is
//! written to a temp file *in the same directory* and renamed over the
//! target — readers see the old complete file or the new complete file,
//! never a torn write. (Same-directory matters: `rename(2)` is only atomic
//! within one filesystem.)
//!
//! Multi-record atomicity is deliberately not offered; that is the
//! documented limit of the backend, not a gap to fill here.

use std::{fs, io, path::Path};

use crate::{error::Error, frontmatter::Document};

/// Read a file to a string. A missing file is [`Error::NotFound`].
pub fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::NotFound { path: path.to_path_buf() },
        _ => Error::Io { path: path.to_path_buf(), source: e },
    })
}

/// Read a file to a string; a missing file is `Ok(None)`.
pub fn read_opt(path: &Path) -> Result<Option<String>, Error> {
    match read(path) {
        Ok(s) => Ok(Some(s)),
        Err(Error::NotFound { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether a record file exists.
pub fn exists(path: &Path) -> bool {
    path.is_file()
}

/// Atomically replace `path` with `content`.
///
/// Writes to a `tempfile::NamedTempFile` created in `path`'s parent
/// directory, fsyncs it, then persists (renames) it over `path`. Parent
/// directories are created as needed.
pub fn write_atomic(path: &Path, content: &str) -> Result<(), Error> {
    let io_err = |e: io::Error| Error::Io { path: path.to_path_buf(), source: e };
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(io_err)?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(io_err)?;
    io::Write::write_all(&mut tmp, content.as_bytes()).map_err(io_err)?;
    tmp.as_file().sync_all().map_err(io_err)?;
    tmp.persist(path).map_err(|e| Error::Io { path: path.to_path_buf(), source: e.error })?;
    Ok(())
}

/// Remove a record file. A missing file is [`Error::NotFound`] — deletes are
/// not silently idempotent, so a double-delete surfaces as the same error a
/// missing `get` would.
pub fn remove(path: &Path) -> Result<(), Error> {
    fs::remove_file(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::NotFound { path: path.to_path_buf() },
        _ => Error::Io { path: path.to_path_buf(), source: e },
    })
}

/// Read-modify-write one record atomically: read → parse [`Document`] →
/// caller mutates it → render → [`write_atomic`].
///
/// This is the single-record update primitive (`update`, `set_parent`).
/// Note it is atomic against *readers* (rename), not against a concurrent
/// writer — intra-process write serialization is the vault handle's job,
/// and cross-process writers are out of scope (single-process stance).
///
/// ```no_run
/// use std::path::Path;
/// markdown_store::fsops::read_modify_write(Path::new("vault/tasks/t-1.md"), |doc| {
///     doc.set("status", "closed");
///     Ok(())
/// })?;
/// # Ok::<(), markdown_store::Error>(())
/// ```
pub fn read_modify_write<F>(path: &Path, f: F) -> Result<(), Error>
where
    F: FnOnce(&mut Document) -> Result<(), Error>,
{
    let raw = read(path)?;
    let mut doc = Document::parse(&raw).map_err(|e| Error::parse_at(path, e))?;
    f(&mut doc)?;
    let rendered = doc.render()?;
    write_atomic(path, &rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn write_read_roundtrip_creates_parents() {
        let dir = tmp();
        let path = dir.path().join("deep/nested/file.md");
        write_atomic(&path, "content\n").unwrap();
        assert_eq!(read(&path).unwrap(), "content\n");
    }

    #[test]
    fn write_atomic_replaces_existing() {
        let dir = tmp();
        let path = dir.path().join("f.md");
        write_atomic(&path, "one\n").unwrap();
        write_atomic(&path, "two\n").unwrap();
        assert_eq!(read(&path).unwrap(), "two\n");
    }

    #[test]
    fn interrupted_write_leaves_old_content_intact() {
        // Simulate a crash between temp-write and rename: the temp file is
        // written but never persisted. The target must be untouched.
        let dir = tmp();
        let path = dir.path().join("f.md");
        write_atomic(&path, "original\n").unwrap();

        let tmp_file = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        std::io::Write::write_all(&mut tmp_file.as_file(), b"half-written").unwrap();
        drop(tmp_file); // "crash": temp cleaned up, no rename happened

        assert_eq!(read(&path).unwrap(), "original\n");
    }

    #[test]
    fn missing_file_maps_to_not_found() {
        let dir = tmp();
        let path = dir.path().join("missing.md");
        assert!(matches!(read(&path), Err(Error::NotFound { .. })));
        assert_eq!(read_opt(&path).unwrap(), None);
        assert!(matches!(remove(&path), Err(Error::NotFound { .. })));
        assert!(!exists(&path));
    }

    #[test]
    fn rmw_mutates_in_place_and_preserves_body() {
        let dir = tmp();
        let path = dir.path().join("t.md");
        write_atomic(&path, "---\nstatus: open\nextra: kept\n---\nBody stays.\n").unwrap();

        read_modify_write(&path, |doc| {
            doc.set("status", "closed");
            Ok(())
        })
        .unwrap();

        let after = read(&path).unwrap();
        assert!(after.contains("status: closed"));
        assert!(after.contains("extra: kept"));
        assert!(after.ends_with("---\nBody stays.\n"));
    }

    #[test]
    fn rmw_on_unparseable_file_fails_without_touching_it() {
        let dir = tmp();
        let path = dir.path().join("bad.md");
        let original = "---\n: : : not yaml\n---\nbody\n";
        write_atomic(&path, original).unwrap();

        let err = read_modify_write(&path, |_| Ok(())).unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
        assert_eq!(read(&path).unwrap(), original, "failed RMW must not rewrite the file");
    }

    #[test]
    fn rmw_callback_error_aborts_write() {
        let dir = tmp();
        let path = dir.path().join("t.md");
        write_atomic(&path, "---\na: 1\n---\n").unwrap();
        let err = read_modify_write(&path, |doc| {
            doc.set("a", 2);
            Err(Error::Parse { message: "caller bailed".into() })
        })
        .unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
        assert!(read(&path).unwrap().contains("a: 1"), "aborted RMW must not persist mutations");
    }
}
