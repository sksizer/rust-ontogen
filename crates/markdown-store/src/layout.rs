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
    /// All records directly under the root: `<root>/<id>.md`. With more
    /// than one entity this relies on a frontmatter type discriminator and
    /// id prefixes for disambiguation; listings see every entity's files.
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

/// Validate a record id for use as a filename stem.
///
/// Rejected: empty ids, path separators (`/`, `\`), NUL, `.` and `..`, and
/// a leading `.` (hidden files are skipped by the default walk, so a
/// dot-leading record would be written but never listed).
pub fn validate_id(id: &str) -> Result<(), Error> {
    let reject =
        |reason: &str| -> Result<(), Error> { Err(Error::InvalidId { id: id.to_string(), reason: reason.into() }) };
    if id.is_empty() {
        return reject("must not be empty");
    }
    if id == "." || id == ".." {
        return reject("must not be a dot path");
    }
    if id.starts_with('.') {
        return reject("must not start with '.' (hidden files are not listed)");
    }
    if id.contains('/') || id.contains('\\') {
        return reject("must not contain path separators");
    }
    if id.contains('\0') {
        return reject("must not contain NUL");
    }
    Ok(())
}

/// Validate an entity directory segment. Same rules as [`validate_id`],
/// reported as [`Error::InvalidSegment`].
pub fn validate_segment(segment: &str) -> Result<(), Error> {
    validate_id(segment).map_err(|e| match e {
        Error::InvalidId { id, reason } => Error::InvalidSegment { segment: id, reason },
        other => other,
    })
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
        for bad in ["../escape", "..", "a/b", "a\\b", "", ".", ".hidden", "x\0y"] {
            assert!(layout.record_path(Path::new("v"), "tasks", bad).is_err(), "id {bad:?} must be rejected");
        }
        for bad in ["../up", "a/b", "", "."] {
            assert!(layout.record_path(Path::new("v"), bad, "ok").is_err(), "segment {bad:?} must be rejected");
        }
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
