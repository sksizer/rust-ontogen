//! The task list filtered by status and epic, and sorted. It replaces the
//! generated `list` and `count`; the rest of the task module is the
//! generated one.

pub use super::generated::task::*;

use ontogen_core::order::OrderBy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{AppError, Task};
use crate::store::Store;
use crate::store::task::TaskSortField;

/// Which tasks `list` and `count` select; an absent field matches every task.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct ListTasksQuery {
    /// Exact status, e.g. `closed/done`
    pub status: Option<String>,
    /// Id of the epic the task belongs to
    pub epic_id: Option<String>,
}

fn matches(task: &Task, query: &ListTasksQuery) -> bool {
    query.status.as_deref().is_none_or(|s| task.status == s)
        && query.epic_id.as_deref().is_none_or(|e| task.epic_id.as_deref() == Some(e))
}

/// List tasks, optionally filtered by status and epic, and sorted
pub async fn list(
    store: &Store,
    query: ListTasksQuery,
    order: &[OrderBy<TaskSortField>],
    limit: Option<u64>,
    offset: Option<u64>,
) -> Result<Vec<Task>, AppError> {
    // The store has no filter, so the page is cut after filtering. Filtering
    // keeps the store's order (the sort keys, then id), so pages are cut
    // from one stable order.
    let tasks = store.list_tasks(order, None, None).await?;
    Ok(tasks
        .into_iter()
        .filter(|t| matches(t, &query))
        .skip(offset.unwrap_or(0) as usize)
        .take(limit.map_or(usize::MAX, |l| l as usize))
        .collect())
}

/// Count the tasks the same filter selects
pub async fn count(store: &Store, query: ListTasksQuery) -> Result<u64, AppError> {
    let tasks = store.list_tasks(&[], None, None).await?;
    Ok(tasks.iter().filter(|t| matches(t, &query)).count() as u64)
}
