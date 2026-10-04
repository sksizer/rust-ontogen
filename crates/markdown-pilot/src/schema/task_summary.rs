use serde::Serialize;

/// How many tasks share a status, and optionally which.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TaskSummary {
    pub status: String,
    pub count: u64,
    /// Present only when the caller asked for detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub titles: Option<Vec<String>>,
}
