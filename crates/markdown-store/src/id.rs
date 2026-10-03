//! Record id derivation: how a new record gets a filename stem.
//!
//! Ids double as filenames (`<dir>/<id>.md`), so derivation and validation
//! live next to each other: everything produced here passes the path-safety
//! rules in [`crate::layout`] by construction.

use crate::error::Error;

/// How `create` derives an id when the caller didn't supply one.
///
/// A non-empty caller-supplied id always wins, under every strategy — the
/// strategy only fills the gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdStrategy {
    /// The caller must supply the id; without one, a create is
    /// [`Error::IdRequired`].
    Provided,
    /// Slugify the value of the named field (e.g. `"title"`). The field
    /// *name* is carried so code generators know which field's value to
    /// pass; [`IdStrategy::make_id`] receives that value.
    SlugFromField(String),
    /// A fresh UUID v4. Requires the `uuid` cargo feature; constructing the
    /// variant is always possible, but deriving an id without the feature
    /// returns [`Error::InvalidId`].
    Uuid,
}

impl IdStrategy {
    /// Derive the id for a new record.
    ///
    /// `provided` is the caller-supplied id (wins unless empty or
    /// whitespace-only); `source_value` is the value of the slug-source
    /// field for [`IdStrategy::SlugFromField`] (ignored otherwise). With no
    /// id to return, it is [`Error::IdRequired`].
    ///
    /// ```
    /// use markdown_store::IdStrategy;
    /// let s = IdStrategy::SlugFromField("title".into());
    /// assert_eq!(s.make_id(None, Some("Ship the Parser!")).unwrap(), "ship-the-parser");
    /// assert_eq!(s.make_id(Some("explicit-id"), Some("ignored")).unwrap(), "explicit-id");
    /// assert!(IdStrategy::Provided.make_id(None, None).is_err());
    /// ```
    pub fn make_id(&self, provided: Option<&str>, source_value: Option<&str>) -> Result<String, Error> {
        if let Some(id) = provided {
            if !id.trim().is_empty() {
                return Ok(id.to_string());
            }
        }
        match self {
            IdStrategy::Provided => {
                Err(Error::IdRequired { reason: "this store requires the caller to supply an id".into() })
            }
            IdStrategy::SlugFromField(field) => {
                let source = source_value.unwrap_or("");
                let slug = slugify(source);
                if slug.is_empty() {
                    return Err(Error::IdRequired { reason: format!("field {field:?} produced an empty slug") });
                }
                Ok(slug)
            }
            IdStrategy::Uuid => {
                #[cfg(feature = "uuid")]
                {
                    Ok(uuid::Uuid::new_v4().to_string())
                }
                #[cfg(not(feature = "uuid"))]
                {
                    Err(Error::InvalidId {
                        id: String::new(),
                        reason: "IdStrategy::Uuid requires the `uuid` cargo feature".into(),
                    })
                }
            }
        }
    }
}

/// Slugify a string into a filename-safe id: ASCII-lowercased alphanumerics,
/// runs of everything else collapsed to single hyphens, no leading/trailing
/// hyphen. Non-ASCII characters are treated as separators — for vaults whose
/// titles need richer ids, supply the id explicitly instead.
///
/// ```
/// assert_eq!(markdown_store::id::slugify("Ship the Parser!"), "ship-the-parser");
/// assert_eq!(markdown_store::id::slugify("  --Weird__ input--  "), "weird-input");
/// assert_eq!(markdown_store::id::slugify("***"), "");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("v1.2 release notes"), "v1-2-release-notes");
        assert_eq!(slugify("Äpfel und Birnen"), "pfel-und-birnen");
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("a"), "a");
    }

    #[test]
    fn provided_wins_under_every_strategy() {
        for strategy in [IdStrategy::Provided, IdStrategy::SlugFromField("title".into()), IdStrategy::Uuid] {
            assert_eq!(strategy.make_id(Some("explicit"), Some("Title")).unwrap(), "explicit");
        }
    }

    #[test]
    fn whitespace_only_provided_does_not_win() {
        let s = IdStrategy::SlugFromField("title".into());
        assert_eq!(s.make_id(Some("   "), Some("Real Title")).unwrap(), "real-title");
    }

    #[test]
    fn slug_strategy_requires_an_id_when_the_slug_is_empty() {
        let s = IdStrategy::SlugFromField("title".into());
        for source in [Some("???"), None] {
            match s.make_id(None, source) {
                Err(Error::IdRequired { reason }) => assert_eq!(reason, "field \"title\" produced an empty slug"),
                other => panic!("expected IdRequired, got {other:?}"),
            }
        }
    }

    #[test]
    fn provided_strategy_requires_an_id() {
        for provided in [None, Some(""), Some(" \t")] {
            match IdStrategy::Provided.make_id(provided, Some("Title")) {
                Err(Error::IdRequired { reason }) => {
                    assert_eq!(reason, "this store requires the caller to supply an id")
                }
                other => panic!("expected IdRequired for {provided:?}, got {other:?}"),
            }
        }
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn uuid_strategy_generates_unique_valid_ids() {
        let a = IdStrategy::Uuid.make_id(None, None).unwrap();
        let b = IdStrategy::Uuid.make_id(None, None).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        crate::layout::validate_id(&a).unwrap();
    }
}
