//! Smoke tests driving the GENERATED markdown store over a real temp vault —
//! the CI-enforced proof that ADR 0001's markdown backend not only compiles
//! but behaves: derived slug ids, wikilinked relations on disk, hand-edit
//! preservation, derived has_many views, and the read-mutate-rewrite
//! set_parent path.

use markdown_pilot::Store;
use markdown_pilot::schema::{Note, Task};
use markdown_store::{IdStrategy, VaultHandle, VaultLayout};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("tempdir");
    let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir, IdStrategy::SlugFromField("title".into()));
    (dir, Store::new(vault))
}

fn note(title: &str, body: &str) -> Note {
    Note { id: String::new(), title: title.into(), body: body.into() }
}

fn task(title: &str, parent: Option<&str>, tags: &[&str]) -> Task {
    Task {
        id: String::new(),
        title: title.into(),
        status: "open".into(),
        parent_id: parent.map(str::to_string),
        subtasks: Vec::new(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        body: format!("Body of {title}.\n"),
    }
}

#[tokio::test]
async fn note_crud_lifecycle() {
    let (dir, store) = store();

    // create: slug id derived from the title.
    let created = store.create_note(note("Hello Vault", "First body.\n")).await.expect("create");
    assert_eq!(created.id, "hello-vault");
    assert_eq!(created.body, "First body.\n");

    // The record is a plain markdown file where it should be.
    let raw = std::fs::read_to_string(dir.path().join("notes/hello-vault.md")).expect("file on disk");
    assert!(raw.contains("title: Hello Vault"));
    assert!(raw.ends_with("---\nFirst body.\n"));

    // get / list.
    let got = store.get_note("hello-vault").await.expect("get");
    assert_eq!(got.title, "Hello Vault");
    store.create_note(note("Another", "")).await.expect("create 2");
    let all = store.list_notes(None, None).await.expect("list");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "another", "lexicographic by id");

    // update via the generated patch struct.
    let updated = store
        .update_note(
            "hello-vault",
            markdown_pilot::store::generated::note::NoteUpdate { title: None, body: Some("Edited body.\n".into()) },
        )
        .await
        .expect("update");
    assert_eq!(updated.body, "Edited body.\n");

    // delete → NotFound afterwards.
    store.delete_note("hello-vault").await.expect("delete");
    let err = store.get_note("hello-vault").await.expect_err("gone");
    assert!(format!("{err}").contains("not found"), "{err}");
}

#[tokio::test]
async fn relations_wikilinks_and_derived_views() {
    let (dir, store) = store();

    let parent = store.create_task(task("Parent epic", None, &[])).await.expect("create parent");
    let child_a = store.create_task(task("Child alpha", Some(&parent.id), &["codegen"])).await.expect("child a");
    let child_b = store.create_task(task("Child beta", Some(&parent.id), &["codegen", "storage"])).await.expect("b");

    // belongs_to + m2m are wikilinks on disk; strip back at the boundary.
    let raw = std::fs::read_to_string(dir.path().join(format!("tasks/{}.md", child_b.id))).expect("file");
    assert!(raw.contains("parent_id: '[[parent-epic]]'"), "FK is a wikilink: {raw}");
    assert!(raw.contains("- '[[codegen]]'"), "m2m is a wikilink list: {raw}");
    assert_eq!(child_b.parent_id.as_deref(), Some("parent-epic"));
    assert_eq!(child_b.tags, vec!["codegen", "storage"]);

    // The has_many derived view comes from the reverse walk — nothing stored
    // on the parent record.
    let parent_now = store.get_task(&parent.id).await.expect("get parent");
    assert_eq!(parent_now.subtasks, vec![child_a.id.clone(), child_b.id.clone()]);
    let parent_raw = std::fs::read_to_string(dir.path().join(format!("tasks/{}.md", parent.id))).expect("file");
    assert!(!parent_raw.contains("subtasks"), "derived views never hit frontmatter: {parent_raw}");

    // Reparent via the generated update path (set_parent under the hood).
    let other = store.create_task(task("Other epic", None, &[])).await.expect("other");
    store
        .update_task(
            &other.id,
            markdown_pilot::store::generated::task::TaskUpdate {
                subtasks: Some(vec![child_a.id.clone()]),
                ..Default::default()
            },
        )
        .await
        .expect("reparent");
    let child_a_now = store.get_task(&child_a.id).await.expect("child a");
    assert_eq!(child_a_now.parent_id.as_deref(), Some(other.id.as_str()), "set_parent rewired the child FK");
}

#[tokio::test]
async fn hand_edits_survive_generated_updates() {
    let (dir, store) = store();
    let created = store.create_note(note("Co edited", "Original.\n")).await.expect("create");

    // A human adds a frontmatter key and appends to the body.
    let path = dir.path().join(format!("notes/{}.md", created.id));
    let mut raw = std::fs::read_to_string(&path).unwrap();
    raw = raw.replace("title: Co edited", "title: Co edited\npriority: high");
    raw.push_str("\nHand-written conclusion.\n");
    std::fs::write(&path, raw).unwrap();

    // A generated update touches only its own fields…
    store
        .update_note(
            &created.id,
            markdown_pilot::store::generated::note::NoteUpdate { title: Some("Co edited!".into()), body: None },
        )
        .await
        .expect("update");

    // …and the hand edits survive the rewrite.
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("priority: high"), "hand-added key survives: {after}");
    assert!(after.contains("Hand-written conclusion."), "hand-edited body survives: {after}");
    assert!(after.contains("title: Co edited!"));
}

#[tokio::test]
async fn change_events_fire_per_lifecycle() {
    let (_dir, store) = store();
    let mut rx = store.subscribe();

    let created = store.create_note(note("Eventful", "")).await.expect("create");
    store.delete_note(&created.id).await.expect("delete");

    let first = rx.try_recv().expect("created event");
    assert!(matches!(first.op, markdown_pilot::schema::ChangeOp::Created));
    assert_eq!(first.id, created.id);
    let second = rx.try_recv().expect("deleted event");
    assert!(matches!(second.op, markdown_pilot::schema::ChangeOp::Deleted));
}
