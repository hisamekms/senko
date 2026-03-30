use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::task::{
    CreateTaskParams, ListTasksFilter, Task, TaskStatus, UnblockedTask, UpdateTaskArrayParams,
    UpdateTaskParams,
};

use super::TaskBackend;

/// Result of completing a task, including newly unblocked tasks.
#[derive(Debug, Clone)]
pub struct CompleteResult {
    pub task: Task,
    pub unblocked: Vec<UnblockedTask>,
}

/// Result of previewing a status transition without executing it.
#[derive(Debug, Clone)]
pub struct PreviewResult {
    pub allowed: bool,
    pub reason: Option<String>,
    pub task: Task,
    pub target_status: TaskStatus,
    pub operations: Vec<String>,
    pub unblocked_tasks: Vec<Task>,
}

/// Application-level port that exposes all task operations.
///
/// Both local (`TaskService`) and remote (`HttpBackend`-backed) implementations
/// can satisfy this trait, allowing the presentation layer to depend only on the
/// abstraction rather than a concrete service type.
#[async_trait]
pub trait TaskOperations: Send + Sync {
    // --- State transitions ---

    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task>;
    async fn ready_task(&self, project_id: i64, id: i64) -> Result<Task>;
    async fn start_task(
        &self,
        project_id: i64,
        id: i64,
        session_id: Option<String>,
        user_id: Option<i64>,
    ) -> Result<Task>;
    async fn next_task(
        &self,
        project_id: i64,
        session_id: Option<String>,
        user_id: Option<i64>,
    ) -> Result<Task>;
    async fn complete_task(
        &self,
        project_id: i64,
        id: i64,
        skip_pr_check: bool,
    ) -> Result<CompleteResult>;
    async fn cancel_task(
        &self,
        project_id: i64,
        id: i64,
        reason: Option<String>,
    ) -> Result<Task>;

    // --- Preview ---

    async fn preview_transition(
        &self,
        project_id: i64,
        task_id: i64,
        target: TaskStatus,
    ) -> Result<PreviewResult>;
    async fn preview_next(&self, project_id: i64) -> Result<PreviewResult>;

    // --- Queries ---

    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task>;
    async fn list_tasks(
        &self,
        project_id: i64,
        filter: &ListTasksFilter,
    ) -> Result<Vec<Task>>;
    async fn list_all_tags(&self, project_id: i64) -> Result<Vec<String>>;
    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>>;

    // --- Edit ---

    async fn edit_task(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskParams,
    ) -> Result<Task>;
    async fn edit_task_arrays(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskArrayParams,
    ) -> Result<()>;
    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()>;

    // --- Definition of Done ---

    async fn check_dod(
        &self,
        project_id: i64,
        task_id: i64,
        index: usize,
    ) -> Result<Task>;
    async fn uncheck_dod(
        &self,
        project_id: i64,
        task_id: i64,
        index: usize,
    ) -> Result<Task>;

    // --- Dependencies ---

    async fn add_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task>;
    async fn remove_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task>;
    async fn set_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
        dep_ids: &[i64],
    ) -> Result<Task>;
    async fn list_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
    ) -> Result<Vec<Task>>;
    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>>;
    async fn ready_count(&self, project_id: i64) -> Result<i64>;

    // --- Accessor ---

    fn backend(&self) -> &dyn TaskBackend;
}
