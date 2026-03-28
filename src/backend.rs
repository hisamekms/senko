use std::collections::HashMap;

use anyhow::Result;

use crate::models::{
    CreateTaskParams, ListTasksFilter, Task, UpdateTaskArrayParams, UpdateTaskParams,
};

pub trait TaskBackend: Send + Sync {
    fn create_task(&self, params: &CreateTaskParams) -> Result<Task>;
    fn get_task(&self, id: i64) -> Result<Task>;
    fn ready_task(&self, id: i64) -> Result<Task>;
    fn start_task(&self, id: i64, assignee_session_id: Option<String>, started_at: &str) -> Result<Task>;
    fn complete_task(&self, id: i64, completed_at: &str) -> Result<Task>;
    fn cancel_task(&self, id: i64, canceled_at: &str, reason: Option<String>) -> Result<Task>;
    fn update_task(&self, id: i64, params: &UpdateTaskParams) -> Result<Task>;
    fn update_task_arrays(&self, id: i64, params: &UpdateTaskArrayParams) -> Result<()>;
    fn delete_task(&self, id: i64) -> Result<()>;
    fn list_tasks(&self, filter: &ListTasksFilter) -> Result<Vec<Task>>;
    fn next_task(&self) -> Result<Option<Task>>;
    fn task_stats(&self) -> Result<HashMap<String, i64>>;
    fn ready_count(&self) -> Result<i64>;
    fn list_ready_tasks(&self) -> Result<Vec<Task>>;
    fn add_dependency(&self, task_id: i64, dep_id: i64) -> Result<Task>;
    fn remove_dependency(&self, task_id: i64, dep_id: i64) -> Result<Task>;
    fn set_dependencies(&self, task_id: i64, dep_ids: &[i64]) -> Result<Task>;
    fn list_dependencies(&self, task_id: i64) -> Result<Vec<Task>>;
    fn check_dod(&self, task_id: i64, index: usize) -> Result<Task>;
    fn uncheck_dod(&self, task_id: i64, index: usize) -> Result<Task>;
}
