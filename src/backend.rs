use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::models::{
    CreateTaskParams, ListTasksFilter, Task, UpdateTaskArrayParams, UpdateTaskParams,
};

#[async_trait]
pub trait TaskBackend: Send + Sync {
    async fn create_task(&self, params: &CreateTaskParams) -> Result<Task>;
    async fn get_task(&self, id: i64) -> Result<Task>;
    async fn ready_task(&self, id: i64) -> Result<Task>;
    async fn start_task(&self, id: i64, assignee_session_id: Option<String>, started_at: &str) -> Result<Task>;
    async fn complete_task(&self, id: i64, completed_at: &str) -> Result<Task>;
    async fn cancel_task(&self, id: i64, canceled_at: &str, reason: Option<String>) -> Result<Task>;
    async fn update_task(&self, id: i64, params: &UpdateTaskParams) -> Result<Task>;
    async fn update_task_arrays(&self, id: i64, params: &UpdateTaskArrayParams) -> Result<()>;
    async fn delete_task(&self, id: i64) -> Result<()>;
    async fn list_tasks(&self, filter: &ListTasksFilter) -> Result<Vec<Task>>;
    async fn next_task(&self) -> Result<Option<Task>>;
    async fn task_stats(&self) -> Result<HashMap<String, i64>>;
    async fn ready_count(&self) -> Result<i64>;
    async fn list_ready_tasks(&self) -> Result<Vec<Task>>;
    async fn add_dependency(&self, task_id: i64, dep_id: i64) -> Result<Task>;
    async fn remove_dependency(&self, task_id: i64, dep_id: i64) -> Result<Task>;
    async fn set_dependencies(&self, task_id: i64, dep_ids: &[i64]) -> Result<Task>;
    async fn list_dependencies(&self, task_id: i64) -> Result<Vec<Task>>;
    async fn check_dod(&self, task_id: i64, index: usize) -> Result<Task>;
    async fn uncheck_dod(&self, task_id: i64, index: usize) -> Result<Task>;
}
