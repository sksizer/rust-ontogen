//! YAML-frontmatter document model with a lossless round-trip.
//!
//! The centerpiece is [`Document`]: a parsed markdown file held as an
//! order-preserving YAML mapping plus the verbatim body. Typed access goes
//! through serde ([`Document::deserialize`] / [`Document::merge_serialize`]),
//! but the mapping itself is retained — so a read-modify-write cycle never
//! drops keys a human added by hand and never reorders the keys it didn't
//! touch. That property is what makes it safe to point a store at files
//! people also edit in a text editor or Obsidian.
//!
//! The fence-splitting contract ([`split`]) is byte-compatible with
//! `markdown-vault`'s (crate-private) `frontmatter::split`, the read-only
//! sibling of this crate; when the two crates eventually share a workspace
//! the duplicate collapses. Keep the two in lockstep — the edge-case tests
//! below mirror markdown-vault's suite case for case.
//!
//! One deliberate divergence from markdown-vault: malformed YAML is an
//! **error** here, not an empty result. Tag extraction can shrug off a bad
//! frontmatter block; a store doing read-modify-write must not, or it would
//! rewrite the file and destroy whatever it failed to parse.

use serde::{de::DeserializeOwned, Serialize};

use crate::error::Error;

/// Split `src` into `(frontmatter_yaml, body)`.
///
/// Frontmatter is recognized only when the document begins with `---\n` (or
/// `---\r\n`) and is terminated by a matching `---` on a line of its own
/// (end-of-input, `\n`, or `\r\n`). Anything else — including a missing
/// terminator — is treated as having no frontmatter, and the entire input is
/// returned as the body.
///
/// Byte-compatible with `markdown-vault`'s split: under CRLF input the
/// trailing `\r` of each YAML line stays attached to the YAML slice, and the
/// body starts after the terminator's line ending.
///
/// ```
/// let (yaml, body) = markdown_store::frontmatter::split("---\ntitle: hi\n---\nbody\n");
/// assert_eq!(yaml, Some("title: hi"));
/// assert_eq!(body, "body\n");
///
/// let (yaml, body) = markdown_store::frontmatter::split("no fence here");
/// assert_eq!(yaml, None);
/// assert_eq!(body, "no fence here");
/// ```
pub fn split(src: &str) -> (Option<&str>, &str) {
    let after_marker = if let Some(rest) = src.strip_prefix("---\n") {
        rest
    } else if let Some(rest) = src.strip_prefix("---\r\n") {
        rest
    } else {
        return (None, src);
    };

    // Empty frontmatter: closing fence sits directly after the opener.
    if let Some(rest) = after_marker.strip_prefix("---\n") {
        return (Some(""), rest);
    }
    if let Some(rest) = after_marker.strip_prefix("---\r\n") {
        return (Some(""), rest);
    }
    if after_marker == "---" {
        return (Some(""), "");
    }

    let bytes = after_marker.as_bytes();
    let mut search_start = 0;
    loop {
        let Some(found) = after_marker[search_start..].find("\n---") else {
            return (None, src);
        };
        let dashes_start = search_start + found + 1; // index of the leading '-'
        let after_dashes = dashes_start + 3;
        // The closing `---` must sit on a line by itself: end-of-input, `\n`, or `\r\n`.
        match bytes.get(after_dashes).copied() {
            None => {
                let yaml = &after_marker[..dashes_start - 1]; // exclude the leading '\n'
                return (Some(yaml), "");
            }
            Some(b'\n') => {
                let yaml = &after_marker[..dashes_start - 1];
                let body_start = after_dashes + 1;
                return (Some(yaml), &after_marker[body_start..]);
            }
            Some(b'\r') if bytes.get(after_dashes + 1) == Some(&b'\n') => {
                let yaml = &after_marker[..dashes_start - 1];
                let body_start = after_dashes + 2;
                return (Some(yaml), &after_marker[body_start..]);
            }
            _ => {
                // `\n---foo` — not a real terminator, keep looking.
                search_start = after_dashes;
            }
        }
    }
}

/// A markdown document as a store record: an order-preserving frontmatter
/// mapping plus the verbatim body.
///
/// The mapping holds *every* frontmatter key found in the source, including
/// ones no typed view models. [`merge_serialize`](Document::merge_serialize)
/// folds a typed value back in without disturbing the rest, which is what
/// keeps hand-edited vaults safe under generated CRUD code.
///
/// ```
/// use markdown_store::Document;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct Task {
///     title: String,
///     done: bool,
/// }
///
/// let src = "---\ntitle: write docs\ndone: false\npriority: high\n---\nSome body.\n";
/// let mut doc = Document::parse(src)?;
///
/// // Typed read of the keys Task models...
/// let mut task: Task = doc.deserialize()?;
/// task.done = true;
///
/// // ...typed write back. `priority` was added by hand and survives.
/// doc.merge_serialize(&task, &["title", "done"])?;
/// let out = doc.render()?;
/// assert!(out.contains("done: true"));
/// assert!(out.contains("priority: high"));
/// assert!(out.ends_with("---\nSome body.\n"));
/// # Ok::<(), markdown_store::Error>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    fm: serde_norway::Mapping,
    body: String,
    had_frontmatter: bool,
}

impl Document {
    /// An empty document: no frontmatter keys, empty body.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a markdown source into frontmatter mapping + body.
    ///
    /// - No opening fence ⇒ empty mapping, the whole input is the body.
    /// - Empty fence block ⇒ empty mapping, [`had_frontmatter`](Self::had_frontmatter) is `true`.
    /// - A fence block that is not valid YAML, or whose top level is not a
    ///   mapping, is an [`Error::Parse`] — never silently dropped, because a
    ///   later render would destroy it.
    pub fn parse(src: &str) -> Result<Self, Error> {
        let (yaml, body) = split(src);
        match yaml {
            None => Ok(Self { fm: serde_norway::Mapping::new(), body: body.to_string(), had_frontmatter: false }),
            Some(yaml) if yaml.trim().is_empty() => {
                Ok(Self { fm: serde_norway::Mapping::new(), body: body.to_string(), had_frontmatter: true })
            }
            Some(yaml) => {
                let value: serde_norway::Value =
                    serde_norway::from_str(yaml).map_err(|e| Error::Parse { message: e.to_string() })?;
                let fm = match value {
                    serde_norway::Value::Mapping(m) => m,
                    serde_norway::Value::Null => serde_norway::Mapping::new(),
                    other => {
                        return Err(Error::Parse {
                            message: format!("frontmatter must be a YAML mapping, found {}", yaml_kind(&other)),
                        });
                    }
                };
                Ok(Self { fm, body: body.to_string(), had_frontmatter: true })
            }
        }
    }

    /// The body text (everything after the closing fence), verbatim.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Replace the body.
    pub fn set_body(&mut self, body: impl Into<String>) {
        self.body = body.into();
    }

    /// Whether the parsed source had a frontmatter block at all. A document
    /// that had one keeps its (possibly empty) fence block on render.
    pub fn had_frontmatter(&self) -> bool {
        self.had_frontmatter
    }

    /// Borrow the raw frontmatter mapping.
    pub fn mapping(&self) -> &serde_norway::Mapping {
        &self.fm
    }

    /// Mutably borrow the raw frontmatter mapping — the escape hatch for
    /// manipulation the typed API doesn't cover.
    pub fn mapping_mut(&mut self) -> &mut serde_norway::Mapping {
        &mut self.fm
    }

    /// Look up a single frontmatter value by string key.
    pub fn get(&self, key: &str) -> Option<&serde_norway::Value> {
        self.fm.get(key)
    }

    /// Insert or overwrite a single frontmatter value. Existing keys keep
    /// their position; new keys append.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<serde_norway::Value>) {
        self.fm.insert(serde_norway::Value::String(key.into()), value.into());
    }

    /// Remove a key, preserving the order of the remaining keys. Returns the
    /// removed value, if any.
    pub fn remove(&mut self, key: &str) -> Option<serde_norway::Value> {
        self.fm.shift_remove(serde_norway::Value::String(key.to_string()))
    }

    /// Deserialize the frontmatter mapping into a typed value. Keys the type
    /// doesn't model are ignored here but retained in the document.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, Error> {
        serde_norway::from_value(serde_norway::Value::Mapping(self.fm.clone()))
            .map_err(|e| Error::Parse { message: e.to_string() })
    }

    /// Serialize `value` into the frontmatter mapping, owning exactly
    /// `owned_keys`.
    ///
    /// Semantics: every key in `owned_keys` that `value` did **not** emit is
    /// removed (so a field cleared to `None` under a
    /// `skip_serializing_if`-style policy doesn't leave its stale key
    /// behind); every key `value` did emit is inserted, overwriting in place
    /// (existing keys keep their position; new keys append). Keys outside
    /// `owned_keys` — hand-added extras — are untouched.
    ///
    /// **`owned_keys` must list every key the type can emit *or skip*.**
    /// Generated code passes its statically-known field set. Passing a
    /// shorter list silently breaks the cleared-`Option` removal guarantee:
    /// a field that skip-serializes to nothing leaves its stale key on disk
    /// if it isn't owned. There is no safe shortcut here — when in doubt,
    /// list every field.
    pub fn merge_serialize<T: Serialize>(&mut self, value: &T, owned_keys: &[&str]) -> Result<(), Error> {
        let serialized = serde_norway::to_value(value).map_err(|e| Error::Serialize { message: e.to_string() })?;
        let serde_norway::Value::Mapping(new_map) = serialized else {
            return Err(Error::Serialize {
                message: format!("value serialized to {}, expected a YAML mapping", yaml_kind(&serialized)),
            });
        };
        for key in owned_keys {
            let k = serde_norway::Value::String((*key).to_string());
            if !new_map.contains_key(&k) {
                self.fm.shift_remove(&k);
            }
        }
        for (k, v) in new_map {
            self.fm.insert(k, v);
        }
        Ok(())
    }

    /// Render the document back to a full markdown string.
    ///
    /// - Non-empty mapping ⇒ `---\n<yaml>---\n<body>`.
    /// - Empty mapping that *had* a fence block ⇒ `---\n---\n<body>` (the
    ///   block is kept so round-tripping an empty-frontmatter file is
    ///   stable).
    /// - Empty mapping, never had one ⇒ the body alone.
    ///
    /// The YAML rendering is serde_norway's emitter output: deterministic and
    /// Obsidian-readable, but not guaranteed byte-identical to arbitrary
    /// hand-authored input (quoting style, list layout). Round-trip
    /// *meaning* is preserved; cosmetic normalization can occur on rewrite.
    pub fn render(&self) -> Result<String, Error> {
        if self.fm.is_empty() {
            if self.had_frontmatter {
                return Ok(format!("---\n---\n{}", self.body));
            }
            return Ok(self.body.clone());
        }
        let mut yaml = serde_norway::to_string(&self.fm).map_err(|e| Error::Serialize { message: e.to_string() })?;
        if !yaml.ends_with('\n') {
            yaml.push('\n');
        }
        Ok(format!("---\n{yaml}---\n{}", self.body))
    }
}

/// One-shot typed parse: `(value, body)`. For callers that don't need
/// unknown-key preservation.
///
/// ```
/// #[derive(serde::Deserialize)]
/// struct Note { title: String }
///
/// let (note, body) = markdown_store::frontmatter::from_str::<Note>("---\ntitle: hi\n---\nText.\n")?;
/// assert_eq!(note.title, "hi");
/// assert_eq!(body, "Text.\n");
/// # Ok::<(), markdown_store::Error>(())
/// ```
pub fn from_str<T: DeserializeOwned>(src: &str) -> Result<(T, String), Error> {
    let doc = Document::parse(src)?;
    let value = doc.deserialize()?;
    Ok((value, doc.body))
}

/// One-shot typed render: serialize `value` as the entire frontmatter, with
/// `body` after the fence.
pub fn to_string<T: Serialize>(value: &T, body: &str) -> Result<String, Error> {
    let mut doc = Document::new();
    doc.merge_serialize(value, &[])?;
    doc.set_body(body);
    doc.render()
}

/// Human-readable YAML value kind, for error messages.
fn yaml_kind(value: &serde_norway::Value) -> &'static str {
    match value {
        serde_norway::Value::Null => "null",
        serde_norway::Value::Bool(_) => "a boolean",
        serde_norway::Value::Number(_) => "a number",
        serde_norway::Value::String(_) => "a string",
        serde_norway::Value::Sequence(_) => "a sequence",
        serde_norway::Value::Mapping(_) => "a mapping",
        serde_norway::Value::Tagged(_) => "a tagged value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── split: mirrors markdown-vault's suite case for case ─────────────

    #[test]
    fn split_no_frontmatter() {
        let (yaml, body) = split("hello world");
        assert!(yaml.is_none());
        assert_eq!(body, "hello world");
    }

    #[test]
    fn split_frontmatter_followed_by_body() {
        let src = "---\ntags: [a, b]\n---\nbody here\n";
        let (yaml, body) = split(src);
        assert_eq!(yaml, Some("tags: [a, b]"));
        assert_eq!(body, "body here\n");
    }

    #[test]
    fn split_frontmatter_at_eof() {
        let src = "---\ntags: [a]\n---";
        let (yaml, body) = split(src);
        assert_eq!(yaml, Some("tags: [a]"));
        assert_eq!(body, "");
    }

    #[test]
    fn split_frontmatter_crlf() {
        let src = "---\r\ntags: [a, b]\r\n---\r\nbody\r\n";
        let (yaml, body) = split(src);
        assert_eq!(yaml, Some("tags: [a, b]\r"));
        assert_eq!(body, "body\r\n");
    }

    #[test]
    fn split_missing_terminator_means_no_frontmatter() {
        let src = "---\ntags: [a]\nno terminator\n";
        let (yaml, _) = split(src);
        assert!(yaml.is_none());
    }

    #[test]
    fn split_dashes_not_at_file_start_arent_frontmatter() {
        let src = "intro\n---\ntags: [a]\n---\n";
        let (yaml, _) = split(src);
        assert!(yaml.is_none());
    }

    #[test]
    fn split_empty_frontmatter() {
        let (yaml, body) = split("---\n---\nbody\n");
        assert_eq!(yaml, Some(""));
        assert_eq!(body, "body\n");
    }

    #[test]
    fn split_false_terminator_inside_yaml() {
        let src = "---\nkey: |\n  ---frontmatter-looking text\nreal: 1\n---\nbody\n";
        let (yaml, body) = split(src);
        assert_eq!(yaml, Some("key: |\n  ---frontmatter-looking text\nreal: 1"));
        assert_eq!(body, "body\n");
    }

    // ── Document round-trip ──────────────────────────────────────────────

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Task {
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        epic: Option<String>,
        tags: Vec<String>,
        priority: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        estimate: Option<f64>,
    }

    fn task_src() -> &'static str {
        "---\ntitle: Ship the parser\nepic: \"[[E0042]]\"\ntags:\n- alpha\n- beta\npriority: 2\nhand_added: keep me\n---\nThe body.\n\nWith two paragraphs.\n"
    }

    #[test]
    fn typed_roundtrip_preserves_unknown_keys_and_body() {
        let mut doc = Document::parse(task_src()).unwrap();
        let task: Task = doc.deserialize().unwrap();
        assert_eq!(task.title, "Ship the parser");
        assert_eq!(task.epic.as_deref(), Some("[[E0042]]"));
        assert_eq!(task.tags, vec!["alpha", "beta"]);
        assert_eq!(task.priority, 2);

        let updated = Task {
            title: "Ship the parser".into(),
            epic: task.epic.clone(),
            tags: task.tags.clone(),
            priority: 3,
            estimate: None,
        };
        doc.merge_serialize(&updated, &["title", "epic", "tags", "priority", "estimate"]).unwrap();
        let out = doc.render().unwrap();

        assert!(out.contains("priority: 3"), "updated key rewritten: {out}");
        assert!(out.contains("hand_added: keep me"), "unknown key preserved: {out}");
        assert!(out.ends_with("---\nThe body.\n\nWith two paragraphs.\n"), "body verbatim: {out}");

        // And the rendered output parses back to the same typed value.
        let (back, body) = from_str::<Task>(&out).unwrap();
        assert_eq!(back, updated);
        assert_eq!(body, "The body.\n\nWith two paragraphs.\n");
    }

    #[test]
    fn owned_key_cleared_to_none_is_removed() {
        let mut doc = Document::parse(task_src()).unwrap();
        let task = Task { title: "t".into(), epic: None, tags: vec![], priority: 0, estimate: None };
        doc.merge_serialize(&task, &["title", "epic", "tags", "priority", "estimate"]).unwrap();
        assert!(doc.get("epic").is_none(), "cleared Option must not leave a stale key");
        assert!(doc.get("hand_added").is_some(), "non-owned key survives");
    }

    #[test]
    fn key_order_is_preserved_across_merge() {
        let mut doc = Document::parse(task_src()).unwrap();
        let task = Task {
            title: "Re-titled".into(),
            epic: Some("[[E0042]]".into()),
            tags: vec!["alpha".into()],
            priority: 9,
            estimate: None,
        };
        doc.merge_serialize(&task, &["title", "epic", "tags", "priority", "estimate"]).unwrap();
        let keys: Vec<String> = doc.mapping().iter().map(|(k, _)| k.as_str().unwrap_or_default().to_string()).collect();
        assert_eq!(keys, vec!["title", "epic", "tags", "priority", "hand_added"], "overwritten keys keep position");
    }

    #[test]
    fn multiline_and_numeric_values_roundtrip() {
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Note {
            note: String,
            count: i64,
            ratio: f64,
        }
        let original = Note { note: "line one\nline two\n".into(), count: 42, ratio: 0.5 };
        let rendered = to_string(&original, "body\n").unwrap();
        let (back, body) = from_str::<Note>(&rendered).unwrap();
        assert_eq!(back, original);
        assert_eq!(body, "body\n");
    }

    #[test]
    fn wikilink_strings_roundtrip_through_yaml_quoting() {
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Ref {
            epic: String,
            tags: Vec<String>,
        }
        let original = Ref { epic: "[[E0042]]".into(), tags: vec!["[[productivity]]".into(), "[[ops]]".into()] };
        let rendered = to_string(&original, "").unwrap();
        let (back, _) = from_str::<Ref>(&rendered).unwrap();
        assert_eq!(back, original, "bracket-leading strings must be quoted by the emitter: {rendered}");
    }

    #[test]
    fn malformed_yaml_is_an_error_not_empty() {
        let err = Document::parse("---\n: : : invalid\n---\nbody\n").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn non_mapping_frontmatter_is_an_error() {
        let err = Document::parse("---\n- just\n- a\n- list\n---\n").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn bodyless_and_fenceless_renders_are_stable() {
        // Never had frontmatter: renders as the bare body.
        let doc = Document::parse("just text\n").unwrap();
        assert_eq!(doc.render().unwrap(), "just text\n");

        // Had an empty fence: keeps it.
        let doc = Document::parse("---\n---\nbody\n").unwrap();
        assert_eq!(doc.render().unwrap(), "---\n---\nbody\n");
    }

    #[test]
    fn set_get_remove_roundtrip() {
        let mut doc = Document::new();
        doc.set("status", "open");
        doc.set("count", 3);
        assert_eq!(doc.get("status").and_then(|v| v.as_str()), Some("open"));
        assert_eq!(doc.remove("count").and_then(|v| v.as_i64()), Some(3));
        assert!(doc.get("count").is_none());
    }
}
