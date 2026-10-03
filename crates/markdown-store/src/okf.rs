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
/// written when its bytes would change, removed when no record remains,
/// left untouched otherwise.
pub(crate) fn sync_index(dir: &Path, is_root: bool, walk: &WalkOptions) -> Result<(), Error> {
    let path = dir.join(INDEX_FILE);
    let wanted = render_index(dir, is_root, walk)?;
    let current = fsops::read_opt(&path)?;
    match (wanted, current) {
        (Some(wanted), Some(current)) if wanted == current => Ok(()),
        (Some(wanted), _) => fsops::write_atomic(&path, &wanted),
        (None, Some(_)) => match fsops::remove(&path) {
            Err(Error::NotFound { .. }) => Ok(()),
            other => other,
        },
        (None, None) => Ok(()),
    }
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
            escape_link_text(&field("title").filter(|t| !t.is_empty()).unwrap_or_else(|| id.to_string())),
            encode_url(file)
        );
        if let Some(description) = field("description").filter(|d| !d.is_empty()) {
            entry.push_str(" - ");
            entry.push_str(&description);
        }
        match field(TYPE_KEY).filter(|t| !t.is_empty()) {
            Some(type_name) => typed.entry(type_name).or_default().push(entry),
            None => untyped.push(entry),
        }
    }

    let mut sections: Vec<String> = typed.iter().map(|(heading, entries)| section(heading, entries)).collect();
    if !untyped.is_empty() {
        sections.push(section(UNTYPED_HEADING, &untyped));
    }
    if !subdirs.is_empty() {
        let entries: Vec<String> =
            subdirs.iter().map(|name| format!("* [{}]({}/)", escape_link_text(name), encode_url(name))).collect();
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

/// Backslash-escape what would end or nest the link text early.
fn escape_link_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '[' | ']') {
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
    fn link_text_escapes_brackets_and_urls_encode_what_commonmark_would_misread() {
        assert_eq!(escape_link_text(r"a [b] \c"), r"a \[b\] \\c");
        assert_eq!(encode_url("plain-id_1.2~x.md"), "plain-id_1.2~x.md");
        assert_eq!(encode_url("my note (draft).md"), "my%20note%20%28draft%29.md");
        assert_eq!(encode_url("a:b%.md"), "a%3Ab%25.md");
        assert_eq!(encode_url("é.md"), "%C3%A9.md");
    }

    #[test]
    fn whitespace_collapses_to_single_spaces() {
        assert_eq!(collapse_whitespace("  one\n two\t\tthree \n"), "one two three");
    }
}
