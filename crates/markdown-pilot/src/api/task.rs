//! Hand-written task ops beside the generated CRUD, which this module
//! re-exports so the servers stage reaches both through one module path.

use tokio::sync::broadcast;

pub use super::generated::task::*;
use crate::AppState;
use crate::schema::{AppError, CreateTaskInput, Tag, Task, TaskSummary};
use crate::store::Store;
use crate::store::task::TaskUpdate;

/// The tasks in `status`, counted, with their titles when `verbose`. `limit`
/// caps the titles.
pub async fn get_summary(
    store: &Store,
    status: &str,
    verbose: Option<bool>,
    limit: Option<u32>,
) -> Result<TaskSummary, AppError> {
    let matching: Vec<Task> = store.list_tasks(None, None).await?.into_iter().filter(|t| t.status == status).collect();
    let titles = verbose.unwrap_or(false).then(|| {
        let cap = limit.map_or(usize::MAX, |l| l as usize);
        matching.iter().take(cap).map(|t| t.title.clone()).collect()
    });
    Ok(TaskSummary { status: status.to_string(), count: matching.len() as u64, titles })
}

/// Create a task, filed under `status` when given instead of the input's own.
pub async fn capture(store: &Store, input: CreateTaskInput, status: Option<String>) -> Result<Task, AppError> {
    let mut task: Task = input.into();
    if let Some(status) = status {
        task.status = status;
    }
    store.create_task(task).await
}

/// Mark a task done.
pub async fn complete(store: &Store, id: &str) -> Result<(), AppError> {
    let updates = TaskUpdate { status: Some("done".into()), ..TaskUpdate::default() };
    store.update_task(id, updates).await.map(drop)
}

/// File a task under `state`, retitled `store` when given. The arguments
/// are named like the bindings of a generated handler, which must still
/// tell them apart.
pub async fn set_state(ctx: &Store, id: &str, state: String, store: Option<String>) -> Result<Task, AppError> {
    let updates = TaskUpdate { status: Some(state), title: store, ..TaskUpdate::default() };
    ctx.update_task(id, updates).await
}

/// Delete every done task, answering how many went.
pub async fn purge_done(store: &Store) -> Result<u64, AppError> {
    let mut purged = 0;
    for task in store.list_tasks(None, None).await? {
        if task.status == "done" {
            store.delete_task(&task.id).await?;
            purged += 1;
        }
    }
    Ok(purged)
}

/// A task's tags, in the order the task lists them.
pub async fn list_tags(store: &Store, id: &str) -> Result<Vec<Tag>, AppError> {
    let task = store.get_task(id).await?;
    let mut tags = Vec::with_capacity(task.tags.len());
    for tag_id in &task.tags {
        tags.push(store.get_tag(tag_id).await?);
    }
    Ok(tags)
}

/// Tag a task. Tagging it twice changes nothing.
pub async fn add_tag(store: &Store, id: &str, tag_id: &str) -> Result<(), AppError> {
    store.get_tag(tag_id).await?;
    let mut tags = store.get_task(id).await?.tags;
    if !tags.iter().any(|t| t == tag_id) {
        tags.push(tag_id.to_string());
    }
    store.update_task(id, TaskUpdate { tags: Some(tags), ..TaskUpdate::default() }).await.map(drop)
}

/// Untag a task.
pub async fn remove_tag(store: &Store, id: &str, tag_id: &str) -> Result<(), AppError> {
    let mut tags = store.get_task(id).await?.tags;
    tags.retain(|t| t != tag_id);
    store.update_task(id, TaskUpdate { tags: Some(tags), ..TaskUpdate::default() }).await.map(drop)
}

/// Every task published to the task feed.
pub fn task_feed(state: &AppState) -> broadcast::Receiver<Task> {
    state.task_feed.subscribe()
}

/// The task feed, for subscribers of one task that must exist.
pub async fn watch_task(state: &AppState, id: String) -> Result<broadcast::Receiver<Task>, AppError> {
    state.store.get_task(&id).await?;
    Ok(state.task_feed.subscribe())
}
