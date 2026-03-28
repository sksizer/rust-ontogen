<!-- TODO: review — title and references updated from old crate names -->
# Ontogen: Detailed Walkthrough

This document walks through the ontogen codegen system end-to-end with concrete examples showing what each layer produces, how the IR (intermediate representation) flows between generators, and what the generated code looks like.

## Table of Contents

1. [The Big Picture](#the-big-picture)
2. [Stage 0: Schema Definition (Input)](#stage-0-schema-definition-input)
3. [Stage 1: Parse Schema → SchemaOutput](#stage-1-parse-schema)
4. [Stage 2: Generate Persistence (SeaORM + Markdown I/O)](#stage-2-generate-persistence)
5. [Stage 3: Generate Store → StoreOutput (DTOs + CRUD + Scaffold Hooks)](#stage-3-generate-store)
6. [Stage 4: Generate API → ApiOutput](#stage-4-generate-api)
7. [Stage 5: Generate Servers → ServersOutput](#stage-5-generate-servers)
8. [Stage 6: Generate Clients](#stage-6-generate-clients)
9. [Full build.rs Example](#full-buildrs-example)
10. [Adding a New Entity: Before and After](#adding-a-new-entity)
11. [Standalone Usage Examples](#standalone-usage)

---

## The Big Picture

The codegen system is a **pipeline of independent generators**. Each generator:

1. Takes **required inputs** (entity definitions and/or config)
2. Takes **optional upstream outputs** (enrichment from prior generators)
3. Writes **code to disk** (Rust files, TypeScript files)
4. Returns a **typed output struct** (metadata for downstream generators)

```
 ┌──────────────────────────────────────────────────────────────────────────┐
 │                        build.rs orchestration                           │
 │                                                                         │
 │  Schema/*.rs ──► parse_schema() ──► SchemaOutput                        │
 │                       │                                                 │
 │          ┌────────────┼────────────┐                                    │
 │          ▼            ▼            ▼                                    │
 │    gen_seaorm   gen_markdown_io  gen_dtos    (independent)              │
 │          │                                                              │
 │          ▼  SeaOrmOutput                                                │
 │    gen_store() ──────► StoreOutput  (DTOs + CRUD + hooks)               │
 │          │             ◄── scans custom/                                │
 │          ▼                                                              │
 │    gen_api() ────────► ApiOutput    (CRUD + scanned custom modules)     │
 │  api/v1/*.rs ──────►  ◄── scans v1/                                    │
 │          │                                                              │
 │          ▼                                                              │
 │    gen_servers() ────► ServersOutput (HTTP routes, IPC cmds, MCP tools) │
 │          │                                                              │
 │          ▼                                                              │
 │    gen_clients() ────► TypeScript, admin registry                       │
 │                        (shape derived from which servers exist)          │
 │                                                                         │
 │  Each arrow is optional. Any generator can run without upstream data.    │
 └──────────────────────────────────────────────────────────────────────────┘
```

The intermediate representations (IRs) are the `*Output` structs. They're plain Rust structs — no magic, no framework. Each one captures what the previous generator produced so the next generator can make smarter decisions.

---

## Stage 0: Schema Definition (Input)

Everything starts with a hand-written schema file. This is the **source of truth** for an entity.

### Example: `src/schema/task.rs`

```rust
use serde::{Deserialize, Serialize};
use crate::schema::common::TaskStatus;

/// A task represents a unit of work to be completed.
#[derive(Debug, Clone, Serialize, Deserialize, Default, OntologyEntity)]
#[ontology(
    entity,
    directory = "tasks",
    table = "tasks",
    type_name = "task",
    prefix = "task"
)]
pub struct Task {
    #[ontology(id)]
    pub id: String,

    pub name: String,

    pub description: Option<String>,

    #[ontology(enum_field)]
    pub status: Option<TaskStatus>,

    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    #[ontology(relation(belongs_to, target = "Agent"))]
    pub assignee_id: Option<String>,

    #[serde(default)]
    #[ontology(relation(many_to_many, target = "Requirement"))]
    pub fulfills: Vec<String>,

    #[serde(default)]
    #[ontology(relation(many_to_many, target = "Task"))]
    pub depends_on: Vec<String>,

    #[serde(default)]
    #[ontology(body)]
    pub body: String,
}
```

**What the annotations mean:**

| Annotation | Meaning |
|------------|---------|
| `#[ontology(entity)]` | This struct is a codegen-managed entity |
| `directory = "tasks"` | Markdown files live in `.ontological/tasks/` |
| `table = "tasks"` | SeaORM database table name |
| `type_name = "task"` | The `type:` field value in YAML frontmatter |
| `prefix = "task"` | IDs must start with `task-` |
| `#[ontology(id)]` | Primary key field |
| `#[ontology(body)]` | Markdown body text (not in frontmatter) |
| `#[ontology(enum_field)]` | Stored as string in DB, parsed as Rust enum |
| `#[ontology(relation(belongs_to, target = "Agent"))]` | Foreign key column pointing to Agent |
| `#[ontology(relation(many_to_many, target = "Requirement"))]` | Junction table `task_fulfills` |
| `#[ontology(relation(many_to_many, target = "Task"))]` | Self-referential junction `task_depends_on` |

---

## Stage 1: Parse Schema

```rust
let schema = parse_schema(&SchemaConfig {
    schema_dir: "src/schema".into(),
})?;
// schema.entities: Vec<EntityDef>
```

### What `parse_schema` does

1. Reads all `.rs` files in `src/schema/`
2. Uses the `syn` crate to parse Rust syntax trees
3. Finds every struct with `#[derive(OntologyEntity)]`
4. Extracts all `#[ontology(...)]` attributes
5. Classifies each field's type, role, and relation info
6. Returns `SchemaOutput { entities: Vec<EntityDef> }`

### The IR: `EntityDef`

For the Task schema above, the parser produces:

```rust
EntityDef {
    name: "Task",                        // Rust struct name
    module: "task",                      // Rust module name (snake_case)
    directory: "tasks",                  // .ontological/ subdirectory
    table: "tasks",                      // DB table name
    type_name: "task",                   // YAML type: value
    prefix: "task",                      // ID prefix

    fields: vec![
        FieldDef {
            name: "id",
            rust_type: FieldType::String,
            role: FieldRole::Id,
            relation: None,
            serde_default: false,
            annotations: {},
        },
        FieldDef {
            name: "name",
            rust_type: FieldType::String,
            role: FieldRole::Regular,
            relation: None,
            serde_default: false,
            annotations: {},
        },
        FieldDef {
            name: "description",
            rust_type: FieldType::OptionString,
            role: FieldRole::Regular,
            relation: None,
            serde_default: false,
            annotations: {},
        },
        FieldDef {
            name: "status",
            rust_type: FieldType::OptionEnum("TaskStatus"),
            role: FieldRole::EnumField,
            relation: None,
            serde_default: false,
            annotations: {},
        },
        FieldDef {
            name: "tags",
            rust_type: FieldType::VecString,
            role: FieldRole::Regular,
            relation: None,
            serde_default: true,
            annotations: {},
        },
        FieldDef {
            name: "assignee_id",
            rust_type: FieldType::OptionString,
            role: FieldRole::Regular,
            relation: Some(RelationInfo {
                kind: RelationKind::BelongsTo,
                target: "Agent",
                foreign_key: None,
                junction: None,
            }),
            serde_default: true,
            annotations: {},
        },
        FieldDef {
            name: "fulfills",
            rust_type: FieldType::VecString,
            role: FieldRole::Regular,
            relation: Some(RelationInfo {
                kind: RelationKind::ManyToMany,
                target: "Requirement",
                foreign_key: None,
                junction: None,   // defaults to "task_fulfills"
            }),
            serde_default: true,
            annotations: {},
        },
        FieldDef {
            name: "depends_on",
            rust_type: FieldType::VecString,
            role: FieldRole::Regular,
            relation: Some(RelationInfo {
                kind: RelationKind::ManyToMany,
                target: "Task",
                foreign_key: None,
                junction: None,   // defaults to "task_depends_on"
            }),
            serde_default: true,
            annotations: {},
        },
        FieldDef {
            name: "body",
            rust_type: FieldType::String,
            role: FieldRole::Body,
            relation: None,
            serde_default: true,
            annotations: {},
        },
    ],
}
```

This `EntityDef` is the **primary IR**. Every downstream generator reads it.

---

## Stage 2: Generate Persistence

The persistence layer has **two independent generators**. Neither chains into the other — they both consume `EntityDef` directly and produce building blocks.

### 2a: gen_seaorm — Database Layer

```rust
let seaorm = gen_seaorm(&schema.entities, &SeaOrmConfig {
    entity_output: "src/persistence/db/entities/generated".into(),
    conversion_output: "src/persistence/db/conversions/generated".into(),
})?;
// seaorm: SeaOrmOutput { entity_tables, junction_tables, conversion_fns }
// This output feeds into gen_store as optional enrichment.
```

### 2b: gen_markdown_io — Markdown I/O

```rust
// Always generates: parsers, writers, path helpers, fs_ops.
// Optionally generates: store event wiring (FsSubscriber + write-back guard).
gen_markdown_io(&schema.entities, &MarkdownIoConfig {
    output_dir: "src/persistence/fs_markdown/generated".into(),
    store_wiring: Some(StoreWiringConfig {
        subscriber_output: "src/persistence/fs_markdown/generated/subscriber.rs".into(),
    }),
})?;
// Returns () — no downstream IR needed.
```

### Files Generated

For our Task entity, the persistence generators produce:

#### SeaORM Entity: `persistence/db/entities/generated/task.rs`

```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: Option<String>,              // enum → string in DB
    pub tags: String,                        // Vec<String> → JSON string in DB
    pub assignee_id: Option<String>,         // belongs_to FK column
    pub body: String,
    // NOTE: fulfills and depends_on are NOT columns — they're junction tables
}
```

#### Junction Tables: `persistence/db/entities/generated/task_fulfills.rs`

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "task_fulfills")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub task_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub requirement_id: String,
}
```

(Same pattern for `task_depends_on` with `task_id` + `target_id`)

#### Conversions: `persistence/db/conversions/generated/task.rs`

```rust
impl Task {
    pub fn from_model(model: &entity::Model) -> Self {
        Self {
            id: model.id.clone(),
            name: model.name.clone(),
            description: model.description.clone(),
            status: model.status.as_ref().and_then(|s| {
                serde_json::from_str::<TaskStatus>(&format!("\"{s}\"")).ok()
            }),
            tags: decode_json_vec(&model.tags),
            assignee_id: model.assignee_id.clone(),
            fulfills: Vec::new(),      // populated later by store
            depends_on: Vec::new(),    // populated later by store
            body: model.body.clone(),
        }
    }

    pub fn to_active_model(&self) -> entity::ActiveModel {
        entity::ActiveModel {
            id: Set(self.id.clone()),
            name: Set(self.name.clone()),
            description: Set(self.description.clone()),
            status: Set(self.status.as_ref().map(|e| enum_to_string(e))),
            tags: Set(serde_json::to_string(&self.tags).unwrap_or_default()),
            assignee_id: Set(self.assignee_id.clone()),
            body: Set(self.body.clone()),
        }
    }
}
```

#### Markdown Writer: `persistence/fs_markdown/generated/task.rs`

```rust
pub fn render_task(task: &Task) -> String {
    let mut yaml = String::from("---\ntype: task\n");
    yaml.push_str(&format!("id: {}\n", task.id));
    yaml.push_str(&format!("name: {}\n", yaml_escape(&task.name)));
    if let Some(ref desc) = task.description {
        yaml.push_str(&format!("description: {}\n", yaml_escape(desc)));
    }
    if let Some(ref status) = task.status {
        yaml.push_str(&format!("status: {}\n", enum_to_string(status)));
    }
    if !task.tags.is_empty() {
        yaml.push_str(&format!("tags: [{}]\n", task.tags.join(", ")));
    }
    if let Some(ref assignee) = task.assignee_id {
        yaml.push_str(&format!("assignee_id: {}\n", assignee));
    }
    // Relation fields rendered as wikilinks
    if !task.fulfills.is_empty() {
        let links: Vec<String> = task.fulfills.iter().map(|id| format!("[[{}]]", id)).collect();
        yaml.push_str(&format!("fulfills: [{}]\n", links.join(", ")));
    }
    if !task.depends_on.is_empty() {
        let links: Vec<String> = task.depends_on.iter().map(|id| format!("[[{}]]", id)).collect();
        yaml.push_str(&format!("depends_on: [{}]\n", links.join(", ")));
    }
    yaml.push_str("---\n");
    if !task.body.is_empty() {
        yaml.push_str(&task.body);
        if !task.body.ends_with('\n') { yaml.push('\n'); }
    }
    yaml
}
```

#### Store Event Wiring (opt-in): `persistence/fs_markdown/generated/subscriber.rs`

When `store_wiring` is configured, `gen_markdown_io` also generates an `FsSubscriber` that listens for store `EntityChange` events and writes markdown files. The pattern is mechanical across all entities — match on `EntityKind`, call the generated `write_*()` function, guard against write-back loops:

```rust
// Generated (opt-in via store_wiring config)
use crate::types::{EntityChange, ChangeOp, EntityKind};
use crate::persistence::fs_markdown::generated::fs_ops::*;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

pub struct FsSubscriber {
    project_root: PathBuf,
    write_guard: Arc<Mutex<HashSet<PathBuf>>>,
}

impl FsSubscriber {
    pub fn new(project_root: PathBuf, write_guard: Arc<Mutex<HashSet<PathBuf>>>) -> Self {
        Self { project_root, write_guard }
    }

    /// Handle an entity change event. Writes the entity to markdown,
    /// guarding against write-back loops from the file watcher.
    pub async fn handle_change(&self, change: &EntityChange, store: &Store) {
        if change.op == ChangeOp::Deleted {
            // Delete the markdown file
            let path = self.entity_path(&change.entity_kind, &change.entity_id);
            if let Some(path) = path {
                let _ = std::fs::remove_file(&path);
            }
            return;
        }

        // Fetch the full entity and write to markdown
        match change.entity_kind {
            EntityKind::Node => {
                if let Ok(entity) = store.get_node(&change.entity_id).await {
                    let path = node_path(&self.project_root, &entity.id);
                    self.guarded_write(&path, || write_node(&self.project_root, &entity)).await;
                }
            }
            EntityKind::Task => {
                if let Ok(entity) = store.get_task(&change.entity_id).await {
                    let path = task_path(&self.project_root, &entity.id);
                    self.guarded_write(&path, || write_task(&self.project_root, &entity)).await;
                }
            }
            EntityKind::Requirement => { /* same pattern */ }
            // ... one arm per entity
        }
    }

    /// Write to file, adding path to write_guard so the file watcher
    /// ignores the change we just caused.
    async fn guarded_write(&self, path: &PathBuf, write_fn: impl FnOnce() -> std::io::Result<()>) {
        {
            let mut guard = self.write_guard.lock().await;
            guard.insert(path.clone());
        }
        let _ = write_fn();
    }

    fn entity_path(&self, kind: &EntityKind, id: &str) -> Option<PathBuf> {
        match kind {
            EntityKind::Node => Some(node_path(&self.project_root, id)),
            EntityKind::Task => Some(task_path(&self.project_root, id)),
            // ... one arm per entity
            _ => None,
        }
    }
}
```

You're still free to hand-write this subscriber if you need custom behavior (batching, debouncing, project-scoped paths, etc.) — just set `store_wiring: None` and the building blocks are still generated for you to call directly.

### The IR: `SeaOrmOutput`

Only `gen_seaorm` produces a downstream IR. `gen_markdown_io` returns `()` — it writes files to disk but produces no metadata for downstream generators.

```rust
SeaOrmOutput {
    entity_tables: vec![
        EntityTableMeta {
            entity_name: "Task",
            table_name: "tasks",
            columns: vec![
                ColumnMeta { name: "id", col_type: "String", is_pk: true },
                ColumnMeta { name: "name", col_type: "String", is_pk: false },
                ColumnMeta { name: "description", col_type: "Option<String>", is_pk: false },
                ColumnMeta { name: "status", col_type: "Option<String>", is_pk: false },
                ColumnMeta { name: "tags", col_type: "String", is_pk: false },       // JSON
                ColumnMeta { name: "assignee_id", col_type: "Option<String>", is_pk: false },
                ColumnMeta { name: "body", col_type: "String", is_pk: false },
            ],
        },
        // ... other entities
    ],
    junction_tables: vec![
        JunctionMeta {
            table_name: "task_fulfills",
            source_entity: "Task",
            source_col: "task_id",
            target_entity: "Requirement",
            target_col: "requirement_id",
            source_field: "fulfills",       // which field on Task this populates
        },
        JunctionMeta {
            table_name: "task_depends_on",
            source_entity: "Task",
            source_col: "task_id",
            target_entity: "Task",
            target_col: "target_id",
            source_field: "depends_on",
        },
    ],
    conversion_fns: vec![
        ConversionMeta {
            entity_name: "Task",
            from_model_fn: "Task::from_model",
            to_active_model_fn: "Task::to_active_model",
        },
    ],
}
```

**Why this IR matters:** The store generator can use `junction_tables` to know exactly which `sync_junction()` calls to emit. Without this IR, it would have to re-derive junction info from the schema annotations — possible, but less precise (e.g., the table name might have been overridden).

---

## Stage 3: Generate Store

The store generator produces **everything the store layer needs**: DTOs, update structs, From conversions, CRUD methods, scaffold hook files, relation population, and change channels. DTOs live here because the store is where they're consumed.

```rust
let store = gen_store(
    &schema.entities,
    Some(&seaorm),                   // optional: enriched with exact table/column names
    &["src/store/custom"],           // scan for hand-written store methods
    &StoreConfig {
        output_dir: "src/store/generated".into(),
        hooks_dir: "src/store/hooks".into(),     // scaffold stubs here (once, never overwrites)
        dto_output: "src/schema/dto".into(),     // DTOs generated as part of store
        event_emission: true,
        change_channels: true,
        entity_overrides: vec![],
    },
)?;
```

### Files Generated

#### DTOs: `schema/dto/task.rs`

Generated by `gen_store` (because the store is what consumes them). Also available standalone via `gen_dtos()` for consumers who want input types without a full store.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Type)]  // Type = specta
pub struct CreateTaskInput {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub tags: Vec<String>,
    pub assignee_id: Option<String>,
    pub fulfills: Vec<String>,
    pub depends_on: Vec<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UpdateTaskInput {
    pub name: Option<String>,
    pub description: Option<Option<String>>,     // Option<Option<T>> for nullable clearing
    pub status: Option<Option<TaskStatus>>,
    pub tags: Option<Vec<String>>,
    pub assignee_id: Option<Option<String>>,
    pub fulfills: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
    pub body: Option<String>,
}
```

#### Scaffold Hooks: `store/hooks/task.rs` (generated once, never overwritten)

```rust
// SCAFFOLDED — fill in as needed. This file is yours; codegen will not overwrite it.
use crate::schema::Task;
use crate::store::Store;
use crate::store::generated::task::TaskUpdate;
use crate::types::AppError;

pub async fn before_create(_store: &Store, _task: &mut Task) -> Result<(), AppError> {
    Ok(())
}

pub async fn after_create(_store: &Store, _task: &Task) -> Result<(), AppError> {
    Ok(())
}

pub async fn before_update(_store: &Store, _current: &Task, _updates: &TaskUpdate) -> Result<(), AppError> {
    Ok(())
}

pub async fn after_update(_store: &Store, _task: &Task) -> Result<(), AppError> {
    Ok(())
}

pub async fn before_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}

pub async fn after_delete(_store: &Store, _id: &str) -> Result<(), AppError> {
    Ok(())
}
```

#### Store CRUD: `store/generated/task.rs`

```rust
use sea_orm::*;
use crate::schema::{Task, TaskStatus};
use crate::schema::dto::{CreateTaskInput, UpdateTaskInput};
use crate::persistence::db::entities::generated::task as entity;
use crate::store::Store;
use crate::store::hooks::task as hooks;
use crate::types::{AppError, ChangeOp, EntityKind};

// ── Update Struct ──────────────────────────────────────────────────

/// Partial update for Task. None = "don't change this field".
/// Option<Option<T>> = outer None means "don't change", Some(None) means "clear".
pub struct TaskUpdate {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub status: Option<Option<TaskStatus>>,
    pub tags: Option<Vec<String>>,
    pub assignee_id: Option<Option<String>>,
    pub fulfills: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
    pub body: Option<String>,
}

impl TaskUpdate {
    /// Apply non-None fields from this update onto the given task.
    pub fn apply(&self, task: &mut Task) {
        if let Some(ref v) = self.name { task.name.clone_from(v); }
        if let Some(ref v) = self.description { task.description.clone_from(v); }
        if let Some(ref v) = self.status { task.status.clone_from(v); }
        if let Some(ref v) = self.tags { task.tags.clone_from(v); }
        if let Some(ref v) = self.assignee_id { task.assignee_id.clone_from(v); }
        if let Some(ref v) = self.fulfills { task.fulfills.clone_from(v); }
        if let Some(ref v) = self.depends_on { task.depends_on.clone_from(v); }
        if let Some(ref v) = self.body { task.body.clone_from(v); }
    }
}

impl From<UpdateTaskInput> for TaskUpdate {
    fn from(input: UpdateTaskInput) -> Self {
        Self {
            name: input.name,
            description: input.description,
            status: input.status,
            tags: input.tags,
            assignee_id: input.assignee_id,
            fulfills: input.fulfills.map(|v| strip_wikilinks_vec(v)),
            depends_on: input.depends_on.map(|v| strip_wikilinks_vec(v)),
            body: input.body,
        }
    }
}

impl From<CreateTaskInput> for Task {
    fn from(input: CreateTaskInput) -> Self {
        Self {
            id: input.id,
            name: input.name,
            description: input.description,
            status: input.status,
            tags: input.tags,
            assignee_id: input.assignee_id,
            // Relation fields: strip wikilinks automatically
            fulfills: strip_wikilinks_vec(input.fulfills),
            depends_on: strip_wikilinks_vec(input.depends_on),
            body: input.body,
            // Persistence-only fields get defaults
            ..Default::default()
        }
    }
}

// ── CRUD Methods ───────────────────────────────────────────────────

impl Store {
    /// List all tasks.
    pub async fn list_tasks(&self) -> Result<Vec<Task>, AppError> {
        let _span = tracing::info_span!("store.list", entity = "task").entered();

        let models = entity::Entity::find().all(self.db()).await?;
        let mut tasks: Vec<Task> = models.iter().map(Task::from_model).collect();
        for task in &mut tasks {
            self.populate_task_relations(task).await?;
        }
        Ok(tasks)
    }

    /// Get a single task by ID.
    pub async fn get_task(&self, id: &str) -> Result<Task, AppError> {
        let _span = tracing::info_span!("store.get", entity = "task", id = %id).entered();

        let model = entity::Entity::find_by_id(id)
            .one(self.db())
            .await?
            .ok_or_else(|| AppError::not_found("Task", id))?;
        let mut task = Task::from_model(&model);
        self.populate_task_relations(&mut task).await?;
        Ok(task)
    }

    /// Create a new task.
    pub async fn create_task(&self, mut task: Task) -> Result<Task, AppError> {
        let _span = tracing::info_span!("store.create", entity = "task", id = %task.id).entered();

        // Hook: before_create
        hooks::before_create(self, &mut task).await?;

        let id = task.id.clone();

        // Save relation field values before consuming into active model
        let fulfills = task.fulfills.clone();
        let depends_on = task.depends_on.clone();

        // Insert main entity
        let active = task.to_active_model();
        active.insert(self.db()).await?;

        // Sync junction tables (derived from schema relation metadata)
        // From: #[ontology(relation(many_to_many, target = "Requirement"))]
        self.sync_junction("task_fulfills", "task_id", "requirement_id", &id, &fulfills).await?;
        // From: #[ontology(relation(many_to_many, target = "Task"))]
        self.sync_junction("task_depends_on", "task_id", "target_id", &id, &depends_on).await?;

        // Emit change event
        self.emit_change(ChangeOp::Created, EntityKind::Task, id.clone());

        // Re-fetch with populated relations
        let result = self.get_task(&id).await?;

        // Hook: after_create
        hooks::after_create(self, &result).await?;

        Ok(result)
    }

    /// Update an existing task.
    pub async fn update_task(&self, id: &str, updates: TaskUpdate) -> Result<Task, AppError> {
        let _span = tracing::info_span!("store.update", entity = "task", id = %id).entered();

        // Fetch current state
        let mut task = self.get_task(id).await?;

        // Hook: before_update
        hooks::before_update(self, &task, &updates).await?;

        // Track which junction fields changed
        let fulfills_changed = updates.fulfills.is_some();
        let depends_on_changed = updates.depends_on.is_some();

        // Apply partial updates
        updates.apply(&mut task);

        // Update main entity
        let active = task.to_active_model();
        entity::Entity::update(active).exec(self.db()).await?;

        // Re-sync junction tables only if changed
        if fulfills_changed {
            self.sync_junction(
                "task_fulfills", "task_id", "requirement_id",
                id, &task.fulfills,
            ).await?;
        }
        if depends_on_changed {
            self.sync_junction(
                "task_depends_on", "task_id", "target_id",
                id, &task.depends_on,
            ).await?;
        }

        // Emit change event
        self.emit_change(ChangeOp::Updated, EntityKind::Task, id.to_string());

        // Re-fetch with populated relations
        let result = self.get_task(id).await?;

        // Hook: after_update
        hooks::after_update(self, &result).await?;

        Ok(result)
    }

    /// Delete a task by ID.
    pub async fn delete_task(&self, id: &str) -> Result<(), AppError> {
        let _span = tracing::info_span!("store.delete", entity = "task", id = %id).entered();

        // Hook: before_delete
        hooks::before_delete(self, id).await?;

        let result = entity::Entity::delete_by_id(id).exec(self.db()).await?;
        if result.rows_affected == 0 {
            return Err(AppError::not_found("Task", id));
        }

        // Emit change event
        self.emit_change(ChangeOp::Deleted, EntityKind::Task, id.to_string());

        // Hook: after_delete
        hooks::after_delete(self, id).await?;

        Ok(())
    }

    /// Populate junction-backed relation fields on a task.
    async fn populate_task_relations(&self, task: &mut Task) -> Result<(), AppError> {
        // From: #[ontology(relation(many_to_many, target = "Requirement"))]
        task.fulfills = self.load_junction_ids(
            "task_fulfills", "task_id", "requirement_id", &task.id
        ).await?;

        // From: #[ontology(relation(many_to_many, target = "Task"))]
        task.depends_on = self.load_junction_ids(
            "task_depends_on", "task_id", "target_id", &task.id
        ).await?;

        Ok(())
    }
}
```

#### Change Channel (when `change_channels: true`): `store/generated/channels.rs`

```rust
use tokio::sync::broadcast;
use crate::schema::*;
use crate::types::ChangeOp;

/// Typed change event carrying the full entity payload.
#[derive(Debug, Clone)]
pub struct EntityChange<T: Clone> {
    pub op: ChangeOp,
    pub entity: Option<T>,  // Some for Create/Update, None for Delete
    pub id: String,
}

/// Per-entity broadcast channels.
pub struct EntityChannels {
    pub task: broadcast::Sender<EntityChange<Task>>,
    pub node: broadcast::Sender<EntityChange<Node>>,
    pub requirement: broadcast::Sender<EntityChange<Requirement>>,
    pub specification: broadcast::Sender<EntityChange<Specification>>,
    pub contract: broadcast::Sender<EntityChange<Contract>>,
    pub agent: broadcast::Sender<EntityChange<Agent>>,
    pub role: broadcast::Sender<EntityChange<Role>>,
    pub work_session: broadcast::Sender<EntityChange<WorkSession>>,
    pub evidence: broadcast::Sender<EntityChange<Evidence>>,
    pub relation: broadcast::Sender<EntityChange<Relation>>,
}

impl EntityChannels {
    pub fn new(capacity: usize) -> Self {
        Self {
            task: broadcast::channel(capacity).0,
            node: broadcast::channel(capacity).0,
            requirement: broadcast::channel(capacity).0,
            specification: broadcast::channel(capacity).0,
            contract: broadcast::channel(capacity).0,
            agent: broadcast::channel(capacity).0,
            role: broadcast::channel(capacity).0,
            work_session: broadcast::channel(capacity).0,
            evidence: broadcast::channel(capacity).0,
            relation: broadcast::channel(capacity).0,
        }
    }
}

impl Store {
    /// Subscribe to Task change events.
    pub fn task_changes(&self) -> broadcast::Receiver<EntityChange<Task>> {
        self.channels.task.subscribe()
    }

    /// Subscribe to Node change events.
    pub fn node_changes(&self) -> broadcast::Receiver<EntityChange<Node>> {
        self.channels.node.subscribe()
    }

    // ... one method per entity
}
```

### The IR: `StoreOutput`

```rust
StoreOutput {
    store_methods: vec![
        StoreMethodMeta {
            entity_name: "Task",
            methods: vec![
                MethodMeta { name: "list_tasks", kind: CrudOp::List, returns: "Vec<Task>" },
                MethodMeta { name: "get_task", kind: CrudOp::Get, params: vec![("id", "&str")] },
                MethodMeta { name: "create_task", kind: CrudOp::Create, params: vec![("task", "Task")] },
                MethodMeta { name: "update_task", kind: CrudOp::Update, params: vec![("id", "&str"), ("updates", "TaskUpdate")] },
                MethodMeta { name: "delete_task", kind: CrudOp::Delete, params: vec![("id", "&str")] },
            ],
        },
        // ... other entities
    ],
    scaffolded_hooks: vec![
        ScaffoldMeta {
            entity_name: "Task",
            file_path: "src/store/hooks/task.rs",
            functions: vec!["before_create", "after_create", "before_update", "after_update", "before_delete", "after_delete"],
        },
    ],
    change_channels: vec![
        ChannelMeta {
            entity_name: "Task",
            subscribe_method: "task_changes",
            event_type: "EntityChange<Task>",
        },
    ],
}
```

**Why this IR matters:** The API generator knows the exact store method names and signatures. It doesn't have to guess that `list_tasks()` exists — it sees it in `StoreOutput`.

### Store: Generated vs Custom (Same Split as API)

The store layer has **three parallel directories**: generated CRUD, scaffolded hooks, and custom methods.

```
src/store/
├── generated/                 # ◄ Generated by gen_store (always regenerated)
│   ├── mod.rs
│   ├── task.rs                #   CRUD (imports hooks::task), TaskUpdate, From<Input>, populate_relations
│   ├── node.rs                #   CRUD (imports hooks::node), NodeUpdate, From<Input>, populate_relations
│   ├── agent.rs               #   CRUD (imports hooks::agent — all no-ops)
│   ├── channels.rs            #   EntityChannels, typed subscribe methods
│   └── ...
├── hooks/                     # ◄ Scaffolded once, never overwritten — fill in as needed
│   ├── mod.rs
│   ├── task.rs                #   before_create, after_create, ... (no-op stubs)
│   ├── node.rs                #   before_create (containment validation), ...
│   ├── requirement.rs         #   before_update (status transition validation), ...
│   └── ...
├── custom/                    # ◄ Hand-written: custom store methods beyond CRUD
│   ├── mod.rs
│   └── node_custom.rs         #   Custom store methods: bulk_reparent_nodes(), etc.
├── mod.rs                     # Hand-written: re-exports generated + hooks + custom
└── store.rs                   # Hand-written: Store struct, sync_junction, helpers
```

**Custom store methods** are additional `impl Store` blocks that live alongside generated ones:

```rust
// Hand-written: src/store/custom/node_custom.rs
impl Store {
    /// Custom method — not CRUD, can't be generated from schema.
    /// Uses generated methods internally.
    pub async fn bulk_reparent_nodes(
        &self,
        node_ids: &[String],
        new_parent_id: &str,
    ) -> Result<Vec<Node>, AppError> {
        let mut results = Vec::new();
        for id in node_ids {
            // Calls the GENERATED update_node() internally
            let updates = NodeUpdate {
                parent_id: Some(Some(new_parent_id.to_string())),
                ..Default::default()
            };
            results.push(self.update_node(id, &updates).await?);
        }
        Ok(results)
    }
}
```

This method can then be called from a custom API function in `api/v1/node.rs`:

```rust
// Hand-written: src/api/v1/node.rs
pub async fn bulk_reparent(
    store: &Store,
    input: BulkReparentInput,
) -> Result<Vec<Node>, AppError> {
    store.bulk_reparent_nodes(&input.node_ids, &input.new_parent_id).await
}
```

Which the transport generator discovers via scan and merges with the generated CRUD. The full chain:

```
Schema ──► gen_store ──► store/generated/node.rs (CRUD, imports hooks::node)
                         store/hooks/node.rs (containment validation) ◄── scaffolded, filled in
                         store/custom/node_custom.rs (bulk_reparent_nodes) ◄── hand-written
                              │
                              ▼
           gen_api ───► api/v1/generated/node.rs (list, get, create, update, delete)
                        api/v1/node.rs (bulk_reparent) ◄── hand-written, calls custom store method
                              │
                              ▼ (merged into single ApiModule)
           gen_servers ──► HTTP/IPC/MCP handlers for all 6 functions
                              ↓
           gen_clients ──► TypeScript client with all 6 methods
```

### Custom Entities with CRUD

Not every entity needs to come from the schema. If you have a `Widget` backed by an external API or a custom storage strategy, you can write CRUD methods in `store/custom/` and the scanner will recognize them by naming convention:

```rust
// store/custom/widget.rs — hand-written, not schema-derived
impl Store {
    pub async fn list_widgets(&self) -> Result<Vec<Widget>, AppError> { ... }
    pub async fn get_widget(&self, id: &str) -> Result<Widget, AppError> { ... }
    pub async fn create_widget(&self, widget: Widget) -> Result<Widget, AppError> { ... }
    pub async fn update_widget(&self, id: &str, updates: WidgetUpdate) -> Result<Widget, AppError> { ... }
    pub async fn delete_widget(&self, id: &str) -> Result<(), AppError> { ... }
}
```

The scanner produces `StoreMethodMeta` entries with `entity_name: "Widget"` and `kind: Crud(Create)`, etc. — inferred from the `create_widget` naming convention. From there, the pipeline treats Widget identically to a schema entity:

```
(no schema) ──► scanner ──► StoreMethodMeta { entity: "Widget", kind: Crud(*) }
                                 │
                                 ▼
               gen_api ───► api/v1/generated/widget.rs (CRUD forwarding)
                                 │
                                 ▼
               gen_servers ──► GET /api/v1/widgets
                               GET /api/v1/widgets/:id
                               POST /api/v1/widgets
                               PUT /api/v1/widgets/:id
                               DELETE /api/v1/widgets/:id
                                 ↓
               gen_clients ──► TypeScript: widgetApi.list(), .get(), .create(), ...
```

For methods where the naming convention doesn't apply, annotate explicitly:

```rust
#[ontology(entity = "Widget", crud = "list")]
pub async fn fetch_all_widgets(&self) -> Result<Vec<Widget>, AppError> { ... }
```

---

## Stage 4: Generate API

```rust
let api = gen_api(
    &schema.entities,       // for generating CRUD forwarding
    Some(&store),           // knows exact store method names
    &["src/api/v1"],        // scan for hand-written custom modules
    &ApiConfig {
        generated_output: "src/api/v1/generated".into(),
        versions: vec!["v1".into()],
        exclude: vec![],
    },
)?;
```

### What gen_api Does

1. **For each entity in `schema.entities`** (unless excluded): generates a CRUD forwarding module in `api/v1/generated/`
2. **Scans `src/api/v1/`** for hand-written modules (graph.rs, events.rs, project.rs, etc.)
3. **Merges** both sources into a single `Vec<ApiModule>` in the output

### Files Generated

#### Generated CRUD: `api/v1/generated/task.rs`

```rust
use crate::store::Store;
use crate::schema::Task;
use crate::schema::dto::{CreateTaskInput, UpdateTaskInput};
use crate::store::generated::task::TaskUpdate;
use crate::types::AppError;

pub async fn list(store: &Store) -> Result<Vec<Task>, AppError> {
    store.list_tasks().await
}

pub async fn get_by_id(store: &Store, id: &str) -> Result<Task, AppError> {
    store.get_task(id).await
}

pub async fn create(store: &Store, input: CreateTaskInput) -> Result<Task, AppError> {
    let task: Task = input.into();
    store.create_task(task).await
}

pub async fn update(store: &Store, id: &str, input: UpdateTaskInput) -> Result<Task, AppError> {
    let updates: TaskUpdate = input.into();
    store.update_task(id, updates).await
}

pub async fn delete(store: &Store, id: &str) -> Result<(), AppError> {
    store.delete_task(id).await
}
```

#### Generated mod.rs: `api/v1/generated/mod.rs`

```rust
pub mod task;
pub mod node;
pub mod requirement;
pub mod specification;
pub mod contract;
pub mod agent;
pub mod role;
pub mod work_session;
pub mod evidence;
pub mod relation;
```

### The Merge: Generated + Scanned

After generation, `gen_api` scans `src/api/v1/` and finds hand-written modules:

```
src/api/v1/
├── graph.rs           ← scanned: get_graph_snapshot, get_node_detail
├── events.rs          ← scanned: graph_updated, entity_changed
├── project.rs         ← scanned: switch_project, open_project, close_project, ...
├── status.rs          ← scanned: get_status_transitions
├── entity_counts.rs   ← scanned: get_entity_counts
├── path_opener.rs     ← scanned: detect_installed_openers, open_path_with_app, ...
└── settings.rs        ← scanned: get_app_info, get_dev_info
```

The resulting `ApiOutput.modules` contains **all** of them:

```rust
ApiOutput {
    modules: vec![
        // ── Generated CRUD (from entity metadata) ──
        ApiModule { name: "task", source: Generated, fns: [list, get_by_id, create, update, delete] },
        ApiModule { name: "node", source: Generated, fns: [list, get_by_id, create, update, delete] },
        ApiModule { name: "requirement", source: Generated, fns: [list, get_by_id, create, update, delete] },
        // ... more entities

        // ── Scanned custom (from source files) ──
        ApiModule { name: "graph", source: Scanned, fns: [get_graph_snapshot, get_node_detail] },
        ApiModule { name: "events", source: Scanned, fns: [graph_updated, entity_changed] },
        ApiModule { name: "project", source: Scanned, fns: [switch_project, open_project, ...] },
        ApiModule { name: "status", source: Scanned, fns: [get_status_transitions] },
        ApiModule { name: "entity_counts", source: Scanned, fns: [get_entity_counts] },
        ApiModule { name: "path_opener", source: Scanned, fns: [detect_installed_openers, ...] },
        ApiModule { name: "settings", source: Scanned, fns: [get_app_info, get_dev_info] },
    ],
}
```

### Merge Example: Adding a Custom Function to a Generated Entity

Suppose you add a custom `archive_task()` function alongside generated CRUD.

**Hand-written:** `src/api/v1/task.rs`

```rust
// Custom function — NOT generated, lives alongside generated CRUD
use crate::store::Store;
use crate::schema::Task;
use crate::types::AppError;

pub async fn archive(store: &Store, id: &str) -> Result<Task, AppError> {
    // Custom archival logic
    let mut task = store.get_task(id).await?;
    task.status = Some(TaskStatus::Archived);
    store.update_task(id, /* ... */).await
}
```

After merge, the `task` module in `ApiOutput` contains **6 functions**:

```rust
ApiModule {
    name: "task",
    fns: [
        // From generated:
        ApiFn { name: "list", source: Generated, ... },
        ApiFn { name: "get_by_id", source: Generated, ... },
        ApiFn { name: "create", source: Generated, ... },
        ApiFn { name: "update", source: Generated, ... },
        ApiFn { name: "delete", source: Generated, ... },
        // From scan:
        ApiFn { name: "archive", source: Scanned, ... },
    ],
}
```

The transport generator sees all 6 and generates handlers for all of them.

---

## Stage 5: Generate Servers

```rust
let servers = gen_servers(&api, Some(&schema.entities), &ServersConfig {
    scoping: Some(ScopingConfig {
        param_name: "project_id",
        rust_type: "uuid::Uuid",
        ts_type: "string",
        state_accessor: "store_for",
    }),
    http: Some(HttpConfig {
        output: "src/api/transport/http/generated.rs".into(),
        route_prefix: "projects/:project_id",
    }),
    ipc: Some(IpcConfig {
        output: "src/api/transport/ipc/generated.rs".into(),
    }),
    mcp: Some(McpConfig {
        output: "src/api/transport/mcp/generated.rs".into(),
    }),
})?;
```

### What gen_servers Does

Iterates over every `ApiModule` in `ApiOutput` and generates server-side handlers for each configured transport. Returns `ServersOutput` describing the concrete endpoints created.

**Key behavior:** For each function, it checks `ApiFn.source` to determine the correct `use` import path:

- `Generated` → `use crate::api::v1::generated::task;`
- `Scanned` → `use crate::api::v1::task;`

Each transport handles project scoping differently based on `ScopingConfig`:
- **HTTP**: URL path prefix (`/projects/:project_id/tasks`)
- **IPC**: Optional camelCase param (`projectId?: string`)
- **MCP**: Injected into tool JSON schema

### Files Generated (Task excerpt)

#### HTTP: `api/transport/http/generated.rs` (excerpt)

```rust
// Task routes — generated + custom merged
pub fn task_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list_tasks))
        .route("/task/:id", get(get_task).put(update_task).delete(delete_task))
        .route("/task", post(create_task))
        .route("/task/:id/archive", post(archive_task))  // from scanned custom fn
}

async fn list_tasks(
    Path(project_id): Path<uuid::Uuid>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Task>>, (StatusCode, Json<ErrorResponse>)> {
    let store = state.store_for(&project_id)?;
    // Generated source → import from api::v1::generated
    api::v1::generated::task::list(&store).await.map(Json).map_err(into_error)
}

async fn archive_task(
    Path((project_id, id)): Path<(uuid::Uuid, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Task>, (StatusCode, Json<ErrorResponse>)> {
    let store = state.store_for(&project_id)?;
    // Scanned source → import from api::v1
    api::v1::task::archive(&store, &id).await.map(Json).map_err(into_error)
}
```

#### IPC: `api/transport/ipc/generated.rs` (excerpt)

```rust
#[tauri::command]
#[serde(rename_all = "camelCase")]
pub async fn list_tasks(
    project_id: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Task>, String> {
    let store = resolve_store(&state, project_id.as_deref()).await?;
    api::v1::generated::task::list(&store).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[serde(rename_all = "camelCase")]
pub async fn archive_task(
    id: String,
    project_id: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Task, String> {
    let store = resolve_store(&state, project_id.as_deref()).await?;
    api::v1::task::archive(&store, &id).await.map_err(|e| e.to_string())
}
```

### The IR: `ServersOutput`

```rust
ServersOutput {
    http_routes: vec![
        HttpRouteMeta { method: "GET", path: "/projects/:project_id/tasks", handler: "list_tasks", module: "task" },
        HttpRouteMeta { method: "GET", path: "/projects/:project_id/task/:id", handler: "get_task", module: "task" },
        HttpRouteMeta { method: "POST", path: "/projects/:project_id/task", handler: "create_task", module: "task" },
        HttpRouteMeta { method: "PUT", path: "/projects/:project_id/task/:id", handler: "update_task", module: "task" },
        HttpRouteMeta { method: "DELETE", path: "/projects/:project_id/task/:id", handler: "delete_task", module: "task" },
        HttpRouteMeta { method: "POST", path: "/projects/:project_id/task/:id/archive", handler: "archive_task", module: "task" },
        // ... all other entities + custom modules
    ],
    ipc_commands: vec![
        IpcCommandMeta { name: "list_tasks", params: vec![("projectId", "Option<String>")], module: "task" },
        IpcCommandMeta { name: "create_task", params: vec![("input", "CreateTaskInput"), ("projectId", "Option<String>")], module: "task" },
        IpcCommandMeta { name: "archive_task", params: vec![("id", "String"), ("projectId", "Option<String>")], module: "task" },
        // ...
    ],
    mcp_tools: vec![
        McpToolMeta { name: "list_tasks", schema: /* JSON Schema */, module: "task" },
        // ...
    ],
}
```

**Why this IR matters:** The client generator reads `ServersOutput` to know exactly which endpoints exist on which protocols. It doesn't guess — it mirrors what was actually generated.

---

## Stage 6: Generate Clients

```rust
gen_clients(&servers, Some(&schema.entities), &ClientsConfig {
    typescript: Some(TypeScriptConfig {
        output: "src-nuxt/app/transport/generated.ts".into(),
        bindings_path: "src-nuxt/app/types/bindings.ts".into(),
    }),
    admin_registry: Some(AdminRegistryConfig {
        output: "src-nuxt/layers/admin/generated/admin-registry.ts".into(),
    }),
})?;
```

### What gen_clients Does

Reads `ServersOutput` and generates client libraries that mirror the server endpoints.

**Key behavior:** The TypeScript client's shape is **derived from which servers exist**:

- `servers.http_routes` is non-empty → generates `createHttpTransport()` with fetch calls
- `servers.ipc_commands` is non-empty → generates `createIpcTransport()` with Tauri `invoke()` calls
- Both exist → generates a unified `Transport` interface + automatic runtime switching

### Files Generated

#### TypeScript: `src-nuxt/app/transport/generated.ts` (excerpt)

```typescript
// ── Unified interface (always generated) ───────────────────────
export interface Transport {
    // Generated from ServersOutput — every endpoint across all servers
    listTasks(projectId?: string): Promise<Task[]>;
    getTaskById(id: string, projectId?: string): Promise<Task>;
    createTask(input: CreateTaskInput, projectId?: string): Promise<Task>;
    updateTask(id: string, input: UpdateTaskInput, projectId?: string): Promise<Task>;
    deleteTask(id: string, projectId?: string): Promise<void>;
    archiveTask(id: string, projectId?: string): Promise<Task>;
    // ... all other entities and custom modules
}

// ── HTTP transport (generated because servers.http_routes is non-empty) ──
export function createHttpTransport(baseUrl: string): Transport {
    return {
        // Mirrors HttpRouteMeta — uses the actual route paths
        listTasks: (projectId) =>
            fetch(`${baseUrl}/projects/${projectId}/tasks`).then(r => r.json()),
        createTask: (input, projectId) =>
            fetch(`${baseUrl}/projects/${projectId}/task`, {
                method: 'POST', body: JSON.stringify(input),
            }).then(r => r.json()),
        archiveTask: (id, projectId) =>
            fetch(`${baseUrl}/projects/${projectId}/task/${id}/archive`, {
                method: 'POST',
            }).then(r => r.json()),
        // ...
    };
}

// ── IPC transport (generated because servers.ipc_commands is non-empty) ──
export function createIpcTransport(): Transport {
    return {
        // Mirrors IpcCommandMeta — uses the actual command names + camelCase params
        listTasks: (projectId) =>
            invoke('list_tasks', { projectId }),
        createTask: (input, projectId) =>
            invoke('create_task', { input, projectId }),
        archiveTask: (id, projectId) =>
            invoke('archive_task', { id, projectId }),
        // ...
    };
}
```

If only HTTP was configured (no IPC), `createIpcTransport()` would not be generated at all — no dead code.

---

## Full build.rs Example

```rust
// src-tauri/build.rs

use ontogen::*;

fn main() {
    // ── Stage 1: Parse schemas ─────────────────────────────────────
    let schema = parse_schema(&SchemaConfig {
        schema_dir: "src/schema".into(),
    }).expect("Failed to parse schemas");

    // ── Stage 2: Persistence (independent generators) ───────────────
    let seaorm = gen_seaorm(&schema.entities, &SeaOrmConfig {
        entity_output: "src/persistence/db/entities/generated".into(),
        conversion_output: "src/persistence/db/conversions/generated".into(),
    }).expect("Failed to generate SeaORM entities");

    gen_markdown_io(&schema.entities, &MarkdownIoConfig {
        output_dir: "src/persistence/fs_markdown/generated".into(),
        store_wiring: Some(StoreWiringConfig {
            subscriber_output: "src/persistence/fs_markdown/generated/subscriber.rs".into(),
        }),
    }).expect("Failed to generate markdown I/O");

    // ── Stage 3: Store (DTOs + CRUD + scaffold hooks, merges custom) ─
    let store = gen_store(
        &schema.entities,
        Some(&seaorm),
        &["src/store/custom"],
        &StoreConfig {
            output_dir: "src/store/generated".into(),
            hooks_dir: "src/store/hooks".into(),     // scaffold stubs here
            dto_output: "src/schema/dto".into(),
            event_emission: true,
            change_channels: true,
            entity_overrides: vec![],
        },
    ).expect("Failed to generate store");

    // ── Stage 4: API layer ─────────────────────────────────────────
    let api = gen_api(
        &schema.entities,
        Some(&store),
        &["src/api/v1"],                       // scan for custom modules
        &ApiConfig {
            generated_output: "src/api/v1/generated".into(),
            versions: vec!["v1".into()],
            exclude: vec![],
            state_type: "AppState".into(),
            store_type: Some("Store".into()),
        },
    ).expect("Failed to generate API");

    // ── Stage 5: Servers ────────────────────────────────────────────
    let servers = gen_servers(&api, Some(&schema.entities), &ServersConfig {
        scoping: Some(ScopingConfig {
            param_name: "project_id",
            rust_type: "uuid::Uuid",
            ts_type: "string",
            state_accessor: "store_for",
        }),
        http: Some(HttpConfig {
            output: "src/api/transport/http/generated.rs".into(),
            route_prefix: "projects/:project_id",
        }),
        ipc: Some(IpcConfig {
            output: "src/api/transport/ipc/generated.rs".into(),
        }),
        mcp: Some(McpConfig {
            output: "src/api/transport/mcp/generated.rs".into(),
        }),
        naming: NamingConfig::default()
            .plural("work_session", "sessions")
            .plural("evidence", "evidence")
            .label("work_session", "Work Session"),
    }).expect("Failed to generate servers");

    // ── Stage 6: Clients ─────────────────────────────────────────────
    gen_clients(&servers, Some(&schema.entities), &ClientsConfig {
        typescript: Some(TypeScriptConfig {
            output: "src-nuxt/app/transport/generated.ts".into(),
            bindings_path: "src-nuxt/app/types/bindings.ts".into(),
        }),
        admin_registry: Some(AdminRegistryConfig {
            output: "src-nuxt/layers/admin/generated/admin-registry.ts".into(),
        }),
    }).expect("Failed to generate clients");

    // ── Tauri build (unchanged) ────────────────────────────────────
    tauri_build::build();
}
```

---

## Adding a New Entity: Before and After

### Adding a "Label" entity

#### Before (current system): 7 manual steps

1. Create `src/schema/label.rs` (hand-written)
2. Create `src/store/label.rs` with list/get/create/update/delete (hand-written, ~150 lines of boilerplate)
3. Create `src/api/v1/label.rs` with 5 forwarding functions (hand-written, ~30 lines)
4. Add `label` to `store/mod.rs` (hand-written)
5. Add `label` to `api/v1/mod.rs` (hand-written)
6. Run build to generate DB entities, conversions, DTOs, markdown I/O, and transport handlers
7. Update entity count test assertion

#### After (unified system): 2 manual steps

1. Create `src/schema/label.rs` (hand-written — same as before)
2. Run build → **everything else is generated**:
   - DB entities, conversions, DTOs, markdown I/O (persistence layer)
   - Store CRUD + scaffold hooks + populate_relations (store layer)
   - API forwarding functions (API layer)
   - HTTP, IPC, MCP, TypeScript handlers (transport layer)

If Label needs custom logic (e.g., a validation hook), just fill in the scaffolded stub:

3. Edit `src/store/hooks/label.rs` — the no-op stubs are already there, just add your logic

---

## Standalone Usage Examples

### Example 1: Transport-only (no schema, just scan source)

Useful if you have an API-only service with hand-written handlers:

```rust
let api = gen_api(&[], None, &["src/handlers"], &ApiConfig {
    generated_output: "".into(),  // no generated CRUD
    versions: vec![],
    exclude: vec![],
    state_type: "AppState".into(),
    store_type: None,
})?;

let servers = gen_servers(&api, None, &ServersConfig {
    scoping: None,
    http: Some(HttpConfig {
        output: "src/transport/http.rs".into(),
        route_prefix: "",
    }),
    ipc: None,
    mcp: None,
    naming: NamingConfig::default(),
})?;

// Client only generates createHttpTransport() — no IPC, no switching
gen_clients(&servers, None, &ClientsConfig {
    typescript: Some(TypeScriptConfig {
        output: "frontend/api.ts".into(),
        bindings_path: "frontend/types.ts".into(),
    }),
    admin_registry: None,
})?;
```

### Example 2: Persistence + Store only (library crate, no API)

Useful for a shared data layer consumed by multiple services:

```rust
let schema = parse_schema(&SchemaConfig {
    schema_dir: "src/models".into(),
})?;

let seaorm = gen_seaorm(&schema.entities, &SeaOrmConfig { /* ... */ })?;
// No gen_markdown_io — no markdown for this project

let _store = gen_store(
    &schema.entities,
    Some(&seaorm),
    &[],                          // no custom store methods
    &StoreConfig {
        output_dir: "src/store/generated".into(),
        hooks_dir: "src/store/hooks".into(),
        dto_output: "src/dto".into(),
        event_emission: false,    // no events needed
        change_channels: false,
        entity_overrides: vec![],
    },
)?;

// No gen_api, gen_servers, or gen_clients — this is a library
```

### Example 3: DTOs only (no store, no CRUD)

Useful when you just need typed input structs for an external API client:

```rust
let schema = parse_schema(&SchemaConfig {
    schema_dir: "src/schema".into(),
})?;

// Standalone DTO generation — same output as gen_store's DTO portion,
// but without generating CRUD/hooks/channels.
gen_dtos(&schema.entities, &DtoConfig {
    output_dir: "src/dto".into(),
})?;
```

### Example 4: Adding a new server type

A future `gen_graphql` could slot in as another server, with its output feeding into `gen_clients`:

```rust
let schema = parse_schema(&schema_config)?;
let seaorm = gen_seaorm(&schema.entities, &seaorm_config)?;
let store = gen_store(&schema.entities, Some(&seaorm), &[], &store_config)?;
let api = gen_api(&schema.entities, Some(&store), &["src/api/v1"], &api_config)?;

let servers = gen_servers(&api, Some(&schema.entities), &servers_config)?;

// New server type — takes ApiOutput, produces GraphQL schema + resolvers
// Could also contribute to ServersOutput for client generation
gen_graphql(&api, Some(&schema.entities), &GraphQLConfig {
    output: "src/graphql/generated.rs".into(),
    schema_output: "schema.graphql".into(),
})?;

gen_clients(&servers, Some(&schema.entities), &clients_config)?;
```

The new generator follows the same pattern: required inputs + optional upstream enrichment.
