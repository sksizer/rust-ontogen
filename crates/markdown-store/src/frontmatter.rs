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
#[derive(Debug, Clone, Default)]
pub struct Document {
    fm: serde_norway::Mapping,
    body: String,
    had_frontmatter: bool,
    /// The verbatim source this document was parsed from, when it came from
    /// [`parse`](Self::parse). While the document is semantically untouched,
    /// [`render`](Self::render) returns this exact text — the byte-stability
    /// guarantee hand-authored corpora depend on.
    raw: Option<String>,
    /// Whether any *semantic* change (a key's value actually differing, a
    /// key added/removed, the body changing) has occurred since parse.
    /// Writes that change nothing keep the document clean.
    dirty: bool,
}

/// Equality is semantic: two documents are equal when their frontmatter
/// mapping, body, and fence-presence agree — regardless of the raw text
/// they were parsed from or their dirty state.
impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.fm == other.fm && self.body == other.body && self.had_frontmatter == other.had_frontmatter
    }
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
    ///
    /// The original source is retained: while the document stays
    /// semantically untouched, [`render`](Self::render) reproduces it
    /// byte-for-byte (comments, quoting style, list layout and all).
    pub fn parse(src: &str) -> Result<Self, Error> {
        let (yaml, body) = split(src);
        let raw = Some(src.to_string());
        match yaml {
            None => Ok(Self {
                fm: serde_norway::Mapping::new(),
                body: body.to_string(),
                had_frontmatter: false,
                raw,
                dirty: false,
            }),
            Some(yaml) if yaml.trim().is_empty() => Ok(Self {
                fm: serde_norway::Mapping::new(),
                body: body.to_string(),
                had_frontmatter: true,
                raw,
                dirty: false,
            }),
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
                Ok(Self { fm, body: body.to_string(), had_frontmatter: true, raw, dirty: false })
            }
        }
    }

    /// The body text (everything after the closing fence), verbatim.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Replace the body. A no-op replacement (identical text) keeps the
    /// document clean, preserving verbatim render.
    pub fn set_body(&mut self, body: impl Into<String>) {
        let body = body.into();
        if body != self.body {
            self.body = body;
            self.dirty = true;
        }
    }

    /// Whether any semantic change has occurred since [`parse`](Self::parse).
    /// A clean document renders as its original source, byte for byte.
    /// Documents built via [`new`](Self::new) are always considered dirty
    /// (there is no original to reproduce).
    pub fn is_dirty(&self) -> bool {
        self.dirty || self.raw.is_none()
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
    /// manipulation the typed API doesn't cover. Conservatively marks the
    /// document dirty (mutations through the raw mapping can't be observed),
    /// so verbatim render is forfeited even if nothing actually changes;
    /// prefer [`set`](Self::set)/[`remove`](Self::remove)/
    /// [`merge_serialize`](Self::merge_serialize), which are change-aware.
    pub fn mapping_mut(&mut self) -> &mut serde_norway::Mapping {
        self.dirty = true;
        &mut self.fm
    }

    /// Look up a single frontmatter value by string key.
    pub fn get(&self, key: &str) -> Option<&serde_norway::Value> {
        self.fm.get(key)
    }

    /// Insert or overwrite a single frontmatter value. Existing keys keep
    /// their position; new keys append. Setting a key to the value it
    /// already holds is a no-op and keeps the document clean.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<serde_norway::Value>) {
        let key = serde_norway::Value::String(key.into());
        let value = value.into();
        if self.fm.get(&key) != Some(&value) {
            self.fm.insert(key, value);
            self.dirty = true;
        }
    }

    /// Remove a key, preserving the order of the remaining keys. Returns the
    /// removed value, if any. Removing an absent key keeps the document
    /// clean.
    pub fn remove(&mut self, key: &str) -> Option<serde_norway::Value> {
        let removed = self.fm.shift_remove(serde_norway::Value::String(key.to_string()));
        if removed.is_some() {
            self.dirty = true;
        }
        removed
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
            if !new_map.contains_key(&k) && self.fm.shift_remove(&k).is_some() {
                self.dirty = true;
            }
        }
        for (k, v) in new_map {
            // Change-aware insert: writing back the value a key already
            // holds keeps the document clean, so a no-op update round-trips
            // the file byte-for-byte.
            if self.fm.get(&k) != Some(&v) {
                self.fm.insert(k, v);
                self.dirty = true;
            }
        }
        Ok(())
    }

    /// Render the document back to a full markdown string.
    ///
    /// **A clean document renders as its original source, byte for byte** —
    /// comments, quoting style, list layout, everything. "Clean" means it
    /// came from [`parse`](Self::parse) and no semantic change has occurred
    /// since (see [`is_dirty`](Self::is_dirty)); change-aware mutators keep
    /// no-op writes clean. This is the property that makes a
    /// parse→render sweep over a hand-authored corpus a zero-diff
    /// operation.
    ///
    /// A dirty document re-emits its frontmatter through the YAML emitter:
    /// - Non-empty mapping ⇒ `---\n<yaml>---\n<body>`.
    /// - Empty mapping that *had* a fence block ⇒ `---\n---\n<body>`.
    /// - Empty mapping, never had one ⇒ the body alone.
    ///
    /// The emitter output is deterministic and Obsidian-readable but
    /// normalizes hand-authored cosmetics of the whole block (quoting
    /// style, list layout) and drops YAML comments. Narrowing that to
    /// surgical per-key rewrites is planned alongside the corpus fidelity
    /// harness; until then, "mutating a record normalizes its frontmatter
    /// block" is the explicit, documented extent of rewrite lossiness.
    pub fn render(&self) -> Result<String, Error> {
        if let (false, Some(raw)) = (self.is_dirty(), self.raw.as_ref()) {
            return Ok(raw.clone());
        }
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

    // ── byte-stability over hand-authored sources ───────────────────────
    // The zero-diff guarantee a hand+LLM-authored corpus depends on: a
    // parse→render sweep (and a no-op typed update) must reproduce the
    // source EXACTLY — comments, quoting variety, odd spacing, block
    // scalars, everything.

    const HAND_AUTHORED: &str = "---\n\
# pinned by the planning tooling\n\
type: task\n\
schema_version: '5'\n\
id: T-XMPL\n\
status: open/ready\n\
created: '2026-06-06'\n\
reviewed_at: 2026-06-06T09:30:00Z\n\
tags:\n\
- ontological-integration\n\
-     store\n\
depends_on: ['[[T-66TG]]', \"[[T-Z0PE]]\"]\n\
weird_quoting: \"double\"\n\
completion_note: |\n\
\x20 Shipped via #N.\n\
\n\
\x20 Two paragraphs, block scalar.\n\
---\n\
\n\
# A hand-authored task\n\
\n\
Some prose with a block id. ^summary\n\
\n\
## Goal\n\
\n\
Body text stays byte-stable.\n";

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct SdlcTask {
        id: String,
        status: String,
        tags: Vec<String>,
        depends_on: Vec<String>,
    }
    const SDLC_FIELDS: &[&str] = &["id", "status", "tags", "depends_on"];

    #[test]
    fn untouched_parse_renders_verbatim() {
        let doc = Document::parse(HAND_AUTHORED).unwrap();
        assert!(!doc.is_dirty());
        assert_eq!(doc.render().unwrap(), HAND_AUTHORED, "comments, quoting, spacing — all byte-stable");
    }

    #[test]
    fn noop_typed_update_keeps_verbatim_render() {
        // The generated-update shape with nothing actually changing: read,
        // deserialize, merge the SAME values back, re-set the same body.
        // The document must stay clean and render byte-identically.
        let mut doc = Document::parse(HAND_AUTHORED).unwrap();
        let task: SdlcTask = doc.deserialize().unwrap();
        doc.merge_serialize(&task, SDLC_FIELDS).unwrap();
        doc.set_body(doc.body().to_string());
        assert!(!doc.is_dirty(), "no-op writes must not dirty the document");
        assert_eq!(doc.render().unwrap(), HAND_AUTHORED, "no-op update is a zero-diff write");
    }

    #[test]
    fn real_mutation_normalizes_and_is_flagged() {
        let mut doc = Document::parse(HAND_AUTHORED).unwrap();
        let mut task: SdlcTask = doc.deserialize().unwrap();
        task.status = "closed/done".into();
        doc.merge_serialize(&task, SDLC_FIELDS).unwrap();
        assert!(doc.is_dirty());
        let out = doc.render().unwrap();
        assert_ne!(out, HAND_AUTHORED);
        assert!(out.contains("closed/done"));
        // Body is still verbatim even on the normalized path.
        assert!(out.contains("Some prose with a block id. ^summary"));
        assert!(out.contains("## Goal"));
        // Unknown keys still present (values preserved; cosmetics may not be).
        assert!(out.contains("completion_note:"));
        assert!(out.contains("Two paragraphs, block scalar."));
    }

    #[test]
    fn escape_hatch_mapping_mut_forfeits_verbatim() {
        let mut doc = Document::parse(HAND_AUTHORED).unwrap();
        let _ = doc.mapping_mut(); // can't observe what happens in here
        assert!(doc.is_dirty(), "raw-mapping access must conservatively dirty the document");
    }

    #[test]
    fn change_aware_set_and_remove() {
        let mut doc = Document::parse(HAND_AUTHORED).unwrap();
        doc.set("id", "T-XMPL"); // same value
        assert!(doc.remove("nonexistent").is_none());
        assert!(!doc.is_dirty(), "no-op set/remove stay clean");
        doc.set("id", "T-OTHR");
        assert!(doc.is_dirty());
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
