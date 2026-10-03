//! The OKF 0.2 artifacts a [`crate::VaultHandle`] writes beside records when
//! asked to: per-directory `index.md` files (§8) and the `generated`
//! provenance stamp (§5.2).
//!
//! Index rendering is a pure function of the directory's records, so a
//! rebuild of an unchanged vault is byte-identical and the store can skip
//! writing an index whose bytes would not change.

use std::{
    collections::BTreeMap,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    error::Error,
    frontmatter::{Document, TYPE_KEY},
    fsops,
    walk::{self, WalkOptions},
};

/// The file name the store writes a directory's index to.
pub(crate) const INDEX_FILE: &str = "index.md";

/// The frontmatter key the provenance stamp is written under.
pub(crate) const GENERATED_KEY: &str = "generated";

/// The bundle-root index's only frontmatter: the OKF version the vault
/// targets (§12).
const ROOT_FRONTMATTER: &str = "---\nokf_version: \"0.2\"\n---\n\n";

/// Records with no string `type` are listed under this heading.
const UNTYPED_HEADING: &str = "Untyped";

/// Subdirectories are listed under this heading, after every record section.
const DIRECTORIES_HEADING: &str = "Directories";

/// Appended to a type's heading when the type would otherwise read as one
/// of the store's own section headings (see [`type_heading`]).
const TYPE_SUFFIX: &str = " (type)";

/// The `generated: { by, at }` mapping for a write by `by` at `at`.
pub(crate) fn generated_stamp(by: &str, at: SystemTime) -> serde_norway::Value {
    let mut stamp = serde_norway::Mapping::with_capacity(2);
    stamp.insert("by".into(), by.into());
    stamp.insert("at".into(), format_utc(at).into());
    serde_norway::Value::Mapping(stamp)
}

/// Format an instant as ISO 8601 UTC with second precision and a `Z`
/// suffix (`2026-10-03T14:05:09Z`), the form OKF's examples use. Sub-second
/// digits are truncated toward the past.
pub(crate) fn format_utc(at: SystemTime) -> String {
    let secs: i64 = match at.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            if before.subsec_nanos() > 0 {
                -whole - 1
            } else {
                -whole
            }
        }
    };
    let days = secs.div_euclid(86_400);
    let of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", of_day / 3_600, of_day % 3_600 / 60, of_day % 60)
}

/// Proleptic Gregorian date of a count of days since 1970-01-01 (Howard
/// Hinnant's `civil_from_days`), so formatting needs no date crate.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Bring `dir/index.md` in line with the records at and below `dir`:
/// written when its bytes would change, left untouched otherwise. With no
/// record left to list, an index the store generated is removed and any
/// other `index.md` is left alone (see [`remove_if_generated`]).
pub(crate) fn sync_index(dir: &Path, is_root: bool, walk: &WalkOptions) -> Result<(), Error> {
    let path = dir.join(INDEX_FILE);
    match render_index(dir, is_root, walk)? {
        // An unreadable current file just reads as different: it is replaced.
        Some(wanted) if fsops::read_opt(&path).ok().flatten().as_deref() == Some(wanted.as_str()) => Ok(()),
        Some(wanted) => fsops::write_atomic(&path, &wanted),
        None => remove_if_generated(&path, walk),
    }
}

/// Remove `path` if it is an index the store generated, so a directory
/// that loses its last record loses its index, while a hand-written
/// `index.md` in a directory without records (an Obsidian folder note, an
/// attachments listing) is never deleted. A file that cannot be read is
/// not provably the store's and stays.
pub(crate) fn remove_if_generated(path: &Path, walk: &WalkOptions) -> Result<(), Error> {
    match fsops::read_opt(path) {
        Ok(Some(current)) if is_store_generated(&current, walk) => match fsops::remove(path) {
            Err(Error::NotFound { .. }) => Ok(()),
            other => other,
        },
        _ => Ok(()),
    }
}

/// Whether `src` has exactly the shape [`render_index`] emits: optionally
/// the root frontmatter (`okf_version: "0.2"` and nothing else), then one or
/// more sections, each a `# <heading>` line, a blank line and one or more
/// `* [<text>](<link>)` entries (optionally followed by ` - <description>`),
/// sections separated by a single blank line, ending in one newline. A link
/// must look like one the store writes: a percent-encoded file name with a
/// record extension from `walk`, or a percent-encoded directory name and
/// `/`. Anything else, an empty file included, is someone else's.
pub(crate) fn is_store_generated(src: &str, walk: &WalkOptions) -> bool {
    #[derive(Clone, Copy)]
    enum Next {
        Heading,
        Gap,
        Entry,
        EntryOrGap,
    }
    let body = src.strip_prefix(ROOT_FRONTMATTER).unwrap_or(src);
    let Some(body) = body.strip_suffix('\n') else { return false };
    let mut next = Next::Heading;
    for line in body.split('\n') {
        next = match next {
            Next::Heading if line.strip_prefix("# ").is_some_and(|h| !h.is_empty() && !h.starts_with(' ')) => Next::Gap,
            Next::Gap if line.is_empty() => Next::Entry,
            Next::Entry | Next::EntryOrGap if is_store_entry(line, walk) => Next::EntryOrGap,
            Next::EntryOrGap if line.is_empty() => Next::Heading,
            _ => return false,
        };
    }
    matches!(next, Next::EntryOrGap)
}

/// One `* [<text>](<link>)` or `* [<text>](<link>) - <description>` line
/// as the store writes it (see [`is_store_generated`]).
fn is_store_entry(line: &str, walk: &WalkOptions) -> bool {
    let Some(rest) = line.strip_prefix("* [") else { return false };
    let mut chars = rest.char_indices();
    let close = loop {
        match chars.next() {
            Some((_, '\\')) => {
                chars.next();
            }
            Some((i, ']')) => break i,
            Some((_, '[')) | None => return false,
            Some(_) => {}
        }
    };
    let Some(rest) = rest[close + 1..].strip_prefix('(') else { return false };
    let Some(end) = rest.find(')') else { return false };
    let (link, tail) = (&rest[..end], &rest[end + 1..]);
    let encoded = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~%".contains(&b));
    let link_ok = match link.strip_suffix('/') {
        Some(dir) => encoded(dir),
        None => {
            encoded(link)
                && link.rsplit_once('.').is_some_and(|(stem, ext)| {
                    !stem.is_empty() && walk.extensions.iter().any(|want| want.eq_ignore_ascii_case(ext))
                })
        }
    };
    close > 0 && link_ok && (tail.is_empty() || tail.strip_prefix(" - ").is_some_and(|d| !d.is_empty()))
}

/// The `index.md` for `dir`, or `None` when no record lives at or below it.
///
/// Lists the directory's own records, grouped by `type`, then the
/// subdirectories that hold records at any depth. Only the directory's own
/// records are read; a subdirectory is walked just until its first record,
/// so the root of a deep vault does not cost a walk of every file in it.
///
/// Records are parsed for their `type`, `title` and `description`; one that
/// cannot be read (not UTF-8, say) or whose frontmatter does not parse is
/// still listed, untyped and titled by its id, so a hand-broken file never
/// blocks a write elsewhere in the directory.
pub(crate) fn render_index(dir: &Path, is_root: bool, walk: &WalkOptions) -> Result<Option<String>, Error> {
    let (records, children) = walk::list_children(dir, walk)?;
    // A subdirectory's records sit one level deeper than `dir`'s, so the
    // walk's depth limit, which counts from `dir`, has one level less left.
    let below = walk.max_depth.map(|d| d.saturating_sub(1));
    let mut subdirs: Vec<&str> = Vec::new();
    for child in &children {
        if let Some(name) = child.file_name().and_then(|n| n.to_str()) {
            if walk::contains_record(child, walk, below)? {
                subdirs.push(name);
            }
        }
    }

    let mut typed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut untyped: Vec<String> = Vec::new();
    for path in &records {
        let (Some(file), Some(id)) =
            (path.file_name().and_then(|s| s.to_str()), path.file_stem().and_then(|s| s.to_str()))
        else {
            continue;
        };
        let doc = fsops::read(path).ok().and_then(|src| Document::parse(&src).ok());
        let field = |key: &str| doc.as_ref().and_then(|d| d.get(key)).and_then(|v| v.as_str()).map(collapse_whitespace);
        let mut entry = format!(
            "* [{}]({})",
            escape_markdown(&field("title").filter(|t| !t.is_empty()).unwrap_or_else(|| id.to_string())),
            encode_url(file)
        );
        if let Some(description) = field("description").filter(|d| !d.is_empty()) {
            entry.push_str(" - ");
            entry.push_str(&escape_markdown(&description));
        }
        match field(TYPE_KEY).filter(|t| !t.is_empty()) {
            Some(type_name) => typed.entry(type_name).or_default().push(entry),
            None => untyped.push(entry),
        }
    }

    let mut sections: Vec<String> =
        typed.iter().map(|(type_name, entries)| section(&escape_markdown(&type_heading(type_name)), entries)).collect();
    if !untyped.is_empty() {
        sections.push(section(UNTYPED_HEADING, &untyped));
    }
    if !subdirs.is_empty() {
        let entries: Vec<String> =
            subdirs.iter().map(|name| format!("* [{}]({}/)", escape_markdown(name), encode_url(name))).collect();
        sections.push(section(DIRECTORIES_HEADING, &entries));
    }

    if sections.is_empty() {
        return Ok(None);
    }
    let mut out = String::new();
    if is_root {
        out.push_str(ROOT_FRONTMATTER);
    }
    out.push_str(&sections.join("\n"));
    Ok(Some(out))
}

fn section(heading: &str, entries: &[String]) -> String {
    let mut out = format!("# {heading}\n\n");
    for entry in entries {
        out.push_str(entry);
        out.push('\n');
    }
    out
}

/// An index entry is one line, so every run of whitespace (newlines
/// included) becomes a single space.
fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The heading of a type's section: the type itself, unless it would read
/// as one of the store's own sections (`Untyped`, `Directories`). Such a
/// type gets ` (type)` appended, and so does a type that already reads as
/// one of those plus any number of ` (type)` suffixes, so no two sections
/// of an index ever share a heading.
fn type_heading(type_name: &str) -> String {
    let mut stem = type_name;
    while let Some(shorter) = stem.strip_suffix(TYPE_SUFFIX) {
        stem = shorter;
    }
    if stem == UNTYPED_HEADING || stem == DIRECTORIES_HEADING {
        format!("{type_name}{TYPE_SUFFIX}")
    } else {
        type_name.to_string()
    }
}

/// Backslash-escape the characters that could turn index text into markup:
/// `` \ ` * _ [ ] < & # ~ $ % = ^ ``. Titles, descriptions and types are
/// arbitrary text, and agents read an index raw, so everything else stays
/// as written. Every piece of text sits mid-line (after `* [`, ` - ` or
/// `# `), so nothing that matters only at the start of a line (`-`, `+`,
/// `>`, `1.`) needs escaping. The set covers CommonMark's code spans,
/// emphasis, link brackets, raw HTML and autolinks (`<!--` would swallow
/// the rest of the file), entities, a heading's closing `#`s and the
/// backslash itself, plus what Obsidian adds: `#tag`s, `~~` strikethrough,
/// `$` math, `%%` comments, `==` highlights and `^` block references.
fn escape_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '&' | '#' | '~' | '$' | '%' | '=' | '^') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Percent-encode a path segment for a link destination. Only RFC 3986
/// unreserved characters pass through: a space or parenthesis would end a
/// CommonMark destination, and a `:` in the first segment would read as a
/// URL scheme.
fn encode_url(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn utc_instants_format_as_iso_8601_with_z() {
        assert_eq!(format_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(at(1_791_036_309)), "2026-10-03T14:05:09Z");
        assert_eq!(format_utc(at(951_782_400)), "2000-02-29T00:00:00Z", "leap day");
        assert_eq!(format_utc(at(4_107_542_399)), "2100-02-28T23:59:59Z", "2100 is not a leap year");
        assert_eq!(format_utc(at(1_791_036_309) + Duration::from_millis(999)), "2026-10-03T14:05:09Z");
        assert_eq!(format_utc(UNIX_EPOCH - Duration::from_millis(1)), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn text_escapes_only_markup_and_urls_encode_what_commonmark_would_misread() {
        assert_eq!(escape_markdown(r"a [b] \c"), r"a \[b\] \\c");
        assert_eq!(
            escape_markdown("<!-- *x* _y_ `z` # & ~~s~~ $m$ %%c%% ==h== ^ref"),
            r"\<!-- \*x\* \_y\_ \`z\` \# \& \~\~s\~\~ \$m\$ \%\%c\%\% \=\=h\=\= \^ref"
        );
        assert_eq!(
            escape_markdown("c-sweep: what's next? (draft). 1+1! | @a/b, \"q\"; {x} >"),
            "c-sweep: what's next? (draft). 1+1! | @a/b, \"q\"; {x} >",
            "punctuation that is no markup mid-line is written as is"
        );
        assert_eq!(escape_markdown("café ünïcode"), "café ünïcode");
        assert_eq!(encode_url("plain-id_1.2~x.md"), "plain-id_1.2~x.md");
        assert_eq!(encode_url("my note (draft).md"), "my%20note%20%28draft%29.md");
        assert_eq!(encode_url("a:b%.md"), "a%3Ab%25.md");
        assert_eq!(encode_url("é.md"), "%C3%A9.md");
    }

    #[test]
    fn only_the_shape_the_store_writes_counts_as_store_generated() {
        let walk = WalkOptions::default();
        for ours in [
            "# Note\n\n* [A](a.md)\n",
            "---\nokf_version: \"0.2\"\n---\n\n# Directories\n\n* [notes](notes/)\n",
            "# Note\n\n* [A \\] b](a.md) - why\n* [B](my%20b.markdown)\n\n# Untyped\n\n* [c](c.MD)\n",
        ] {
            assert!(is_store_generated(ours, &walk), "{ours:?}");
        }
        for theirs in [
            "",
            "# Note\n",
            "# Note\n\n* [A](a.md)",
            "# Note\n\n* [A](a.md)\n\n",
            "# Note\n* [A](a.md)\n",
            "# Note\n\n* [A](a.md)\n\n\n# More\n\n* [B](b.md)\n",
            "# Note\n\nA folder note.\n",
            "## Note\n\n* [A](a.md)\n",
            "# Note\n\n- [A](a.md)\n",
            "# Note\n\n* [A](a.md) trailing\n",
            "# Note\n\n* [A](a.md) - \n",
            "# Files\n\n* [Diagram](diagram.png)\n",
            "# Note\n\n* [A](../a.md)\n",
            "# Note\n\n* [A](<a b.md>)\n",
            "# Note\n\n* [](a.md)\n",
            "---\nokf_version: \"0.2\"\nauthor: me\n---\n\n# Note\n\n* [A](a.md)\n",
            "---\ntitle: x\n---\n# Note\n\n* [A](a.md)\n",
        ] {
            assert!(!is_store_generated(theirs, &walk), "{theirs:?}");
        }
    }

    #[test]
    fn a_type_never_takes_a_heading_of_the_stores_own() {
        assert_eq!(type_heading("Note"), "Note");
        assert_eq!(type_heading("Untyped"), "Untyped (type)");
        assert_eq!(type_heading("Directories"), "Directories (type)");
        assert_eq!(type_heading("Untyped (type)"), "Untyped (type) (type)", "the suffixed form stays distinct too");
        assert_eq!(type_heading("untyped"), "untyped", "headings compare case-sensitively");
        assert_eq!(type_heading("Note (type)"), "Note (type)");
    }

    #[test]
    fn whitespace_collapses_to_single_spaces() {
        assert_eq!(collapse_whitespace("  one\n two\t\tthree \n"), "one two three");
    }
}
