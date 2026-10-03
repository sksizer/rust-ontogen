//! The generated markdown store's create, lookup, order and `has_many`
//! rules (JSON:API wire contract §5.4 and §8.2, ADR 0006 §3), driven over a
//! real temp vault: the typed `IdRequired` / `AlreadyExists` /
//! `ParentRequired` errors, `-2` probing, lookups of ids no record can
//! have, id byte order, and dropped `has_many` children.

use markdown_pilot::Store;
use markdown_pilot::schema::{AppError, Note, Section, Task};
use markdown_pilot::store::generated::section::SectionUpdate;
use markdown_pilot::store::generated::task::TaskUpdate;
use markdown_store::{VaultHandle, VaultLayout};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("tempdir");
    let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir);
    (dir, Store::new(vault))
}

fn note(id: &str, title: &str) -> Note {
    Note { id: id.into(), title: title.into(), body: String::new() }
}

fn task(id: &str, parent: Option<&str>) -> Task {
    Task {
        id: id.into(),
        title: format!("Task {id}"),
        status: "open".into(),
        parent_id: parent.map(str::to_string),
        subtasks: Vec::new(),
        tags: Vec::new(),
        body: String::new(),
    }
}

fn section(id: &str, parent: &str) -> Section {
    Section { id: id.into(), title: format!("Section {id}"), parent_id: parent.into(), children: Vec::new() }
}

/// Every file under the vault, path → bytes, to prove a refused write
/// touched nothing.
fn snapshot(dir: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

#[tokio::test]
async fn a_create_with_no_id_to_derive_is_id_required() {
    let (dir, store) = store();
    for id in ["", "  ", "\t"] {
        match store.create_note(note(id, "!!!")).await {
            Err(AppError::NoteIdRequired(reason)) => assert_eq!(reason, "field \"title\" produced an empty slug"),
            other => panic!("expected NoteIdRequired for id {id:?}, got {other:?}"),
        }
    }
    assert!(snapshot(dir.path()).is_empty(), "nothing was written");
}

#[tokio::test]
async fn a_blank_id_is_absent_and_a_derived_id_probes_past_taken_and_reserved_ones() {
    let (_dir, store) = store();
    assert_eq!(store.create_note(note(" ", "Same")).await.unwrap().id, "same");
    assert_eq!(store.create_note(note("", "Same")).await.unwrap().id, "same-2");
    assert_eq!(store.create_note(note("", "Index")).await.unwrap().id, "index-2");
}

#[tokio::test]
async fn a_duplicate_provided_id_already_exists() {
    let (_dir, store) = store();
    store.create_note(note("fixed", "First")).await.expect("create");
    match store.create_note(note("fixed", "Second")).await {
        Err(AppError::NoteAlreadyExists(id)) => assert_eq!(id, "fixed"),
        other => panic!("expected NoteAlreadyExists, got {other:?}"),
    }
    assert_eq!(store.get_note("fixed").await.unwrap().title, "First", "the original is untouched");
}

#[tokio::test]
async fn an_invalid_provided_id_is_not_a_typed_error() {
    let (_dir, store) = store();
    for id in ["a/b", "log", ".hidden"] {
        match store.create_note(note(id, "x")).await {
            Err(AppError::Md(msg)) => assert!(msg.contains("invalid id"), "{msg}"),
            other => panic!("an invalid id is a server-side bug (500), got {other:?} for {id:?}"),
        }
    }
}

#[tokio::test]
async fn lookups_of_an_id_no_record_can_have_are_not_found() {
    let (_dir, store) = store();
    for id in ["a/b", "..", "\t", "index", "C:x", "trailing."] {
        assert!(matches!(store.get_note(id).await, Err(AppError::NoteNotFound(got)) if got == id), "get {id:?}");
        let update = markdown_pilot::store::generated::note::NoteUpdate { title: Some("x".into()), body: None };
        assert!(matches!(store.update_note(id, update).await, Err(AppError::NoteNotFound(_))), "update {id:?}");
        assert!(matches!(store.delete_note(id).await, Err(AppError::NoteNotFound(_))), "delete {id:?}");
    }
}

#[tokio::test]
async fn lists_and_has_many_children_are_in_id_byte_order() {
    let (_dir, store) = store();
    for id in ["z", "é", "a", "B"] {
        store.create_note(note(id, id)).await.expect("create");
    }
    let ids: Vec<String> = store.list_notes(None, None).await.unwrap().into_iter().map(|n| n.id).collect();
    assert_eq!(ids, ["B", "a", "z", "é"], "UTF-8 byte order, not case-folded or locale order");
    let page: Vec<String> = store.list_notes(Some(2), Some(1)).await.unwrap().into_iter().map(|n| n.id).collect();
    assert_eq!(page, ["a", "z"]);

    store.create_task(task("parent", None)).await.unwrap();
    for id in ["c", "a", "b"] {
        store.create_task(task(id, Some("parent"))).await.unwrap();
    }
    assert_eq!(store.get_task("parent").await.unwrap().subtasks, ["a", "b", "c"]);
}

#[tokio::test]
async fn a_has_many_update_clears_the_foreign_key_of_a_dropped_child() {
    let (_dir, store) = store();
    store.create_task(task("parent", None)).await.unwrap();
    store.create_task(task("kept", Some("parent"))).await.unwrap();
    store.create_task(task("dropped", Some("parent"))).await.unwrap();
    store.create_task(task("adopted", None)).await.unwrap();

    let updated = store
        .update_task(
            "parent",
            TaskUpdate { subtasks: Some(vec!["kept".into(), "adopted".into()]), ..Default::default() },
        )
        .await
        .expect("update");

    assert_eq!(updated.subtasks, ["adopted", "kept"]);
    assert_eq!(store.get_task("dropped").await.unwrap().parent_id, None, "the dropped child is cleared");
    assert_eq!(store.get_task("adopted").await.unwrap().parent_id.as_deref(), Some("parent"));
    assert_eq!(store.get_task("kept").await.unwrap().parent_id.as_deref(), Some("parent"));
}

#[tokio::test]
async fn dropping_a_child_whose_parent_is_required_writes_nothing() {
    let (dir, store) = store();
    store.create_section(section("book", "book")).await.unwrap();
    store.create_section(section("ch1", "book")).await.unwrap();
    store.create_section(section("ch2", "book")).await.unwrap();
    store.create_section(section("appendix", "appendix")).await.unwrap();
    assert_eq!(store.get_section("book").await.unwrap().children, ["ch1", "ch2"], "a root is not its own child");

    let before = snapshot(dir.path());
    let refused = store
        .update_section(
            "book",
            SectionUpdate { title: Some("Renamed".into()), children: Some(vec!["ch1".into()]), ..Default::default() },
        )
        .await;
    match refused {
        Err(AppError::SectionParentRequired(id)) => assert_eq!(id, "ch2"),
        other => panic!("expected SectionParentRequired, got {other:?}"),
    }
    assert_eq!(snapshot(dir.path()), before, "the refusal came before any write, the record's own included");

    // Adding a child moves it from its previous parent; keeping every
    // existing child is allowed.
    let updated = store
        .update_section(
            "book",
            SectionUpdate { children: Some(vec!["ch1".into(), "ch2".into(), "appendix".into()]), ..Default::default() },
        )
        .await
        .expect("an update that drops nothing");
    assert_eq!(updated.children, ["appendix", "ch1", "ch2"]);
    assert_eq!(store.get_section("appendix").await.unwrap().parent_id, "book");
}
