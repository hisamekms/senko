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
    async fn next_task(&self, project_id: i64) -> Result<Option<Task>>;
    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>>;
    async fn ready_count(&self, project_id: i64) -> Result<i64>;
    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>>;
}
