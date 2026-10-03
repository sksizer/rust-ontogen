//! Exact record names on filesystems that alias them.
//!
//! macOS (APFS, HFS+) and Windows (NTFS) resolve a name to a file stored
//! under another spelling: another letter case, and on macOS another
//! Unicode normalization form of the same text. There, opening `KEPT.md`
//! opens `kept.md`. A record's id is its file stem byte for byte, so a
//! lookup must not take such a match for the record: reading it would
//! report the wrong id, and updating or deleting it would change another
//! record.
//!
//! [`NameCheck`] tells the two apart. It probes once whether the vault's
//! filesystem aliases names; where it does not, an open is already exact
//! and nothing more is done. Where it does, the record's directory is
//! listed to find an entry spelled exactly like the requested name.

use std::{ffi::OsStr, fs, io, path::Path, sync::OnceLock};

use crate::error::Error;

/// The tail of the probe file's name: an uppercase letter and a composed
/// (NFC) `é`, so both a case-folded and a decomposed (NFD) spelling exist.
const PROBE_TAIL: &str = "-X\u{e9}";
/// Spellings of [`PROBE_TAIL`] that only an aliasing filesystem resolves to
/// the probe file: lowercase, and NFD.
const PROBE_VARIANTS: [&str; 2] = ["-x\u{e9}", "-Xe\u{301}"];

/// Whether files are stored under exactly the names a lookup asks for. One
/// per vault handle, shared by its clones; the probe runs on the first
/// check and its answer is kept for the handle's life. The whole vault is
/// assumed to sit on one filesystem.
#[derive(Debug, Default)]
pub(crate) struct NameCheck {
    aliasing: OnceLock<bool>,
}

impl NameCheck {
    /// Whether `path` is not ruled out as a file stored under exactly its
    /// own final component. On a filesystem that aliases names this is
    /// whether the parent directory lists that exact name; elsewhere a
    /// lookup of `path` cannot reach another file, so it is `true` without
    /// touching the disk, and the caller's own open decides whether the file
    /// exists.
    pub(crate) fn exact(&self, path: &Path) -> Result<bool, Error> {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Ok(false);
        };
        let aliasing = match self.aliasing.get() {
            Some(&aliasing) => aliasing,
            None => match probe(dir) {
                Ok(aliasing) => *self.aliasing.get_or_init(|| aliasing),
                // No directory, so no record: nothing learned, nothing kept.
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
                // A directory the probe cannot write to (a read-only vault):
                // the listing is exact on every filesystem, so use it.
                Err(_) => *self.aliasing.get_or_init(|| true),
            },
        };
        if !aliasing {
            return Ok(true);
        }
        lists(dir, name)
    }
}

/// Create a probe file in `dir` and see whether a case or normalization
/// variant of its name opens it. The file is removed when the probe ends.
/// Its name starts with `.` and has no record extension, so a concurrent
/// listing does not see it.
fn probe(dir: &Path) -> io::Result<bool> {
    let file = tempfile::Builder::new().prefix(".markdown-store-probe-").suffix(PROBE_TAIL).tempfile_in(dir)?;
    let name = file.path().file_name().and_then(OsStr::to_str).unwrap_or_default();
    let Some(head) = name.strip_suffix(PROBE_TAIL) else {
        return Ok(true);
    };
    Ok(PROBE_VARIANTS.iter().any(|tail| fs::symlink_metadata(dir.join(format!("{head}{tail}"))).is_ok()))
}

/// Whether `dir` holds an entry named exactly `name`.
fn lists(dir: &Path, name: &OsStr) -> Result<bool, Error> {
    let io_err = |source: io::Error| Error::Io { path: dir.to_path_buf(), source };
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io_err(e)),
    };
    for entry in entries {
        if entry.map_err(io_err)?.file_name() == name {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether this test machine's temp filesystem resolves a case variant
    /// to an existing file.
    fn folds_case(dir: &Path) -> bool {
        fs::write(dir.join("Case-Probe"), "").unwrap();
        let folds = dir.join("case-probe").exists();
        fs::remove_file(dir.join("Case-Probe")).unwrap();
        folds
    }

    #[test]
    fn the_probe_matches_the_filesystem_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let aliasing = probe(dir.path()).unwrap();
        if folds_case(dir.path()) {
            assert!(aliasing, "a case-insensitive filesystem aliases names");
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "the probe file is removed");
    }

    #[test]
    fn exact_names_match_and_variants_do_not() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("kept.md"), "").unwrap();
        let check = NameCheck::default();
        assert!(check.exact(&dir.path().join("kept.md")).unwrap());
        for variant in ["KEPT.md", "Kept.md"] {
            assert!(
                !check.exact(&dir.path().join(variant)).unwrap() || !dir.path().join(variant).exists(),
                "{variant} is ruled out wherever it would open kept.md"
            );
        }
        let fresh = NameCheck::default();
        assert!(!fresh.exact(&dir.path().join("missing/kept.md")).unwrap(), "no directory, no record");
        assert!(fresh.aliasing.get().is_none(), "and nothing learned about the filesystem");
    }
}
