use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::pagination::ListPage;
use crate::domain::project::ProjectId;
use crate::domain::task::{
    CreateTaskParams, ListTaskDepsFilter, ListTasksFilter, ListTasksPage, MetadataUpdate, Task,
    TaskId, TaskStatus, UnblockedTask, UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::UserId;

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
/// Both local (`LocalTaskOperations`) and remote (`RemoteTaskOperations`) implementations
/// can satisfy this trait, allowing the presentation layer to depend only on the
/// abstraction rather than a concrete service type.
#[async_trait]
pub trait TaskOperations: Send + Sync {
    // --- State transitions ---

    async fn create_task(&self, project_id: ProjectId, params: &CreateTaskParams) -> Result<Task>;
    async fn publish_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task>;
    async fn start_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        session_id: Option<String>,
        user_id: Option<UserId>,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task>;
    async fn resume_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        session_id: Option<String>,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task>;
    async fn next_task(
        &self,
        project_id: ProjectId,
        session_id: Option<String>,
        user_id: Option<UserId>,
        include_unassigned: bool,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task>;
    async fn complete_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        skip_pr_check: bool,
    ) -> Result<CompleteResult>;
    async fn cancel_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        reason: Option<String>,
    ) -> Result<Task>;

    // --- Preview ---

    async fn preview_transition(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        target: TaskStatus,
    ) -> Result<PreviewResult>;
    async fn preview_next(&self, project_id: ProjectId) -> Result<PreviewResult>;

    // --- Queries ---

    async fn get_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task>;
    async fn list_tasks(
        &self,
        project_id: ProjectId,
        filter: &ListTasksFilter,
    ) -> Result<ListTasksPage>;
    async fn list_all_tags(&self, project_id: ProjectId) -> Result<Vec<String>>;
    async fn task_stats(&self, project_id: ProjectId) -> Result<HashMap<String, i64>>;

    // --- Edit ---

    async fn edit_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskParams,
    ) -> Result<Task>;
    async fn edit_task_arrays(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskArrayParams,
    ) -> Result<()>;
    async fn delete_task(&self, project_id: ProjectId, id: TaskId) -> Result<()>;
    async fn save_task(&self, project_id: ProjectId, id: TaskId, task: &Task) -> Result<()>;

    // --- Definition of Done ---

    async fn check_dod(&self, project_id: ProjectId, task_id: TaskId, index: usize)
    -> Result<Task>;
    async fn uncheck_dod(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        index: usize,
    ) -> Result<Task>;

    // --- Dependencies ---

    async fn add_dependency(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_id: TaskId,
    ) -> Result<Task>;
    async fn remove_dependency(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_id: TaskId,
    ) -> Result<Task>;
    async fn set_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_ids: &[TaskId],
    ) -> Result<Task>;
    async fn list_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        filter: &ListTaskDepsFilter,
    ) -> Result<ListPage<Task>>;
    async fn list_ready_tasks(&self, project_id: ProjectId) -> Result<Vec<Task>>;
    async fn ready_count(&self, project_id: ProjectId) -> Result<i64>;
}
