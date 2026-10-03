//! The record-id rule both store backends enforce on create, and the slug
//! function derived ids come from.
//!
//! Generated SeaORM store code calls these at runtime. The markdown backend
//! enforces the same rule through `markdown_store::layout::validate_id`,
//! because that crate takes no ontogen dependency; a root-workspace test
//! (`tests/id_rule_parity.rs`) keeps the two in agreement. The rule is
//! the JSON:API wire contract's §8.2 validity rule. Lookups are never
//! checked against it (§8.1), so records stored under an older, looser rule
//! stay readable.

use std::fmt;

/// Ids OKF reserves at every level of a bundle: `index.md` is a directory
/// listing and `log.md` an update history. Held to on both backends so a
/// SeaORM store can move to a markdown vault without renaming records.
pub const RESERVED_IDS: &[&str] = &["index", "log"];

/// The longest id [`validate_id`] accepts, in bytes. With `.md` appended
/// it stays under the 255-byte filename limit of common filesystems.
pub const MAX_ID_LEN: usize = 200;

/// The longest id [`slugify`] returns, in bytes. The 10 bytes it leaves
/// under [`MAX_ID_LEN`] hold a `-N` probe suffix for every N below one
/// billion, so every probe of a slug keeps the slug's whole text.
pub const SLUG_MAX_LEN: usize = 190;

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
/// Valid: 1 to [`MAX_ID_LEN`] bytes of lowercase ASCII letters, digits,
/// `.`, `_`, `~` and `-`, not starting or ending with `.`, and not a
/// reserved id (see [`is_reserved_id`]). Every such id is a portable
/// filename stem and a URL path segment that needs no escaping.
///
/// ```
/// use ontogen_core::id::validate_id;
/// assert!(validate_id("ship-the-parser").is_ok());
/// assert!(validate_id("v1.2_notes~draft").is_ok());
/// assert!(validate_id("Ship").is_err());
/// assert!(validate_id("café").is_err());
/// assert!(validate_id("index").is_err());
/// assert!(validate_id("a/b").is_err());
/// ```
pub fn validate_id(id: &str) -> Result<(), InvalidId> {
    let reject = |reason: &'static str| Err(InvalidId { id: id.to_string(), reason });
    if id.is_empty() {
        return reject("must not be empty");
    }
    if id.len() > MAX_ID_LEN {
        return reject("must be at most 200 bytes");
    }
    if is_reserved_id(id) {
        return reject("is reserved: OKF uses index and log for directory listings and update logs");
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
    if !id.bytes().all(is_id_byte) {
        return reject("may contain only a-z, 0-9, '.', '_', '~' and '-'");
    }
    Ok(())
}

fn is_id_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'~' | b'-')
}

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
/// use ontogen_core::id::slugify;
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
/// `base-3`, and so on without end. A reserved base is skipped, so a title
/// that slugifies to `index` lands on `index-2`. No candidate is longer
/// than [`MAX_ID_LEN`]: where `base` and its suffix would be, `base` is cut
/// to fit, and a `-` the cut leaves at the end is trimmed.
///
/// ```
/// use ontogen_core::id::candidates;
/// let first: Vec<String> = candidates("draft").take(3).collect();
/// assert_eq!(first, ["draft", "draft-2", "draft-3"]);
/// assert_eq!(candidates("index").next().as_deref(), Some("index-2"));
/// ```
pub fn candidates(base: &str) -> impl Iterator<Item = String> + use<> {
    let base = base.to_string();
    let head = truncate_id(&base, MAX_ID_LEN);
    let first = (!is_reserved_id(head)).then(|| head.to_string());
    first.into_iter().chain((2u64..).map(move |n| {
        let suffix = format!("-{n}");
        format!("{}{suffix}", truncate_id(&base, MAX_ID_LEN - suffix.len()))
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
        let longest = "a".repeat(MAX_ID_LEN);
        for ok in ["a", "0", "ship-the-parser", "v1.2-notes", "a_b", "a~b", "index-2", "logs", "-", "~", &longest] {
            assert!(validate_id(ok).is_ok(), "{ok:?} must be accepted");
        }
    }

    #[test]
    fn rejects_what_the_rule_rejects() {
        let too_long = "a".repeat(MAX_ID_LEN + 1);
        for bad in [
            "", " ", "\t", "\n ", ".", "..", ".hidden", "dotted.", "spaced ", "my index", "a/b", "a\\b", "c:x", "x\0y",
            "é", "B", "Zeta", "a+b", "a%20b", &too_long,
        ] {
            assert!(validate_id(bad).is_err(), "{bad:?} must be rejected");
        }
        for reserved in ["index", "log", "Index", "LOG", "lOg"] {
            assert!(validate_id(reserved).is_err(), "{reserved:?} is reserved");
        }
    }

    #[test]
    fn the_error_names_the_id_and_the_rule_it_broke() {
        assert_eq!(validate_id("Ab").unwrap_err().to_string(), "invalid id \"Ab\": must not contain uppercase letters");
        let reason = |id: &str| validate_id(id).unwrap_err().reason;
        assert_eq!(reason(""), "must not be empty");
        assert_eq!(reason(&"a".repeat(MAX_ID_LEN + 1)), "must be at most 200 bytes");
        assert!(reason("log").starts_with("is reserved"));
        assert_eq!(reason(".."), "must not start with '.'");
        assert_eq!(reason("a."), "must not end with '.'");
        assert_eq!(reason("café"), "must be ASCII");
        assert_eq!(reason("a b"), "may contain only a-z, 0-9, '.', '_', '~' and '-'");
        assert_eq!(reason("a/b"), "may contain only a-z, 0-9, '.', '_', '~' and '-'");
    }

    #[test]
    fn candidates_skip_a_reserved_base() {
        assert_eq!(candidates("log").take(2).collect::<Vec<_>>(), ["log-2", "log-3"]);
        assert_eq!(candidates("a").nth(9).as_deref(), Some("a-10"));
    }

    #[test]
    fn candidates_never_exceed_the_id_limit() {
        let slug = slugify(&"word ".repeat(100));
        assert_eq!(slug.len(), SLUG_MAX_LEN - 1, "cut at 190, then the dangling '-' trimmed: {slug}");
        let probes: Vec<String> = candidates(&slug).take(3).collect();
        assert_eq!(probes, [slug.clone(), format!("{slug}-2"), format!("{slug}-3")]);

        let long = "b".repeat(MAX_ID_LEN + 50);
        let mut probes = candidates(&long);
        assert_eq!(probes.next(), Some("b".repeat(MAX_ID_LEN)));
        assert_eq!(probes.next(), Some(format!("{}-2", "b".repeat(MAX_ID_LEN - 2))));
        assert_eq!(probes.nth(97), Some(format!("{}-100", "b".repeat(MAX_ID_LEN - 4))));

        let dashed = format!("{}-{}", "c".repeat(197), "d".repeat(10));
        assert_eq!(candidates(&dashed).nth(1), Some(format!("{}-2", "c".repeat(197))), "a dangling '-' is trimmed");
        for candidate in candidates(&dashed).take(20).chain(candidates(&long).skip(1_000).take(5)) {
            assert!(validate_id(&candidate).is_ok(), "{candidate:?}");
        }
    }

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("v1.2 release notes"), "v1-2-release-notes");
        assert_eq!(slugify("Äpfel und Birnen"), "apfel-und-birnen");
        assert_eq!(slugify("snake_case~tilde"), "snake-case-tilde");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_folds_latin_letters_and_separates_the_rest() {
        assert_eq!(slugify("Ærøskøbing Œuvre"), "aeroskobing-oeuvre");
        assert_eq!(slugify("Þór ðe Ĳssel"), "thor-de-ijssel");
        assert_eq!(slugify("ÀÉÎÕÜ àéîõü Ÿÿ ſ"), "aeiou-aeiou-yy-s");
        assert_eq!(slugify("cafe\u{301}"), "cafe", "a combining accent is dropped, not a separator");
        assert_eq!(slugify("re\u{301}sume\u{301}"), "resume");
        assert_eq!(slugify("2×3÷4"), "2-3-4");
        assert_eq!(slugify("日本 Ω tea"), "tea");
        assert_eq!(slugify("Ωmega"), "mega");
    }

    #[test]
    fn fold_latin_covers_both_blocks() {
        for c in ('\u{c0}'..='\u{17f}').filter(|c| c.is_alphabetic()) {
            let folded = fold_latin(c).unwrap_or_else(|| panic!("{c:?} (U+{:04X}) has no fold", c as u32));
            assert!(folded.bytes().all(|b| b.is_ascii_lowercase()), "{c:?} folds to {folded:?}");
        }
        for c in ['×', '÷', 'ª', 'º', 'µ', 'Ω', 'ƒ'] {
            assert_eq!(fold_latin(c), None, "{c:?}");
        }
    }

    #[test]
    fn slugify_cuts_long_input_to_the_slug_limit() {
        assert_eq!(slugify(&"x".repeat(500)), "x".repeat(SLUG_MAX_LEN));
        let cut_on_hyphen = format!("{}-tail", "y".repeat(SLUG_MAX_LEN - 1));
        assert_eq!(slugify(&cut_on_hyphen), "y".repeat(SLUG_MAX_LEN - 1));
        assert_eq!(slugify(&"ß".repeat(200)), "s".repeat(SLUG_MAX_LEN));
    }

    #[test]
    fn every_nonempty_slug_is_valid_or_reserved() {
        let long = "Long title ".repeat(40);
        for title in ["Index", "LOG", "Hello", &long, "Ça va?", "...dots..."] {
            let slug = slugify(title);
            assert!(slug.is_empty() || is_reserved_id(&slug) || validate_id(&slug).is_ok(), "{title:?} -> {slug:?}");
        }
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
