//! The record-id rule both store backends enforce, and the slug function
//! derived ids come from.
//!
//! Generated SeaORM store code calls these at runtime. The markdown backend
//! enforces the same rule through `markdown_store::layout::validate_id`,
//! because that crate takes no ontogen dependency; a root-workspace test
//! (`tests/id_rule_parity.rs`) keeps the two in agreement. The rule is
//! the JSON:API wire contract's §8.2 validity rule.

use std::fmt;

/// Ids OKF reserves at every level of a bundle: `index.md` is a directory
/// listing and `log.md` an update history. Held to on both backends so a
/// SeaORM store can move to a markdown vault without renaming records.
pub const RESERVED_IDS: &[&str] = &["index", "log"];

/// Whether `id` is one of [`RESERVED_IDS`], compared ASCII
/// case-insensitively (macOS and Windows filesystems alias `Index.md` onto
/// `index.md`).
pub fn is_reserved_id(id: &str) -> bool {
    RESERVED_IDS.iter().any(|r| r.eq_ignore_ascii_case(id))
}

/// An id rejected by [`validate_id`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidId {
    /// The rejected id.
    pub id: String,
    /// Why it was rejected.
    pub reason: &'static str,
}

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid id {:?}: {}", self.id, self.reason)
    }
}

impl std::error::Error for InvalidId {}

/// Check an id being created against the shared rule.
///
/// Valid: a non-empty string that is not whitespace-only, contains no `/`,
/// `\`, `:` or NUL, does not start with `.`, does not end with `.` or a
/// space, and is not a reserved id (see [`is_reserved_id`]).
///
/// ```
/// use ontogen_core::id::validate_id;
/// assert!(validate_id("ship-the-parser").is_ok());
/// assert!(validate_id("v1.2-notes").is_ok());
/// assert!(validate_id("\t").is_err());
/// assert!(validate_id("Index").is_err());
/// assert!(validate_id("a/b").is_err());
/// ```
pub fn validate_id(id: &str) -> Result<(), InvalidId> {
    let reject = |reason: &'static str| Err(InvalidId { id: id.to_string(), reason });
    if id.is_empty() {
        return reject("must not be empty");
    }
    if id.trim().is_empty() {
        return reject("must not be whitespace-only");
    }
    if id.starts_with('.') {
        return reject("must not start with '.'");
    }
    if id.ends_with('.') || id.ends_with(' ') {
        return reject("must not end with '.' or a space");
    }
    if id.contains('/') || id.contains('\\') {
        return reject("must not contain path separators");
    }
    if id.contains(':') {
        return reject("must not contain ':'");
    }
    if id.contains('\0') {
        return reject("must not contain NUL");
    }
    if is_reserved_id(id) {
        return reject("is reserved: OKF uses index and log for directory listings and update logs");
    }
    Ok(())
}

/// Slugify a string into an id: ASCII-lowercased alphanumerics, every run of
/// anything else collapsed to one hyphen, no leading or trailing hyphen.
/// Non-ASCII characters are separators. The result is empty when the input
/// has no ASCII alphanumeric.
///
/// ```
/// use ontogen_core::id::slugify;
/// assert_eq!(slugify("Ship the Parser!"), "ship-the-parser");
/// assert_eq!(slugify("  --Weird__ input--  "), "weird-input");
/// assert_eq!(slugify("***"), "");
/// ```
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_hyphen = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            pending_hyphen = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending_hyphen = true;
        }
    }
    out
}

/// The ids a derived base is probed under, in order: `base`, then `base-2`,
/// `base-3`, and so on without end. A reserved base is skipped, so a title
/// that slugifies to `index` lands on `index-2`.
///
/// ```
/// use ontogen_core::id::candidates;
/// let first: Vec<String> = candidates("draft").take(3).collect();
/// assert_eq!(first, ["draft", "draft-2", "draft-3"]);
/// assert_eq!(candidates("index").next().as_deref(), Some("index-2"));
/// ```
pub fn candidates(base: &str) -> impl Iterator<Item = String> + use<> {
    let base = base.to_string();
    let first = (!is_reserved_id(&base)).then(|| base.clone());
    first.into_iter().chain((2u64..).map(move |n| format!("{base}-{n}")))
}

/// A fresh random (v4) UUID, hyphenated and lowercase.
#[cfg(feature = "uuid")]
pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_ids() {
        for ok in ["a", "ship-the-parser", "v1.2-notes", "index-2", "logs", "my index", "é", "B"] {
            assert!(validate_id(ok).is_ok(), "{ok:?} must be accepted");
        }
    }

    #[test]
    fn rejects_what_the_rule_rejects() {
        for bad in ["", " ", "\t", "\n ", ".", "..", ".hidden", "dotted.", "spaced ", "a/b", "a\\b", "C:x", "x\0y"] {
            assert!(validate_id(bad).is_err(), "{bad:?} must be rejected");
        }
        for reserved in ["index", "log", "Index", "LOG", "lOg"] {
            assert!(validate_id(reserved).is_err(), "{reserved:?} is reserved");
        }
    }

    #[test]
    fn the_error_names_the_id_and_the_reason() {
        let err = validate_id("\t").unwrap_err();
        assert_eq!(err.to_string(), "invalid id \"\\t\": must not be whitespace-only");
    }

    #[test]
    fn candidates_skip_a_reserved_base() {
        assert_eq!(candidates("LOG").take(2).collect::<Vec<_>>(), ["LOG-2", "LOG-3"]);
        assert_eq!(candidates("a").nth(9).as_deref(), Some("a-10"));
    }

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("v1.2 release notes"), "v1-2-release-notes");
        assert_eq!(slugify("Äpfel und Birnen"), "pfel-und-birnen");
        assert_eq!(slugify(""), "");
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn new_uuid_is_a_valid_unique_id() {
        let (a, b) = (new_uuid(), new_uuid());
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert!(validate_id(&a).is_ok());
    }
}
