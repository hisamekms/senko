use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::models::{
    CreateProjectParams, CreateTaskParams, ListTasksFilter, Project, Task, UpdateTaskArrayParams,
    UpdateTaskParams,
};

#[async_trait]
pub trait TaskBackend: Send + Sync {
    // Project management
    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project>;
    async fn get_project(&self, id: i64) -> Result<Project>;
    async fn get_project_by_name(&self, name: &str) -> Result<Project>;
    async fn list_projects(&self) -> Result<Vec<Project>>;
    async fn delete_project(&self, id: i64) -> Result<()>;

    // Task CRUD
    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task>;
    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task>;
    async fn ready_task(&self, project_id: i64, id: i64) -> Result<Task>;
    async fn start_task(&self, project_id: i64, id: i64, assignee_session_id: Option<String>, started_at: &str) -> Result<Task>;
    async fn complete_task(&self, project_id: i64, id: i64, completed_at: &str) -> Result<Task>;
    async fn cancel_task(&self, project_id: i64, id: i64, canceled_at: &str, reason: Option<String>) -> Result<Task>;
    async fn update_task(&self, project_id: i64, id: i64, params: &UpdateTaskParams) -> Result<Task>;
    async fn update_task_arrays(&self, project_id: i64, id: i64, params: &UpdateTaskArrayParams) -> Result<()>;
    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()>;
    async fn list_tasks(&self, project_id: i64, filter: &ListTasksFilter) -> Result<Vec<Task>>;
    async fn next_task(&self, project_id: i64) -> Result<Option<Task>>;
    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>>;
    async fn ready_count(&self, project_id: i64) -> Result<i64>;
    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>>;
    async fn add_dependency(&self, project_id: i64, task_id: i64, dep_id: i64) -> Result<Task>;
    async fn remove_dependency(&self, project_id: i64, task_id: i64, dep_id: i64) -> Result<Task>;
    async fn set_dependencies(&self, project_id: i64, task_id: i64, dep_ids: &[i64]) -> Result<Task>;
    async fn list_dependencies(&self, project_id: i64, task_id: i64) -> Result<Vec<Task>>;
    async fn check_dod(&self, project_id: i64, task_id: i64, index: usize) -> Result<Task>;
    async fn uncheck_dod(&self, project_id: i64, task_id: i64, index: usize) -> Result<Task>;
}
