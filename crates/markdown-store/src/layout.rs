//! Vault layout: how record ids map onto file paths.
//!
//! Path construction is the crate's security boundary. Record ids and entity
//! directory segments arrive from API input at runtime, so both are
//! validated before they touch a `PathBuf` — making path traversal out of
//! the vault root impossible by construction rather than by caller
//! discipline. Everything else in the crate funnels through
//! [`VaultLayout::record_path`].

use std::path::{Path, PathBuf};

use crate::error::Error;

/// On-disk arrangement of record files under the vault root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultLayout {
    /// One directory per entity: `<root>/<dir_segment>/<id>.md`. The
    /// default, and the layout ADR 0001 documents.
    PerEntityDir,
    /// All records directly under the root: `<root>/<id>.md`. Every entity
    /// shares one id space and raw listings see every entity's files;
    /// the store's `EntityRecords` view tells them apart by their OKF `type`.
    Flat,
}

impl VaultLayout {
    /// Resolve the file path for one record. Validates both `dir_segment`
    /// and `id` (see [`validate_id`] / [`validate_segment`]).
    ///
    /// ```
    /// use markdown_store::VaultLayout;
    /// use std::path::Path;
    ///
    /// let p = VaultLayout::PerEntityDir.record_path(Path::new("vault"), "tasks", "t-1")?;
    /// assert_eq!(p, Path::new("vault/tasks/t-1.md"));
    ///
    /// // Traversal attempts are rejected, not resolved:
    /// assert!(VaultLayout::PerEntityDir.record_path(Path::new("vault"), "tasks", "../escape").is_err());
    /// # Ok::<(), markdown_store::Error>(())
    /// ```
    pub fn record_path(&self, vault_root: &Path, dir_segment: &str, id: &str) -> Result<PathBuf, Error> {
        validate_id(id)?;
        match self {
            VaultLayout::PerEntityDir => {
                validate_segment(dir_segment)?;
                Ok(vault_root.join(dir_segment).join(format!("{id}.md")))
            }
            VaultLayout::Flat => Ok(vault_root.join(format!("{id}.md"))),
        }
    }

    /// Resolve the directory listed when enumerating an entity's records.
    /// For [`VaultLayout::Flat`] this is the vault root itself.
    pub fn entity_dir(&self, vault_root: &Path, dir_segment: &str) -> Result<PathBuf, Error> {
        match self {
            VaultLayout::PerEntityDir => {
                validate_segment(dir_segment)?;
                Ok(vault_root.join(dir_segment))
            }
            VaultLayout::Flat => Ok(vault_root.to_path_buf()),
        }
    }
}

/// Filename stems OKF reserves at every level of a bundle: `index.md` is a
/// directory listing and `log.md` an update history, never a concept.
pub const RESERVED_IDS: &[&str] = &["index", "log"];

/// Whether `id` is one of [`RESERVED_IDS`]. Compared ASCII
/// case-insensitively because macOS and Windows filesystems alias
/// `Index.md` onto `index.md`.
pub fn is_reserved_id(id: &str) -> bool {
    RESERVED_IDS.iter().any(|r| r.eq_ignore_ascii_case(id))
}

/// Validate a record id for use as a filename stem.
///
/// Rejected: everything [`validate_segment`] rejects, plus the OKF
/// reserved stems `index` and `log` (see [`is_reserved_id`]).
pub fn validate_id(id: &str) -> Result<(), Error> {
    validate_stem(id).map_err(|reason| Error::InvalidId { id: id.to_string(), reason })?;
    if is_reserved_id(id) {
        return Err(Error::InvalidId {
            id: id.to_string(),
            reason: "is reserved: OKF uses index.md and log.md for directory listings and update logs".into(),
        });
    }
    Ok(())
}

/// Validate an entity directory segment, reported as
/// [`Error::InvalidSegment`].
///
/// Rejected: empty segments; path separators (`/`, `\`); `:` (a Windows
/// drive prefix like `C:evil` makes `Path::join` *replace* the base — a
/// vault escape — and `:` also addresses NTFS alternate data streams); NUL;
/// `.` and `..`; a leading `.` (hidden files are skipped by the default
/// walk, so a dot-leading record would be written but never listed); and a
/// trailing `.` or space (silently stripped by Windows, aliasing two names
/// onto one file). Unlike ids, `index` and `log` are fine: OKF reserves
/// them as filenames, not as directory names.
pub fn validate_segment(segment: &str) -> Result<(), Error> {
    validate_stem(segment).map_err(|reason| Error::InvalidSegment { segment: segment.to_string(), reason })
}

/// The path-safety rules ids and segments share.
fn validate_stem(s: &str) -> Result<(), String> {
    let reject = |reason: &str| -> Result<(), String> { Err(reason.to_string()) };
    if s.is_empty() {
        return reject("must not be empty");
    }
    if s == "." || s == ".." {
        return reject("must not be a dot path");
    }
    if s.starts_with('.') {
        return reject("must not start with '.' (hidden files are not listed)");
    }
    if s.ends_with('.') || s.ends_with(' ') {
        return reject("must not end with '.' or a space (stripped by Windows, aliasing ids)");
    }
    if s.contains('/') || s.contains('\\') {
        return reject("must not contain path separators");
    }
    if s.contains(':') {
        return reject("must not contain ':' (a Windows drive prefix like C: escapes the vault root)");
    }
    if s.contains('\0') {
        return reject("must not contain NUL");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_entity_dir_paths() {
        let layout = VaultLayout::PerEntityDir;
        assert_eq!(
            layout.record_path(Path::new("docs/data"), "tasks", "t-1").unwrap(),
            PathBuf::from("docs/data/tasks/t-1.md")
        );
        assert_eq!(layout.entity_dir(Path::new("docs/data"), "tasks").unwrap(), PathBuf::from("docs/data/tasks"));
    }

    #[test]
    fn flat_paths_ignore_segment() {
        let layout = VaultLayout::Flat;
        assert_eq!(layout.record_path(Path::new("v"), "anything", "t-1").unwrap(), PathBuf::from("v/t-1.md"));
        assert_eq!(layout.entity_dir(Path::new("v"), "anything").unwrap(), PathBuf::from("v"));
    }

    #[test]
    fn traversal_attempts_rejected() {
        let layout = VaultLayout::PerEntityDir;
        for bad in
            ["../escape", "..", "a/b", "a\\b", "", ".", ".hidden", "x\0y", "C:evil", "a:stream", "dotted.", "spaced "]
        {
            assert!(layout.record_path(Path::new("v"), "tasks", bad).is_err(), "id {bad:?} must be rejected");
        }
        for bad in ["../up", "a/b", "", "."] {
            assert!(layout.record_path(Path::new("v"), bad, "ok").is_err(), "segment {bad:?} must be rejected");
        }
    }

    #[test]
    fn index_and_log_are_reserved_ids_in_any_case() {
        let layout = VaultLayout::PerEntityDir;
        for reserved in ["index", "log", "Index", "LOG", "lOg"] {
            assert!(
                matches!(layout.record_path(Path::new("v"), "tasks", reserved), Err(Error::InvalidId { .. })),
                "id {reserved:?} must be rejected"
            );
        }
        for fine in ["index-2", "logs", "changelog", "my-index"] {
            assert!(layout.record_path(Path::new("v"), "tasks", fine).is_ok(), "id {fine:?} must be accepted");
        }
    }

    #[test]
    fn reserved_ids_are_fine_as_directory_segments() {
        let layout = VaultLayout::PerEntityDir;
        assert_eq!(layout.record_path(Path::new("v"), "log", "entry-1").unwrap(), PathBuf::from("v/log/entry-1.md"));
        assert_eq!(layout.entity_dir(Path::new("v"), "index").unwrap(), PathBuf::from("v/index"));
    }

    #[test]
    fn dots_inside_ids_are_fine() {
        let layout = VaultLayout::PerEntityDir;
        assert_eq!(
            layout.record_path(Path::new("v"), "notes", "v1.2-notes").unwrap(),
            PathBuf::from("v/notes/v1.2-notes.md")
        );
    }
}
