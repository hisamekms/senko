use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::project::ProjectId;
use crate::domain::task::{ListTasksFilter, Task, TaskId};

/// Port trait for querying tasks.
/// Separated from TaskRepository to keep the repository focused on
/// command operations (get/save/delete).
#[async_trait]
pub trait TaskQueryPort: Send + Sync {
    async fn list_tasks(
        &self,
        project_id: ProjectId,
        filter: &ListTasksFilter,
    ) -> Result<Vec<Task>>;
    async fn next_task(
        &self,
        project_id: ProjectId,
        user_id: Option<i64>,
        include_unassigned: bool,
    ) -> Result<Option<Task>>;
    async fn task_stats(&self, project_id: ProjectId) -> Result<HashMap<String, i64>>;
    async fn ready_count(&self, project_id: ProjectId) -> Result<i64>;
    async fn list_ready_tasks(&self, project_id: ProjectId) -> Result<Vec<Task>>;
    /// Returns whether a single task is currently "ready to be worked on"
    /// (status == todo AND every dependency is completed).
    ///
    /// The second argument is the task's public `id` (the per-project sequential
    /// identifier exposed by HTTP APIs / CLI output / hook payloads). The infra
    /// layer resolves it to an internal DB primary key on its own.
    ///
    /// Must mirror the canonical definition in `crate::domain::task::Task::is_ready`.
    /// Returns `Ok(false)` if the task does not exist in the given project.
    async fn is_task_ready(&self, project_id: ProjectId, id: TaskId) -> Result<bool>;
}
