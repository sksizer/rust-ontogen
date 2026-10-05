//! Record id derivation: how a new record gets a filename stem.
//!
//! Ids double as filenames (`<dir>/<id>.md`), so derivation and validation
//! live next to each other: a non-empty slug that is not reserved, and
//! every probe [`candidates`] yields, passes the create rule
//! ([`crate::layout::validate_id`]) by construction.

use crate::error::Error;

/// How `create` derives an id when the caller didn't supply one.
///
/// A non-empty caller-supplied id always wins, under every strategy — the
/// strategy only fills the gap.
///
/// The `Uuid` variant exists only with the `uuid` cargo feature, so code
/// that names it without the feature fails to compile rather than failing
/// every create at run time:
///
#[cfg_attr(feature = "uuid", doc = "```")]
#[cfg_attr(not(feature = "uuid"), doc = "```compile_fail")]
/// let id = markdown_store::IdStrategy::Uuid.make_id(None, None).unwrap();
/// assert_eq!(id.len(), 36);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdStrategy {
    /// The caller must supply the id; without one, a create is
    /// [`Error::IdRequired`].
    Provided,
    /// Slugify the value of the named field (e.g. `"title"`). The field
    /// *name* is carried so code generators know which field's value to
    /// pass; [`IdStrategy::make_id`] receives that value.
    SlugFromField(String),
    /// A fresh UUID v4, hyphenated lowercase (36 bytes), which passes the
    /// create rule. Only with the `uuid` cargo feature.
    #[cfg(feature = "uuid")]
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
            #[cfg(feature = "uuid")]
            IdStrategy::Uuid => Ok(uuid::Uuid::new_v4().to_string()),
        }
    }
}

/// The longest id [`slugify`] returns, in bytes. The 10 bytes it leaves
/// under [`crate::layout::MAX_ID_LEN`] hold a `-N` probe suffix for every N
/// below one billion, so every probe of a slug keeps the slug's whole text.
pub const SLUG_MAX_LEN: usize = 190;

/// Slugify a string into an id.
///
/// ASCII letters are lowercased; Latin letters with diacritics and Latin
/// ligatures are folded to ASCII (see [`fold_latin`]); combining marks are
/// dropped, so a decomposed `e` + acute folds like `é`; every run of
/// anything else becomes one `-`; leading and trailing `-` are trimmed. A
/// result longer than [`SLUG_MAX_LEN`] is cut there, and a `-` the cut
/// leaves at the end is trimmed. The result is empty when the input has
/// nothing to keep.
///
/// ```
/// use markdown_store::id::slugify;
/// assert_eq!(slugify("Ship the Parser!"), "ship-the-parser");
/// assert_eq!(slugify("  --Weird__ input--  "), "weird-input");
/// assert_eq!(slugify("Crème brûlée à Łódź"), "creme-brulee-a-lodz");
/// assert_eq!(slugify("Straße"), "strasse");
/// assert_eq!(slugify("***"), "");
/// ```
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_hyphen = false;
    let mut buf = [0u8; 4];
    for c in s.chars() {
        let text: &str = if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase().encode_utf8(&mut buf)
        } else if let Some(folded) = fold_latin(c) {
            folded
        } else {
            if !is_combining_mark(c) {
                pending_hyphen = true;
            }
            continue;
        };
        if pending_hyphen && !out.is_empty() {
            out.push('-');
        }
        pending_hyphen = false;
        out.push_str(text);
    }
    if out.len() > SLUG_MAX_LEN {
        out.truncate(SLUG_MAX_LEN);
        out.truncate(out.trim_end_matches('-').len());
    }
    out
}

/// The lowercase ASCII spelling of a letter from the Latin-1 Supplement or
/// Latin Extended-A block: a letter with a diacritic, or a ligature. `None`
/// for anything else, including those blocks' symbols (`×`, `÷`, …).
pub fn fold_latin(c: char) -> Option<&'static str> {
    Some(match c {
        // Latin-1 Supplement
        'À'..='Å' | 'à'..='å' => "a",
        'Æ' | 'æ' => "ae",
        'Ç' | 'ç' => "c",
        'È'..='Ë' | 'è'..='ë' => "e",
        'Ì'..='Ï' | 'ì'..='ï' => "i",
        'Ð' | 'ð' => "d",
        'Ñ' | 'ñ' => "n",
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' => "o",
        'Ù'..='Ü' | 'ù'..='ü' => "u",
        'Ý' | 'ý' | 'ÿ' => "y",
        'Þ' | 'þ' => "th",
        'ß' => "ss",
        // Latin Extended-A (U+0100..=U+017F): every code point is a letter
        'Ā'..='ą' => "a",
        'Ć'..='č' => "c",
        'Ď'..='đ' => "d",
        'Ē'..='ě' => "e",
        'Ĝ'..='ģ' => "g",
        'Ĥ'..='ħ' => "h",
        'Ĩ'..='ı' => "i",
        'Ĳ' | 'ĳ' => "ij",
        'Ĵ' | 'ĵ' => "j",
        'Ķ'..='ĸ' => "k",
        'Ĺ'..='ł' => "l",
        'Ń'..='ŋ' => "n",
        'Ō'..='ő' => "o",
        'Œ' | 'œ' => "oe",
        'Ŕ'..='ř' => "r",
        'Ś'..='š' | 'ſ' => "s",
        'Ţ'..='ŧ' => "t",
        'Ũ'..='ų' => "u",
        'Ŵ' | 'ŵ' => "w",
        'Ŷ'..='Ÿ' => "y",
        'Ź'..='ž' => "z",
        _ => return None,
    })
}

/// Combining Diacritical Marks (U+0300..=U+036F): the accents of a
/// decomposed (NFD) letter.
fn is_combining_mark(c: char) -> bool {
    ('\u{300}'..='\u{36f}').contains(&c)
}

/// The ids a derived base is probed under, in order: `base`, then `base-2`,
/// `base-3`, and so on without end. A reserved base or device name is
/// skipped, so a title that slugifies to `index` lands on `index-2` and
/// one that slugifies to `con` on `con-2`. The suffixed probes are never
/// reserved when `base` has no `.`, as no [`slugify`] output does. No
/// candidate is longer than [`crate::layout::MAX_ID_LEN`]: where `base` and
/// its suffix would be, `base` is cut to fit, and a `-` the cut leaves at
/// the end is trimmed.
///
/// ```
/// use markdown_store::id::candidates;
/// let first: Vec<String> = candidates("draft").take(3).collect();
/// assert_eq!(first, ["draft", "draft-2", "draft-3"]);
/// assert_eq!(candidates("index").next().as_deref(), Some("index-2"));
/// assert_eq!(candidates("con").next().as_deref(), Some("con-2"));
/// ```
pub fn candidates(base: &str) -> impl Iterator<Item = String> {
    use crate::layout::{is_device_name, is_reserved_id};
    let max = crate::layout::MAX_ID_LEN;
    let base = base.to_string();
    let head = truncate_id(&base, max);
    let first = (!is_reserved_id(head) && !is_device_name(head)).then(|| head.to_string());
    first.into_iter().chain((2u64..).map(move |n| {
        let suffix = format!("-{n}");
        format!("{}{suffix}", truncate_id(&base, max - suffix.len()))
    }))
}

/// `s` cut to at most `max` bytes on a char boundary, less any `-` the cut
/// leaves at the end.
fn truncate_id(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s[..cut].trim_end_matches('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("v1.2 release notes"), "v1-2-release-notes");
        assert_eq!(slugify("Äpfel und Birnen"), "apfel-und-birnen");
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("a"), "a");
    }

    #[test]
    fn slugs_and_their_probes_are_creatable() {
        let slug = slugify(&"Grüße aus Köln ".repeat(30));
        assert_eq!(slug.len(), SLUG_MAX_LEN);
        assert!(slug.ends_with("-koln-grusse-aus-kol"), "{slug}");
        for candidate in candidates(&slug).take(12) {
            crate::layout::validate_id(&candidate).unwrap();
        }
        let long = "b".repeat(300);
        let probes: Vec<String> = candidates(&long).take(2).collect();
        assert_eq!(probes, ["b".repeat(200), format!("{}-2", "b".repeat(198))]);
        for title in ["Con", "NUL", "com1.backup", "Index"] {
            let slug = slugify(title);
            let first = candidates(&slug).next().unwrap();
            crate::layout::validate_id(&first).unwrap_or_else(|e| panic!("{title:?} -> {first:?}: {e}"));
        }
        assert_eq!(candidates(&slugify("Con")).next().as_deref(), Some("con-2"));
        assert_eq!(candidates(&slugify("com1.backup")).next().as_deref(), Some("com1-backup"));
    }

    #[test]
    fn provided_wins_under_every_strategy() {
        let strategies = [
            IdStrategy::Provided,
            IdStrategy::SlugFromField("title".into()),
            #[cfg(feature = "uuid")]
            IdStrategy::Uuid,
        ];
        for strategy in strategies {
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
