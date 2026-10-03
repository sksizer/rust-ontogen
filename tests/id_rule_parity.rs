//! One create-time id rule binds both store backends (JSON:API wire
//! contract §8.2), but it has two implementations: `ontogen_core::id`,
//! which generated SeaORM stores call, and `markdown_store::layout`, which
//! the vault enforces on create (markdown-store takes no ontogen
//! dependency). This test holds them to the same answers, reasons and
//! messages, and
//! their slug functions and probe sequences to the same output, so an id
//! one backend creates the other creates too.

use markdown_store::layout;
use ontogen_core::id;

/// Ids built at run time: the length limit's edges and long slug sources.
fn generated() -> Vec<String> {
    vec![
        "a".repeat(id::MAX_ID_LEN - 1),
        "a".repeat(id::MAX_ID_LEN),
        "a".repeat(id::MAX_ID_LEN + 1),
        format!("{}.", "a".repeat(id::MAX_ID_LEN - 1)),
        "é".repeat(id::MAX_ID_LEN / 2),
        "A".repeat(id::MAX_ID_LEN),
        "word ".repeat(100),
        "Ünïcödé Títle ".repeat(30),
        format!("{}-tail", "y".repeat(id::SLUG_MAX_LEN - 1)),
        "ß".repeat(200),
    ]
}

fn everything() -> Vec<String> {
    CORPUS.iter().map(|s| s.to_string()).chain(generated()).collect()
}

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
    // Windows device names, whole and before the first dot, in any case
    "con",
    "prn",
    "aux",
    "nul",
    "com1",
    "com9",
    "lpt1",
    "lpt9",
    "CON",
    "Nul",
    "nul.x",
    "com1.backup",
    "con.a.b",
    "LPT3.TXT",
    "con.",
    // near the device names
    "console",
    "con-2",
    "xcon",
    "a.con",
    ".con",
    "com",
    "com0",
    "com10",
    "lpt0",
    "nulls",
    "con_x",
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
    // non-ASCII, including decomposed accents
    "é",
    "café",
    "cafe\u{301}",
    "日本",
    "Ω",
    "\u{feff}a",
    // uppercase
    "B",
    "Zeta",
    "aB",
    "SHIP-IT",
    // the punctuation the charset allows, and some it does not
    "a_b",
    "a~b",
    "~",
    "_",
    "-",
    "a-",
    "-a",
    "a+b",
    "a%20b",
    "a@b",
    "a#b",
    "a?b",
    "a*b",
    "a\"b",
    "a<b",
    "a|b",
    "a,b",
    "a'b",
    // ordinary slugs
    "a",
    "ship-the-parser",
    "same-title-2",
    "0",
    "10",
    "v2.0.1",
];

#[test]
fn both_validators_agree_on_every_id() {
    let mut disagreements = Vec::new();
    for candidate in everything() {
        let candidate = candidate.as_str();
        let core = id::validate_id(candidate).map_err(|e| (e.reason.to_string(), e.to_string()));
        let vault = layout::validate_id(candidate).map_err(|e| match e {
            markdown_store::Error::InvalidId { ref reason, .. } => (reason.clone(), e.to_string()),
            other => panic!("{candidate:?}: {other:?}"),
        });
        if core != vault {
            disagreements.push(format!("{candidate:?}: ontogen_core says {core:?}, markdown_store says {vault:?}"));
        }
    }
    assert!(disagreements.is_empty(), "the two id rules disagree:\n{}", disagreements.join("\n"));
}

#[test]
fn both_state_the_same_rule() {
    assert_eq!(id::ID_RULE, layout::ID_RULE);
    assert_eq!(id::RESERVED_REASON, layout::RESERVED_REASON);
    assert_eq!(id::DEVICE_NAME_REASON, layout::DEVICE_NAME_REASON);
}

#[test]
fn the_corpus_exercises_both_answers() {
    let all = everything();
    let valid = all.iter().filter(|c| id::validate_id(c).is_ok()).count();
    assert!(valid > 10 && valid < all.len() - 10, "a corpus that is all one answer proves little: {valid} valid");
}

#[test]
fn both_crates_share_the_limits() {
    assert_eq!(id::MAX_ID_LEN, layout::MAX_ID_LEN);
    assert_eq!(id::SLUG_MAX_LEN, markdown_store::id::SLUG_MAX_LEN);
    const { assert!(layout::MAX_STEM_LEN >= id::MAX_ID_LEN, "every creatable id is reachable") };
}

#[test]
fn every_creatable_id_is_reachable_on_markdown() {
    for candidate in everything() {
        if id::validate_id(&candidate).is_ok() {
            assert!(layout::validate_lookup_id(&candidate).is_ok(), "{candidate:?}");
        }
    }
}

#[test]
fn both_fold_tables_agree() {
    for c in ('\u{80}'..='\u{24f}').chain(['Ω', '日', '\u{301}']) {
        assert_eq!(id::fold_latin(c), markdown_store::id::fold_latin(c), "{c:?}");
    }
}

#[test]
fn both_probe_the_same_candidates() {
    let bases = [
        "draft".to_string(),
        "index".to_string(),
        "LOG".to_string(),
        "con".to_string(),
        "lpt9".to_string(),
        "b".repeat(250),
        format!("{}-tail", "c".repeat(197)),
    ];
    for base in bases {
        let core: Vec<String> = id::candidates(&base).take(12).collect();
        let vault: Vec<String> = markdown_store::id::candidates(&base).take(12).collect();
        assert_eq!(core, vault, "{base:?}");
        let far: Vec<String> = id::candidates(&base).skip(100_000).take(2).collect();
        assert_eq!(far, markdown_store::id::candidates(&base).skip(100_000).take(2).collect::<Vec<_>>());
        for candidate in core.iter().chain(&far) {
            assert!(candidate.len() <= id::MAX_ID_LEN, "{candidate:?} is {} bytes", candidate.len());
        }
    }
}

#[test]
fn both_agree_on_what_is_reserved() {
    for &candidate in CORPUS {
        assert_eq!(id::is_reserved_id(candidate), layout::is_reserved_id(candidate), "{candidate:?}");
        assert_eq!(id::is_device_name(candidate), layout::is_device_name(candidate), "{candidate:?}");
    }
    assert_eq!(id::RESERVED_IDS, layout::RESERVED_IDS);
    assert_eq!(id::DEVICE_NAMES, layout::DEVICE_NAMES);
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
        "Con",
        "nul.x",
        "COM1 backup",
        "MiXeD 123 case",
        "a",
        "trailing-",
        "Crème brûlée à Łódź",
        "Straße Œuvre Ĳssel Þór",
        "re\u{301}sume\u{301}",
        "2×3÷4",
        "snake_case~tilde",
    ];
    for title in titles.iter().map(|s| s.to_string()).chain(everything()) {
        assert_eq!(id::slugify(&title), markdown_store::id::slugify(&title), "{title:?}");
    }
}

#[test]
fn every_nonempty_slug_is_a_valid_id_unless_reserved() {
    for title in everything() {
        let slug = id::slugify(&title);
        assert!(slug.len() <= id::SLUG_MAX_LEN, "{title:?} slugified to {} bytes", slug.len());
        if !slug.is_empty() && !id::is_reserved_id(&slug) && !id::is_device_name(&slug) {
            assert!(id::validate_id(&slug).is_ok(), "{title:?} slugified to the invalid id {slug:?}");
        }
        // With no `.` in a slug, the first probe `candidates` yields is
        // creatable even when the slug itself is reserved.
        assert!(!slug.contains('.'), "{title:?} slugified to {slug:?}");
        if !slug.is_empty() {
            let first = id::candidates(&slug).next().expect("an endless sequence");
            assert!(id::validate_id(&first).is_ok(), "{title:?} probes {first:?} first");
        }
    }
}
