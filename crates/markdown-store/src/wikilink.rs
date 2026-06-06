//! Obsidian-style wikilink encoding, stripping, and parsing.
//!
//! In a markdown store, relation fields carry their foreign ids as wikilinks
//! (`epic: "[[E0042]]"`), which makes the same files render as a navigable
//! graph in Obsidian. The store's typed boundary encodes ids into wikilinks
//! on write and strips them back to plain ids on read; these are the
//! primitives that boundary is generated against.
//!
//! `strip` is deliberately forgiving: a plain id passes through unchanged,
//! so data that was never wikilink-wrapped (or already stripped) is safe to
//! strip again. Idempotence is what lets generated `From<CreateInput>` code
//! strip unconditionally.

/// Wrap an id in wikilink syntax: `foo` → `[[foo]]`.
///
/// Returns the bare wikilink without YAML quoting — quoting is the YAML
/// emitter's job (a `[[…]]` string is always quoted on render because it
/// would otherwise parse as a nested sequence).
///
/// ```
/// assert_eq!(markdown_store::wikilink::encode("E0042"), "[[E0042]]");
/// ```
pub fn encode(id: &str) -> String {
    format!("[[{id}]]")
}

/// Strip wikilink syntax down to the link target: `[[foo]]` → `foo`,
/// `[[foo|Alias]]` → `foo`, `[[foo#heading]]` → `foo`. Anything that isn't
/// a single well-formed wikilink — including a plain id — passes through
/// unchanged. Idempotent.
///
/// ```
/// use markdown_store::wikilink::strip;
/// assert_eq!(strip("[[task-0001]]"), "task-0001");
/// assert_eq!(strip("[[task-0001|My Task]]"), "task-0001");
/// assert_eq!(strip("task-0001"), "task-0001");
/// assert_eq!(strip(strip("[[x]]").as_str()), "x");
/// // Two adjacent links are NOT one link; left alone.
/// assert_eq!(strip("[[a]] [[b]]"), "[[a]] [[b]]");
/// ```
pub fn strip(s: &str) -> String {
    match parse(s) {
        Some(link) => link.target,
        None => s.to_string(),
    }
}

/// [`strip`] lifted over `Option<String>`.
pub fn strip_opt(value: Option<String>) -> Option<String> {
    value.map(|s| strip(&s))
}

/// [`strip`] applied to every element of a `Vec<String>`.
pub fn strip_vec(values: Vec<String>) -> Vec<String> {
    values.into_iter().map(|s| strip(&s)).collect()
}

/// A parsed wikilink: `[[target#anchor|alias]]` (anchor and alias optional,
/// in Obsidian's order — anchor before alias).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// The link target — for store records, the record id.
    pub target: String,
    /// Display alias, from `[[target|alias]]`.
    pub alias: Option<String>,
    /// Heading/block anchor, from `[[target#anchor]]`.
    pub anchor: Option<String>,
}

/// Parse a single well-formed wikilink. Returns `None` for anything else:
/// plain strings, empty targets, or text containing multiple links.
///
/// ```
/// use markdown_store::wikilink::parse;
/// let link = parse("[[notes/intro#setup|Getting started]]").unwrap();
/// assert_eq!(link.target, "notes/intro");
/// assert_eq!(link.anchor.as_deref(), Some("setup"));
/// assert_eq!(link.alias.as_deref(), Some("Getting started"));
/// assert!(parse("not a link").is_none());
/// ```
pub fn parse(s: &str) -> Option<WikiLink> {
    let trimmed = s.trim();
    let inner = trimmed.strip_prefix("[[")?.strip_suffix("]]")?;
    // Reject nested/multiple links masquerading as one ("[[a]] [[b]]").
    if inner.contains("[[") || inner.contains("]]") {
        return None;
    }
    let (before_alias, alias) = match inner.split_once('|') {
        Some((left, alias)) => (left, Some(alias.trim().to_string()).filter(|a| !a.is_empty())),
        None => (inner, None),
    };
    let (target, anchor) = match before_alias.split_once('#') {
        Some((left, anchor)) => (left, Some(anchor.trim().to_string()).filter(|a| !a.is_empty())),
        None => (before_alias, None),
    };
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    Some(WikiLink { target: target.to_string(), alias, anchor })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_strip_roundtrip() {
        for id in ["a", "task-0001", "notes/deep/idea", "E0042"] {
            assert_eq!(strip(&encode(id)), id);
        }
    }

    #[test]
    fn strip_is_idempotent() {
        for s in ["[[x]]", "x", "[[x|Alias]]", "[[a]] [[b]]", ""] {
            assert_eq!(strip(&strip(s)), strip(s));
        }
    }

    #[test]
    fn strip_passthrough_cases() {
        assert_eq!(strip(""), "");
        assert_eq!(strip("[["), "[[");
        assert_eq!(strip("]]"), "]]");
        assert_eq!(strip("[[]]"), "[[]]"); // empty target is not a link
        assert_eq!(strip("[ [x] ]"), "[ [x] ]");
    }

    #[test]
    fn strip_opt_and_vec() {
        assert_eq!(strip_opt(Some("[[a]]".into())), Some("a".into()));
        assert_eq!(strip_opt(None), None);
        assert_eq!(strip_vec(vec!["[[a]]".into(), "b".into()]), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn parse_variants() {
        assert_eq!(parse("[[t]]").unwrap(), WikiLink { target: "t".into(), alias: None, anchor: None });
        assert_eq!(
            parse("[[t|Alias]]").unwrap(),
            WikiLink { target: "t".into(), alias: Some("Alias".into()), anchor: None }
        );
        assert_eq!(parse("[[t#h]]").unwrap(), WikiLink { target: "t".into(), alias: None, anchor: Some("h".into()) });
        assert_eq!(
            parse("  [[t#h|A]]  ").unwrap(),
            WikiLink { target: "t".into(), alias: Some("A".into()), anchor: Some("h".into()) }
        );
        assert!(parse("[[a]] trailing").is_none());
        assert!(parse("[[a]][[b]]").is_none());
        assert!(parse("[[#anchor-only]]").is_none());
    }
}
