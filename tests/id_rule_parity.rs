//! One id rule binds both store backends (JSON:API wire contract §8.2), but
//! it has two implementations: `ontogen_core::id`, which generated SeaORM
//! stores call, and `markdown_store::layout`, which the vault enforces at
//! its path boundary (markdown-store takes no ontogen dependency). This test
//! holds them to the same answers, and their slug functions to the same
//! output, so an id one backend accepts the other accepts too.

use markdown_store::layout;
use ontogen_core::id;

/// Ids on both sides of every clause of the rule.
const CORPUS: &[&str] = &[
    // empty and whitespace-only
    "",
    " ",
    "  ",
    "\t",
    "\n",
    " \t\n",
    "\u{a0}",
    // reserved stems, in any case
    "index",
    "log",
    "Index",
    "INDEX",
    "Log",
    "lOg",
    // near the reserved stems
    "index-2",
    "logs",
    "changelog",
    "my-index",
    "index.md",
    // dots
    ".",
    "..",
    ".hidden",
    "..x",
    "v1.2-notes",
    "dotted.",
    "a.b",
    // trailing and inner spaces
    "spaced ",
    " leading",
    "inner space",
    // separators, drive prefixes, NUL
    "a/b",
    "/abs",
    "a\\b",
    "C:evil",
    "a:stream",
    "x\0y",
    "../escape",
    // non-ASCII
    "é",
    "café",
    "日本",
    "B",
    // ordinary slugs
    "a",
    "ship-the-parser",
    "same-title-2",
    "0",
];

#[test]
fn both_validators_agree_on_every_id() {
    let mut disagreements = Vec::new();
    for &candidate in CORPUS {
        let core = id::validate_id(candidate).is_ok();
        let vault = layout::validate_id(candidate).is_ok();
        if core != vault {
            disagreements.push(format!("{candidate:?}: ontogen_core says {core}, markdown_store says {vault}"));
        }
    }
    assert!(disagreements.is_empty(), "the two id rules disagree:\n{}", disagreements.join("\n"));
}

#[test]
fn the_corpus_exercises_both_answers() {
    let valid = CORPUS.iter().filter(|c| id::validate_id(c).is_ok()).count();
    assert!(valid > 5 && valid < CORPUS.len() - 5, "a corpus that is all one answer proves little: {valid} valid");
}

#[test]
fn both_agree_on_what_is_reserved() {
    for &candidate in CORPUS {
        assert_eq!(id::is_reserved_id(candidate), layout::is_reserved_id(candidate), "{candidate:?}");
    }
    assert_eq!(id::RESERVED_IDS, layout::RESERVED_IDS);
}

#[test]
fn both_slug_functions_agree() {
    let titles = [
        "Ship the Parser!",
        "  --Weird__ input--  ",
        "***",
        "",
        "v1.2 release notes",
        "Äpfel und Birnen",
        "日本語のタイトル",
        "Index",
        "MiXeD 123 case",
        "a",
        "trailing-",
    ];
    for title in titles.iter().copied().chain(CORPUS.iter().copied()) {
        assert_eq!(id::slugify(title), markdown_store::id::slugify(title), "{title:?}");
    }
}

#[test]
fn every_nonempty_slug_is_a_valid_id_unless_reserved() {
    for &title in CORPUS {
        let slug = id::slugify(title);
        if !slug.is_empty() && !id::is_reserved_id(&slug) {
            assert!(id::validate_id(&slug).is_ok(), "{title:?} slugified to the invalid id {slug:?}");
        }
    }
}
