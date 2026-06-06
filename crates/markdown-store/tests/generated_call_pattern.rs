//! Rehearsal of the exact call pattern ontogen's markdown store backend
//! generates per CRUD operation (ADR 0001). Each test body below is shaped
//! like the corresponding generated `impl Store` method body; the golden
//! consumer files in the campaign's showcase PR formalize this contract,
//! and the emitter is later graded against those goldens.
//!
//! If a change here is needed to make the crate ergonomic, the generated
//! code shape changes with it — review accordingly.

use markdown_store::{wikilink, Document, Error, IdStrategy, VaultHandle, VaultLayout};
use serde::{Deserialize, Serialize};

// ── what the consumer crate provides ─────────────────────────────────────

/// Domain entity, as `crate::schema::Task` in a consumer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Task {
    #[serde(skip)]
    id: String,
    title: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    epic_id: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(skip)]
    body: String,
}

/// The generated `{Entity}Frontmatter` boundary: wikilink-encodes relation
/// fields on serialize; the entity's `id`/`body` live outside frontmatter
/// (filename stem / markdown body).
#[derive(Serialize, Deserialize)]
struct TaskFrontmatter {
    title: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    epic_id: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

const TASK_FM_FIELDS: &[&str] = &["title", "status", "epic_id", "tags"];
const TASKS_DIR: &str = "tasks";

impl TaskFrontmatter {
    fn from_task(task: &Task) -> Self {
        Self {
            title: task.title.clone(),
            status: task.status.clone(),
            epic_id: task.epic_id.as_deref().map(wikilink::encode),
            tags: task.tags.iter().map(|t| wikilink::encode(t)).collect(),
        }
    }

    fn into_task(self, id: String, body: String) -> Task {
        Task {
            id,
            title: self.title,
            status: self.status,
            epic_id: wikilink::strip_opt(self.epic_id),
            tags: wikilink::strip_vec(self.tags),
            body,
        }
    }
}

/// The consumer's `Store` analog: holds the vault where the SeaORM consumer
/// holds `db`.
struct Store {
    vault: VaultHandle,
}

// ── the generated method bodies (the contract under rehearsal) ───────────

impl Store {
    fn vault(&self) -> &VaultHandle {
        &self.vault
    }

    /// Shape of generated `create_task`.
    fn create_task(&self, mut task: Task) -> Result<Task, Error> {
        // hooks::before_create(self, &mut task) — fires here, identically to SeaORM.
        let id = self.vault().make_record_id(
            TASKS_DIR,
            Some(&task.id).filter(|s| !s.is_empty()).map(String::as_str),
            Some(&task.title),
        )?;
        task.id = id.clone();
        let mut doc = Document::new();
        doc.merge_serialize(&TaskFrontmatter::from_task(&task), TASK_FM_FIELDS)?;
        doc.set_body(task.body.clone());
        self.vault().create_record(TASKS_DIR, &id, &doc)?;
        let created = self.get_task(&id)?;
        // self.emit_change(ChangeOp::Created, EntityKind::Task, id) — fires here.
        // hooks::after_create(self, &created) — fires here.
        Ok(created)
    }

    /// Shape of generated `get_task`.
    fn get_task(&self, id: &str) -> Result<Task, Error> {
        let doc = self.vault().read_record(TASKS_DIR, id)?;
        let fm: TaskFrontmatter = doc.deserialize()?;
        Ok(fm.into_task(id.to_string(), doc.body().to_string()))
    }

    /// Shape of generated `list_tasks(limit, offset)`.
    fn list_tasks(&self, limit: Option<u64>, offset: Option<u64>) -> Result<Vec<Task>, Error> {
        let mut tasks = Vec::new();
        for (id, doc) in self.vault().read_all(TASKS_DIR)? {
            let fm: TaskFrontmatter = doc.deserialize()?;
            tasks.push(fm.into_task(id, doc.body().to_string()));
        }
        let offset = offset.unwrap_or(0) as usize;
        let limit = limit.map(|l| l as usize).unwrap_or(usize::MAX);
        Ok(tasks.into_iter().skip(offset).take(limit).collect())
    }

    /// Shape of generated `update_task` (read → apply → re-render → write).
    fn update_task(&self, id: &str, new_status: &str) -> Result<Task, Error> {
        // hooks::before_update(self, &current, &updates) — after the read, before apply.
        self.vault().modify_record(TASKS_DIR, id, |doc| {
            let mut fm: TaskFrontmatter = doc.deserialize()?;
            fm.status = new_status.to_string(); // updates.apply(&mut current)
            doc.merge_serialize(&fm, TASK_FM_FIELDS)
        })?;
        let updated = self.get_task(id)?;
        // emit_change(Updated) + hooks::after_update — fire here.
        Ok(updated)
    }

    /// Shape of generated `delete_task`.
    fn delete_task(&self, id: &str) -> Result<(), Error> {
        // hooks::before_delete(self, id) — fires here.
        self.vault().remove_record(TASKS_DIR, id)?;
        // emit_change(Deleted) + hooks::after_delete — fire here.
        Ok(())
    }

    /// Shape of generated `set_task_parent` — the markdown replacement for
    /// SeaORM's raw-SQL FK fast path: read-mutate-rewrite the child.
    fn set_task_parent(&self, child_id: &str, parent_id: Option<&str>) -> Result<(), Error> {
        self.vault().modify_record(TASKS_DIR, child_id, |doc| {
            let mut fm: TaskFrontmatter = doc.deserialize()?;
            fm.epic_id = parent_id.map(wikilink::encode);
            doc.merge_serialize(&fm, TASK_FM_FIELDS)
        })
    }

    /// Shape of generated has_many reverse population: walk + filter.
    fn epic_task_ids(&self, epic_id: &str) -> Result<Vec<String>, Error> {
        let mut ids = Vec::new();
        for (id, doc) in self.vault().read_all(TASKS_DIR)? {
            let fm: TaskFrontmatter = doc.deserialize()?;
            if wikilink::strip_opt(fm.epic_id).as_deref() == Some(epic_id) {
                ids.push(id);
            }
        }
        Ok(ids)
    }
}

// ── the rehearsal ─────────────────────────────────────────────────────────

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().expect("tempdir");
    let vault = VaultHandle::new(dir.path(), VaultLayout::PerEntityDir, IdStrategy::SlugFromField("title".into()));
    (dir, Store { vault })
}

fn task(title: &str, epic: Option<&str>, tags: &[&str]) -> Task {
    Task {
        id: String::new(),
        title: title.into(),
        status: "open".into(),
        epic_id: epic.map(str::to_string),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        body: format!("Body of {title}.\n"),
    }
}

#[test]
fn full_crud_lifecycle_matches_generated_shape() {
    let (_dir, store) = store();

    // create → derived slug id, wikilinked relations on disk, typed value back.
    let created = store.create_task(task("Ship the emitter", Some("E0042"), &["codegen", "storage"])).unwrap();
    assert_eq!(created.id, "ship-the-emitter");
    assert_eq!(created.epic_id.as_deref(), Some("E0042"), "wikilink stripped on read");
    assert_eq!(created.tags, vec!["codegen", "storage"]);
    assert_eq!(created.body, "Body of Ship the emitter.\n");

    let raw = std::fs::read_to_string(store.vault().record_path(TASKS_DIR, &created.id).unwrap()).unwrap();
    assert!(raw.contains("epic_id: '[[E0042]]'"), "FK stored as wikilink: {raw}");
    assert!(raw.contains("- '[[codegen]]'"), "m2m stored as wikilink list: {raw}");

    // get
    let got = store.get_task(&created.id).unwrap();
    assert_eq!(got, created);

    // update
    let updated = store.update_task(&created.id, "closed").unwrap();
    assert_eq!(updated.status, "closed");
    assert_eq!(updated.tags, created.tags, "untouched fields survive update");

    // duplicate create with the same explicit id fails loudly
    let mut dup = task("Ship the emitter", None, &[]);
    dup.id = created.id.clone();
    assert!(matches!(store.create_task(dup), Err(Error::AlreadyExists { .. })));

    // delete → NotFound afterwards
    store.delete_task(&created.id).unwrap();
    assert!(matches!(store.get_task(&created.id), Err(Error::NotFound { .. })));
}

#[test]
fn list_pagination_and_stable_order() {
    let (_dir, store) = store();
    for title in ["Charlie", "Alpha", "Bravo", "Delta"] {
        store.create_task(task(title, None, &[])).unwrap();
    }

    let all = store.list_tasks(None, None).unwrap();
    let ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["alpha", "bravo", "charlie", "delta"], "lexicographic by id");

    let page = store.list_tasks(Some(2), Some(1)).unwrap();
    let page_ids: Vec<&str> = page.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(page_ids, vec!["bravo", "charlie"], "skip(offset).take(limit)");
}

#[test]
fn set_parent_and_reverse_walk() {
    let (_dir, store) = store();
    let a = store.create_task(task("First child", Some("E1"), &[])).unwrap();
    let b = store.create_task(task("Second child", None, &[])).unwrap();

    assert_eq!(store.epic_task_ids("E1").unwrap(), vec![a.id.clone()]);

    // Reparent b under E1; unparent a.
    store.set_task_parent(&b.id, Some("E1")).unwrap();
    store.set_task_parent(&a.id, None).unwrap();

    assert_eq!(store.epic_task_ids("E1").unwrap(), vec![b.id.clone()]);
    let a_now = store.get_task(&a.id).unwrap();
    assert_eq!(a_now.epic_id, None, "cleared FK leaves no stale frontmatter key");

    let raw = std::fs::read_to_string(store.vault().record_path(TASKS_DIR, &a.id).unwrap()).unwrap();
    assert!(!raw.contains("epic_id"), "cleared key removed from disk: {raw}");
}

#[test]
fn hand_edits_survive_generated_updates() {
    let (_dir, store) = store();
    let t = store.create_task(task("Co-edited", None, &[])).unwrap();

    // A human adds a field and edits the body in their editor.
    let path = store.vault().record_path(TASKS_DIR, &t.id).unwrap();
    let mut raw = std::fs::read_to_string(&path).unwrap();
    raw = raw.replace("status: open", "status: open\npriority: high");
    raw.push_str("\nHand-written conclusion.\n");
    std::fs::write(&path, raw).unwrap();

    // Generated update touches only its own fields...
    store.update_task(&t.id, "closed").unwrap();

    // ...and the hand edits survive.
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("priority: high"), "hand-added key survives: {after}");
    assert!(after.contains("Hand-written conclusion."), "hand-edited body survives: {after}");
    assert!(after.contains("status: closed"));
}
