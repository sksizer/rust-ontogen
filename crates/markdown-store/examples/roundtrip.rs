//! Round-trip a planning-tracker-shaped record (the `docs/planning/tasks`
//! frontmatter shape: status / created / epic / tags + a markdown body),
//! demonstrating the property the crate exists for: **typed updates never
//! destroy hand-authored content** — unknown keys, key order, and the body
//! all survive.
//!
//! Run with: `cargo run -p markdown-store --example roundtrip`

use markdown_store::{wikilink, Document};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Task {
    status: String,
    created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    epic: Option<String>,
    tags: Vec<String>,
}

const TASK_FIELDS: &[&str] = &["status", "created", "epic", "tags"];

fn main() -> Result<(), markdown_store::Error> {
    // A file as a human (or the SDLC tooling) would author it — including a
    // `completion_note` field our Task type doesn't model, and a multi-line
    // body.
    let src = "---\n\
               status: in-progress\n\
               created: 2026-05-29\n\
               epic: \"[[markdown-backend]]\"\n\
               tags:\n\
               - codegen\n\
               - storage\n\
               completion_note: |\n\
               \x20 Hand-written. Not modeled by the Task type.\n\
               ---\n\
               \n\
               ## Goal\n\
               \n\
               Ship the markdown store backend.\n";

    let mut doc = Document::parse(src)?;

    // Typed read. The wikilink is stripped at the boundary, not in the file.
    let mut task: Task = doc.deserialize()?;
    println!("loaded: status={} epic={:?}", task.status, wikilink::strip_opt(task.epic.clone()));
    assert_eq!(wikilink::strip_opt(task.epic.clone()).as_deref(), Some("markdown-backend"));

    // Typed update: close the task.
    task.status = "closed".into();
    doc.merge_serialize(&task, TASK_FIELDS)?;

    let out = doc.render()?;
    println!("\n--- rendered after typed update ---\n{out}");

    // The update is in...
    assert!(out.contains("status: closed"));
    // ...and everything the type didn't model is untouched.
    assert!(out.contains("completion_note:"), "unknown key preserved");
    assert!(out.contains("Hand-written. Not modeled"), "unknown value preserved");
    assert!(out.contains("## Goal"), "body preserved");
    // Key order: status is still the first key.
    let first_key = doc.mapping().iter().next().and_then(|(k, _)| k.as_str()).unwrap();
    assert_eq!(first_key, "status", "key order preserved");

    println!("roundtrip ok: typed update, zero collateral damage");
    Ok(())
}
