//! An OKF 0.2 §11 conformance checker for a bundle directory.
//!
//! A bundle conforms when every non-reserved `.md` file has a parseable
//! YAML frontmatter block with a non-empty string `type`, and the reserved
//! `index.md` and `log.md` files follow §8 and §9: no frontmatter (a
//! bundle-root `index.md` may carry only `okf_version`, a string), and log
//! date headings in `## YYYY-MM-DD` form. An `index.md` body is one or more
//! sections, each a heading followed by `* [title](url)` entries with an
//! optional ` - description`, separated by blank lines.
//!
//! Index entries must link to a file or directory that exists. §11 tells
//! consumers to tolerate broken cross-links; the checker holds producers,
//! ontogen's index writer included, to more, because an index is a
//! generated listing of what is there. Everything else §11 calls soft
//! guidance (unknown types and keys, missing indexes) is accepted.

use std::fmt;
use std::path::{Path, PathBuf};

/// One way a file breaks §11.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

/// Check every `.md` file under `root` (hidden entries skipped), returning
/// the violations in path order. An empty result means the bundle conforms.
pub fn check_bundle(root: &Path) -> Vec<Violation> {
    let mut files = Vec::new();
    collect_markdown(root, &mut files);
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        let src = match std::fs::read_to_string(&path) {
            Ok(src) => src,
            Err(e) => {
                violations.push(Violation { path, message: format!("unreadable: {e}") });
                continue;
            }
        };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_ascii_lowercase();
        let problems = match name.as_str() {
            "index.md" => check_index(&src, &path, root),
            "log.md" => check_log(&src),
            _ => check_concept(&src),
        };
        violations.extend(problems.into_iter().map(|message| Violation { path: path.clone(), message }));
    }
    violations
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')) {
            continue;
        }
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            out.push(path);
        }
    }
}

/// §4.1 and §11 rules 1–2: parseable frontmatter with a non-empty `type`.
fn check_concept(src: &str) -> Vec<String> {
    let doc = match markdown_store::Document::parse(src) {
        Ok(doc) => doc,
        Err(e) => return vec![format!("frontmatter does not parse: {e}")],
    };
    if !doc.had_frontmatter() {
        return vec!["concept document has no frontmatter block".into()];
    }
    match doc.get("type") {
        None => vec!["frontmatter has no `type`".into()],
        Some(_) if doc.type_name().is_some_and(|t| !t.trim().is_empty()) => vec![],
        Some(other) => vec![format!("`type` must be a non-empty string, found {other:?}")],
    }
}

/// §8: no frontmatter, except a string `okf_version` in the bundle-root
/// index, and a body of headed sections listing entries that resolve.
fn check_index(src: &str, path: &Path, root: &Path) -> Vec<String> {
    let doc = match markdown_store::Document::parse(src) {
        Ok(doc) => doc,
        Err(e) => return vec![format!("index frontmatter does not parse: {e}")],
    };
    let mut problems = Vec::new();
    if doc.had_frontmatter() {
        if path.parent() != Some(root) {
            problems.push("index.md below the bundle root must not have frontmatter".into());
        } else {
            for (key, value) in doc.mapping() {
                match key.as_str() {
                    Some("okf_version") if value.is_string() => {}
                    Some("okf_version") => problems.push(format!("`okf_version` must be a string, found {value:?}")),
                    _ => problems.push(format!(
                        "bundle-root index.md may only carry `okf_version`, found `{}`",
                        key.as_str().unwrap_or("?")
                    )),
                }
            }
        }
    }
    let dir = path.parent().unwrap_or(root);
    problems.extend(check_index_body(doc.body(), dir, root));
    problems
}

/// The §8 body: `# Heading` lines and `* [title](url) - description`
/// entries, every entry under a heading, every heading after the first
/// preceded by a blank line.
fn check_index_body(body: &str, dir: &Path, root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let mut headings = 0;
    let mut previous_blank = true;
    for line in body.lines() {
        if line.trim().is_empty() {
            previous_blank = true;
            continue;
        }
        if is_heading(line) {
            if headings > 0 && !previous_blank {
                problems.push(format!("heading `{line}` must follow a blank line"));
            }
            headings += 1;
        } else if let Some((target, rest)) = parse_entry(line) {
            if headings == 0 {
                problems.push(format!("entry `{line}` comes before any section heading"));
            }
            if !(rest.is_empty() || rest.strip_prefix(" - ").is_some_and(|d| !d.trim().is_empty())) {
                problems.push(format!("entry `{line}` must end after the link or with ` - <description>`"));
            }
            if let Some(problem) = check_link(&target, dir, root) {
                problems.push(format!("entry `{line}`: {problem}"));
            }
        } else {
            problems.push(format!("line `{line}` is neither a heading nor a `* [title](url)` entry"));
        }
        previous_blank = false;
    }
    if headings == 0 {
        problems.push("index.md has no section: §8 lists entries under one or more headings".into());
    }
    problems
}

fn is_heading(line: &str) -> bool {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    (1..=6).contains(&hashes) && line[hashes..].strip_prefix(' ').is_some_and(|text| !text.trim().is_empty())
}

/// Split `* [title](url)rest` into the link destination and `rest`. The
/// title may hold backslash-escaped brackets; the destination is either
/// `<...>` or free of spaces and parentheses.
fn parse_entry(line: &str) -> Option<(String, &str)> {
    let after = line.strip_prefix("* [")?;
    let mut escaped = false;
    let mut close = None;
    for (i, c) in after.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '[' => return None,
            ']' => {
                close = Some(i);
                break;
            }
            _ => {}
        }
    }
    let close = close?;
    if close == 0 {
        return None;
    }
    let link = after[close + 1..].strip_prefix('(')?;
    if let Some(angled) = link.strip_prefix('<') {
        let end = angled.find('>')?;
        let rest = angled[end + 1..].strip_prefix(')')?;
        return Some((angled[..end].to_string(), rest));
    }
    let end = link.find(')')?;
    let target = &link[..end];
    if target.is_empty() || target.contains([' ', '(']) {
        return None;
    }
    Some((target.to_string(), &link[end + 1..]))
}

/// A relative (or bundle-absolute `/...`) link must name an existing file,
/// or with a trailing `/` an existing directory, after percent-decoding.
fn check_link(target: &str, dir: &Path, root: &Path) -> Option<String> {
    let Some(decoded) = percent_decode(target) else {
        return Some(format!("link `{target}` has a malformed percent-escape"));
    };
    let (path, want_dir) = match decoded.strip_suffix('/') {
        Some(stripped) => (stripped, true),
        None => (decoded.as_str(), false),
    };
    let resolved = match path.strip_prefix('/') {
        Some(from_root) => root.join(from_root),
        None => dir.join(path),
    };
    let ok = if want_dir { resolved.is_dir() } else { resolved.is_file() };
    (!ok).then(|| {
        format!("link `{target}` does not resolve to an existing {}", if want_dir { "directory" } else { "file" })
    })
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// §9: no frontmatter; every `##` heading is an ISO 8601 date.
fn check_log(src: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if markdown_store::frontmatter::split(src).0.is_some() {
        problems.push("log.md must not have frontmatter".into());
    }
    for line in src.lines() {
        if let Some(heading) = line.strip_prefix("## ")
            && !is_iso_date(heading.trim_end())
        {
            problems.push(format!("log heading `{line}` is not `## YYYY-MM-DD`"));
        }
    }
    problems
}

fn is_iso_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return false;
    };
    let number = |part: &str, len: usize| {
        (part.len() == len && part.bytes().all(|c| c.is_ascii_digit())).then(|| part.parse::<u32>().ok()).flatten()
    };
    number(year, 4).is_some()
        && number(month, 2).is_some_and(|m| (1..=12).contains(&m))
        && number(day, 2).is_some_and(|d| (1..=31).contains(&d))
}
