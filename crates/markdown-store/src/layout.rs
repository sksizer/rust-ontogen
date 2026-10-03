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
    /// Resolve the file path for one record. Checks `dir_segment` with
    /// [`validate_segment`] and `id` with [`validate_lookup_id`], the
    /// path-safety rule: a record is reachable under any stem that is safe
    /// to join, including one the create rule ([`validate_id`]) refuses.
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
        validate_lookup_id(id)?;
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

/// The Windows device names. Windows opens the device for a file named
/// one of these, in any case and with any extension (`con.md`,
/// `NUL.tar.gz`), so no record file created there can carry one as its
/// stem.
pub const DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt0",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Whether `id`, or the part of it before its first `.`, is one of
/// [`DEVICE_NAMES`], compared ASCII case-insensitively. The create rule
/// refuses these; a `con.md` written on Linux or macOS stays listable and
/// reachable there. On Windows a lookup of one is
/// [`Error::NotFound`] unopened (see [`crate::VaultHandle`]).
///
/// ```
/// use markdown_store::layout::is_device_name;
/// assert!(is_device_name("con") && is_device_name("nul.x") && is_device_name("COM1.backup"));
/// assert!(!is_device_name("console") && !is_device_name("con-2") && !is_device_name("x.con"));
/// ```
pub fn is_device_name(id: &str) -> bool {
    let stem = id.split('.').next().unwrap_or(id);
    DEVICE_NAMES.iter().any(|d| d.eq_ignore_ascii_case(stem))
}

/// The whole create rule, as an [`Error::InvalidId`] from [`validate_id`]
/// states it after the clause the id broke.
pub const ID_RULE: &str = "an id is 1-200 bytes of a-z, 0-9, '.', '_', '~', '-', not starting or ending with '.', \
                           and not a reserved name";

/// Why an id in [`RESERVED_IDS`] is refused.
pub const RESERVED_REASON: &str =
    "is reserved: OKF (Open Knowledge Format) uses index and log for directory listings and update logs";

/// Why [`validate_id`] refuses a device name (see [`is_device_name`]).
pub const DEVICE_NAME_REASON: &str =
    "is reserved: Windows has no file named con, prn, aux, nul, com0-com9 or lpt0-lpt9, with or without an extension";

/// The longest id [`validate_id`] accepts, in bytes. With `.md` appended
/// it stays under the 255-byte filename limit of common filesystems.
pub const MAX_ID_LEN: usize = 200;

/// The longest stem [`validate_lookup_id`] lets through, in bytes: a
/// 255-byte filename less `.md`. A longer stem names no file that can
/// exist, so it is refused before the filesystem fails on it.
pub const MAX_STEM_LEN: usize = 252;

/// Validate the id of a record being created.
///
/// Valid: 1 to [`MAX_ID_LEN`] bytes of lowercase ASCII letters, digits,
/// `.`, `_`, `~` and `-`, not starting or ending with `.`, not an OKF
/// reserved stem (`index`, `log`; see [`is_reserved_id`]) and not a device
/// name (see [`is_device_name`]). Every such id names a file on Linux,
/// macOS and Windows, and passes [`validate_lookup_id`]. A reserved name is
/// reported before a case error, since lowercasing cannot fix it. This is
/// the id rule of ontogen's JSON:API wire contract (§8.2); the ontogen
/// workspace tests that its own copy of the rule agrees with this one.
///
/// ```
/// use markdown_store::layout::validate_id;
/// assert!(validate_id("v1.2_notes~draft").is_ok());
/// assert!(validate_id("Draft").is_err());
/// assert!(validate_id("café").is_err());
/// assert!(validate_id("nul.txt").is_err());
/// ```
pub fn validate_id(id: &str) -> Result<(), Error> {
    let reject = |reason: &str| Err(Error::InvalidId { id: id.to_string(), reason: reason.into(), create_rule: true });
    if id.is_empty() {
        return reject("must not be empty");
    }
    if id.len() > MAX_ID_LEN {
        return reject("must be at most 200 bytes");
    }
    if is_reserved_id(id) {
        return reject(RESERVED_REASON);
    }
    if is_device_name(id) {
        return reject(DEVICE_NAME_REASON);
    }
    if id.starts_with('.') {
        return reject("must not start with '.'");
    }
    if id.ends_with('.') {
        return reject("must not end with '.'");
    }
    if id.bytes().any(|b| b.is_ascii_uppercase()) {
        return reject("must not contain uppercase letters");
    }
    if !id.is_ascii() {
        return reject("must be ASCII");
    }
    if !id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'~' | b'-')) {
        return reject("may contain only a-z, 0-9, '.', '_', '~' and '-'");
    }
    Ok(())
}

/// Validate a record id for use as a filename stem when looking a record
/// up: the path-safety rule every read, update and delete goes through.
///
/// Looser than [`validate_id`], so a record whose file was written by hand
/// or under an older rule (`Draft.md`, `café.md`) stays reachable.
/// Rejected: everything [`validate_segment`] rejects, a whitespace-only id,
/// a stem longer than [`MAX_STEM_LEN`], and the OKF reserved stems `index`
/// and `log` (see [`is_reserved_id`]).
///
/// ```
/// use markdown_store::layout::validate_lookup_id;
/// assert!(validate_lookup_id("Draft").is_ok());
/// assert!(validate_lookup_id("café").is_ok());
/// assert!(validate_lookup_id("../escape").is_err());
/// assert!(validate_lookup_id("index").is_err());
/// ```
pub fn validate_lookup_id(id: &str) -> Result<(), Error> {
    let invalid = |reason: String| Error::InvalidId { id: id.to_string(), reason, create_rule: false };
    let reject = |reason: &str| Err(invalid(reason.into()));
    validate_stem(id).map_err(invalid)?;
    if id.trim().is_empty() {
        return reject("must not be whitespace-only");
    }
    if id.len() > MAX_STEM_LEN {
        return reject("must be at most 252 bytes (a 255-byte filename less `.md`)");
    }
    if is_reserved_id(id) {
        return reject(RESERVED_REASON);
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
        for bad in [
            "../escape",
            "..",
            "a/b",
            "a\\b",
            "",
            ".",
            ".hidden",
            "x\0y",
            "C:evil",
            "a:stream",
            "dotted.",
            "spaced ",
            "\t",
            "\n\t",
        ] {
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
        let err = layout.record_path(Path::new("v"), "tasks", "index").unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("invalid id \"index\": {RESERVED_REASON}"),
            "a lookup states no create rule"
        );
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
    fn lookups_reach_stems_the_create_rule_refuses() {
        let layout = VaultLayout::PerEntityDir;
        let long = "x".repeat(MAX_STEM_LEN);
        for stem in ["Draft", "café", "a b", "a+b", "Ω", long.as_str()] {
            assert!(validate_id(stem).is_err(), "{stem:?} is not creatable");
            assert_eq!(
                layout.record_path(Path::new("v"), "notes", stem).unwrap(),
                PathBuf::from(format!("v/notes/{stem}.md")),
                "{stem:?} is reachable"
            );
        }
        let too_long = "x".repeat(MAX_STEM_LEN + 1);
        assert!(matches!(layout.record_path(Path::new("v"), "notes", &too_long), Err(Error::InvalidId { .. })));
    }

    #[test]
    fn the_create_rule() {
        let longest = "a".repeat(MAX_ID_LEN);
        for ok in ["a", "0", "v1.2-notes", "a_b", "a~b", "index-2", longest.as_str()] {
            assert!(validate_id(ok).is_ok(), "{ok:?} must be accepted");
        }
        let reason = |id: &str| match validate_id(id) {
            Err(Error::InvalidId { reason, .. }) => reason,
            other => panic!("{id:?}: {other:?}"),
        };
        assert_eq!(reason(""), "must not be empty");
        assert_eq!(reason(&"a".repeat(MAX_ID_LEN + 1)), "must be at most 200 bytes");
        assert_eq!(reason("Index"), RESERVED_REASON, "reserved before uppercase");
        assert_eq!(reason("CON"), DEVICE_NAME_REASON, "reserved before uppercase");
        assert_eq!(reason("con."), DEVICE_NAME_REASON, "reserved before the trailing dot");
        assert_eq!(reason("."), "must not start with '.'");
        assert_eq!(reason("a."), "must not end with '.'");
        assert_eq!(reason("Ab"), "must not contain uppercase letters");
        assert_eq!(reason("é"), "must be ASCII");
        for charset in [" ", "a b", "a/b", "a\\b", "c:d", "x\0y"] {
            assert_eq!(reason(charset), "may contain only a-z, 0-9, '.', '_', '~' and '-'", "{charset:?}");
        }
    }

    #[test]
    fn a_create_rule_failure_states_the_rule_and_the_remedy() {
        assert_eq!(
            validate_id("Ship").unwrap_err().to_string(),
            "invalid id \"Ship\": must not contain uppercase letters; an id is 1-200 bytes of a-z, 0-9, '.', '_', \
             '~', '-', not starting or ending with '.', and not a reserved name: choose another id"
        );
    }

    #[test]
    fn device_names_are_refused_on_create_only() {
        let layout = VaultLayout::PerEntityDir;
        for device in [
            "con",
            "prn",
            "aux",
            "nul",
            "com0",
            "com1",
            "com9",
            "lpt0",
            "lpt1",
            "lpt9",
            "nul.x",
            "com1.backup",
            "Con.md",
            "LPT0.txt",
        ] {
            match validate_id(device) {
                Err(Error::InvalidId { reason, .. }) => assert_eq!(reason, DEVICE_NAME_REASON, "{device:?}"),
                other => panic!("{device:?}: {other:?}"),
            }
            assert!(layout.record_path(Path::new("v"), "notes", device).is_ok(), "{device:?} stays reachable");
        }
        for fine in ["console", "con-2", "xcon", "a.con", "com", "com10", "lpt10", "nulls"] {
            assert!(validate_id(fine).is_ok(), "{fine:?} must be accepted");
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
