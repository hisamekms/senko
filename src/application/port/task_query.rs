use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::task::{ListTasksFilter, Task};

/// Port trait for querying tasks.
/// Separated from TaskRepository to keep the repository focused on
/// command operations (get/save/delete).
#[async_trait]
pub trait TaskQueryPort: Send + Sync {
    async fn list_tasks(&self, project_id: i64, filter: &ListTasksFilter) -> Result<Vec<Task>>;
    async fn next_task(
        &self,
        project_id: i64,
        user_id: Option<i64>,
        include_unassigned: bool,
    ) -> Result<Option<Task>>;
    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>>;
    async fn ready_count(&self, project_id: i64) -> Result<i64>;
    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>>;
    /// Returns whether a single task is currently "ready to be worked on"
    /// (status == todo AND every dependency is completed).
    ///
    /// Must mirror the canonical definition in `crate::domain::task::Task::is_ready`.
    /// Returns `Ok(false)` if the task does not exist in the given project.
    async fn is_task_ready(&self, project_id: i64, task_id: i64) -> Result<bool>;
}
