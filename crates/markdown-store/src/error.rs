//! The crate-wide error type.
//!
//! `#[non_exhaustive]` so new failure modes can land without a semver break,
//! mirroring `markdown-vault`'s `ExtractError` convention.

use std::{io, path::PathBuf};

use thiserror::Error;

/// Errors produced by frontmatter parsing/serialization, vault file
/// operations, and record listing.
///
/// Consumers embedding this crate behind their own error enum typically write
/// a single `From<markdown_store::Error>` impl; the variants are designed so
/// that "not found" and "already exists" remain distinguishable (they map to
/// distinct consumer semantics) while everything else can collapse into a
/// string if the consumer prefers.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum Error {
    /// A record file was expected to exist and does not.
    #[error("record not found: {path}")]
    NotFound {
        /// The path that was probed.
        path: PathBuf,
    },

    /// A record file already exists where a new record was being created.
    /// Creation never silently overwrites.
    #[error("record already exists: {path}")]
    AlreadyExists {
        /// The path that already exists.
        path: PathBuf,
    },

    /// An underlying I/O failure (read, write, rename, remove, walk).
    #[error("I/O error at {path}: {source}")]
    Io {
        /// The path being operated on when the failure occurred.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: io::Error,
    },

    /// The frontmatter block exists but is not valid YAML, is not a mapping,
    /// or does not deserialize into the requested type.
    #[error("frontmatter parse error: {message}")]
    Parse {
        /// Human-readable description, including the offending path when the
        /// failure happened during a file operation.
        message: String,
    },

    /// A value could not be serialized into a YAML frontmatter mapping.
    #[error("frontmatter serialize error: {message}")]
    Serialize {
        /// Human-readable description.
        message: String,
    },

    /// A directory listing exceeded the configured cap. The cap exists so a
    /// store pointed at an unexpectedly large directory fails loudly instead
    /// of parsing tens of thousands of files per `list()` call (ADR 0001's
    /// explicit scale ceiling).
    #[error("listing {dir} exceeded the configured cap: {count} entries > {cap}")]
    ListCapExceeded {
        /// The directory being listed.
        dir: PathBuf,
        /// How many record files were found.
        count: usize,
        /// The configured cap.
        cap: usize,
    },

    /// A record id failed validation. Ids become filename stems, so they are
    /// validated at the path-construction boundary to make path traversal
    /// impossible (see [`crate::layout`]). An id refused on create also
    /// gets the whole create rule ([`crate::layout::ID_RULE`]) in its
    /// message.
    #[error("invalid id {id:?}: {reason}{}", create_rule_suffix(*.create_rule))]
    InvalidId {
        /// The rejected id.
        id: String,
        /// Why it was rejected: the one clause it broke.
        reason: String,
        /// Whether `reason` is a clause of the create rule
        /// ([`crate::layout::validate_id`]) rather than of the looser
        /// path-safety rule lookups go through.
        create_rule: bool,
    },

    /// A new record has no id: the caller supplied none and the
    /// [`IdStrategy`](crate::IdStrategy) could not derive one (the strategy
    /// is `Provided`, or the slug source slugified to nothing).
    #[error("id required: {reason}")]
    IdRequired {
        /// Why no id could be derived.
        reason: String,
    },

    /// An entity directory segment failed validation: the path-safety rules
    /// lookup ids follow, and not a Windows device name.
    #[error("invalid path segment {segment:?}: {reason}")]
    InvalidSegment {
        /// The rejected segment.
        segment: String,
        /// Why it was rejected.
        reason: String,
    },
}

fn create_rule_suffix(create_rule: bool) -> String {
    if create_rule {
        format!("; {}: choose another id", crate::layout::ID_RULE)
    } else {
        String::new()
    }
}

impl Error {
    /// Construct a [`Error::Parse`] with a path prefix in the message.
    /// (Only the file-operation layers attach paths, hence the gate —
    /// without it, `--no-default-features` builds flag dead code.)
    #[cfg(feature = "fsops")]
    pub(crate) fn parse_at(path: &std::path::Path, message: impl std::fmt::Display) -> Self {
        Error::Parse { message: format!("{}: {message}", path.display()) }
    }
}
