//! Wikilink encode / strip / parse across the shapes generated store code
//! deals with: scalar foreign keys, optional foreign keys, and
//! many-to-many lists — plus alias/anchor parsing for richer vault tooling.
//!
//! Run with: `cargo run -p markdown-store --example wikilinks`

use markdown_store::wikilink::{self, WikiLink};

fn main() {
    // Encoding: how relation ids land in frontmatter.
    let epic_ref = wikilink::encode("E0042");
    println!("encoded epic ref:   {epic_ref}");
    assert_eq!(epic_ref, "[[E0042]]");

    // Stripping: scalar / optional / list — the typed-boundary shapes.
    assert_eq!(wikilink::strip("[[E0042]]"), "E0042");
    assert_eq!(wikilink::strip_opt(Some("[[E0042]]".into())), Some("E0042".into()));
    assert_eq!(
        wikilink::strip_vec(vec!["[[productivity]]".into(), "[[ops]]".into(), "already-plain".into()]),
        vec!["productivity", "ops", "already-plain"]
    );

    // Idempotent + forgiving: plain ids and already-stripped values pass
    // through, so generated code can strip unconditionally.
    assert_eq!(wikilink::strip("plain-id"), "plain-id");
    assert_eq!(wikilink::strip(&wikilink::strip("[[x]]")), "x");

    // Parsing: Obsidian's full form, for tooling beyond the store layer.
    let link = wikilink::parse("[[notes/setup#install|Install guide]]").expect("valid link");
    println!("parsed: target={} anchor={:?} alias={:?}", link.target, link.anchor, link.alias);
    assert_eq!(
        link,
        WikiLink { target: "notes/setup".into(), anchor: Some("install".into()), alias: Some("Install guide".into()) }
    );

    // Things that are NOT a single link are left alone / unparsed.
    assert_eq!(wikilink::strip("[[a]] [[b]]"), "[[a]] [[b]]");
    assert!(wikilink::parse("[[a]] [[b]]").is_none());

    println!("wikilinks ok");
}
