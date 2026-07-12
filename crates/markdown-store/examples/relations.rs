//! Relationship patterns on a planning-tracker shape, exactly as ADR 0001's
//! relationship model specifies them for the markdown backend:
//!
//! - `belongs_to` — a wikilink field on the child (`epic: "[[...]]"`),
//!   updated by read-modify-write (the `set_parent` pattern).
//! - `has_many` (reverse walk) — no stored list anywhere; walk the child
//!   directory and filter on the foreign key. O(N) over the child folder.
//! - `many_to_many` (authoritative side) — a wikilink list on the owning
//!   record (`tags: ["[[a]]", "[[b]]"]`); the other side is a derived view
//!   by the same reverse-walk mechanism.
//!
//! Run with: `cargo run -p markdown-store --example relations`

use markdown_store::{wikilink, Document, Error, IdStrategy, VaultHandle, VaultLayout};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Task {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    epic: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

const TASK_FIELDS: &[&str] = &["title", "epic", "tags"];

fn create_task(vault: &VaultHandle, title: &str, epic: Option<&str>, tags: &[&str]) -> Result<String, Error> {
    let task = Task {
        title: title.into(),
        // Foreign ids are stored as wikilinks — the Obsidian-graph contract.
        epic: epic.map(wikilink::encode),
        tags: tags.iter().map(|t| wikilink::encode(t)).collect(),
    };
    let mut doc = Document::new();
    doc.merge_serialize(&task, TASK_FIELDS)?;
    // Id derivation + dedup + write happen atomically under the write lock.
    vault.create_record_derived("tasks", None, Some(title), &doc)
}

fn main() -> Result<(), Error> {
    let dir = tempfile::tempdir().expect("tempdir");
    let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir, IdStrategy::SlugFromField("title".into()));

    // One epic, three tasks pointing at it (two sharing a tag).
    let mut epic_doc = Document::new();
    epic_doc.set("title", "Markdown backend");
    vault.create_record("epics", "markdown-backend", &epic_doc)?;

    let a = create_task(&vault, "Lift the store layer", Some("markdown-backend"), &["codegen"])?;
    let b = create_task(&vault, "Write the emitter", Some("markdown-backend"), &["codegen", "storage"])?;
    let c = create_task(&vault, "Unrelated chore", None, &[])?;

    // ── belongs_to read: strip the wikilink at the typed boundary ───────
    let task_a: Task = vault.read_record("tasks", &a)?.deserialize()?;
    assert_eq!(wikilink::strip_opt(task_a.epic).as_deref(), Some("markdown-backend"));

    // ── has_many reverse walk: which tasks belong to the epic? ──────────
    // No junction, no stored child list: walk the child dir, filter on the
    // foreign key. This is exactly what generated `populate_*_relations`
    // emits for has_many.
    let children: Vec<String> = vault
        .read_all("tasks")?
        .into_iter()
        .filter_map(|(id, doc)| {
            let task: Task = doc.deserialize().ok()?;
            (wikilink::strip_opt(task.epic).as_deref() == Some("markdown-backend")).then_some(id)
        })
        .collect();
    println!("epic children: {children:?}");
    assert_eq!(children, vec![a.clone(), b.clone()]); // sorted by id: lift… < write…

    // ── set_parent: read-mutate-rewrite the child's FK field ────────────
    // (The markdown replacement for SeaORM's raw-SQL fast path.)
    vault.modify_record("tasks", &c, |doc| {
        let mut task: Task = doc.deserialize()?;
        task.epic = Some(wikilink::encode("markdown-backend"));
        doc.merge_serialize(&task, TASK_FIELDS)
    })?;
    let task_c: Task = vault.read_record("tasks", &c)?.deserialize()?;
    assert_eq!(wikilink::strip_opt(task_c.epic).as_deref(), Some("markdown-backend"));
    println!("set_parent({c}) via read-modify-write");

    // ── many_to_many, authoritative side ────────────────────────────────
    // The owning record carries the full list; create/update writes it with
    // the record itself — no junction sync step exists on this backend.
    let task_b: Task = vault.read_record("tasks", &b)?.deserialize()?;
    assert_eq!(wikilink::strip_vec(task_b.tags), vec!["codegen", "storage"]);

    // The derived view (which tasks carry tag "codegen"?) is the same
    // reverse-walk mechanism as has_many:
    let tagged: Vec<String> = vault
        .read_all("tasks")?
        .into_iter()
        .filter_map(|(id, doc)| {
            let task: Task = doc.deserialize().ok()?;
            wikilink::strip_vec(task.tags).iter().any(|t| t == "codegen").then_some(id)
        })
        .collect();
    println!("tasks tagged codegen: {tagged:?}");
    assert_eq!(tagged, vec![a, b]);

    // And on disk, it's all plain Obsidian-flavored markdown:
    let raw = std::fs::read_to_string(vault.record_path("tasks", &c)?).expect("read raw");
    println!("\n--- tasks/{c}.md on disk ---\n{raw}");
    assert!(raw.contains("epic: '[[markdown-backend]]'"));

    println!("relations ok");
    Ok(())
}
