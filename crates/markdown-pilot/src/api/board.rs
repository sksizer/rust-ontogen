//! Boards: one per tag, holding the tasks that carry it. No entity stands
//! behind the module, so its junction ops are served as custom ops. They
//! take the store, so the scoped router serves them under the prefix.

use crate::schema::{AppError, Task};
use crate::store::Store;
use crate::store::task::TaskUpdate;

/// The tasks carrying the tag `tag_id`, which must exist.
pub async fn list_tasks(store: &Store, tag_id: &str) -> Result<Vec<Task>, AppError> {
    store.get_tag(tag_id).await?;
    let tasks = store.list_tasks(None, None).await?;
    Ok(tasks.into_iter().filter(|t| t.tags.iter().any(|tag| tag == tag_id)).collect())
}

/// Put a task on the tag's board. Putting it there twice changes nothing.
pub async fn add_task(store: &Store, tag_id: &str, task_id: &str) -> Result<(), AppError> {
    store.get_tag(tag_id).await?;
    let mut tags = store.get_task(task_id).await?.tags;
    if !tags.iter().any(|t| t == tag_id) {
        tags.push(tag_id.to_string());
    }
    store.update_task(task_id, TaskUpdate { tags: Some(tags), ..TaskUpdate::default() }).await.map(drop)
}

/// Take a task off the tag's board.
pub async fn remove_task(store: &Store, tag_id: &str, task_id: &str) -> Result<(), AppError> {
    let mut tags = store.get_task(task_id).await?.tags;
    tags.retain(|t| t != tag_id);
    store.update_task(task_id, TaskUpdate { tags: Some(tags), ..TaskUpdate::default() }).await.map(drop)
}
