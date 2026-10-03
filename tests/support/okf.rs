//! An OKF 0.2 §11 conformance checker for a bundle directory.
//!
//! A bundle conforms when every non-reserved `.md` file has a parseable
//! YAML frontmatter block with a non-empty string `type`, and the reserved
//! `index.md` and `log.md` files follow §8 and §9: no frontmatter (a
//! bundle-root `index.md` may carry only `okf_version`), and log date
//! headings in `## YYYY-MM-DD` form. Only the hard rules are checked;
//! everything §11 calls soft guidance (unknown types and keys, broken
//! links, missing indexes) is accepted.

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
        let at_root = path.parent() == Some(root);
        let problems = match name.as_str() {
            "index.md" => check_index(&src, at_root),
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

/// §8: no frontmatter, except `okf_version` in the bundle-root index.
fn check_index(src: &str, at_root: bool) -> Vec<String> {
    let doc = match markdown_store::Document::parse(src) {
        Ok(doc) => doc,
        Err(e) => return vec![format!("index frontmatter does not parse: {e}")],
    };
    if !doc.had_frontmatter() {
        return vec![];
    }
    if !at_root {
        return vec!["index.md below the bundle root must not have frontmatter".into()];
    }
    doc.mapping()
        .iter()
        .filter_map(|(k, _)| k.as_str())
        .filter(|k| *k != "okf_version")
        .map(|k| format!("bundle-root index.md may only carry `okf_version`, found `{k}`"))
        .collect()
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
