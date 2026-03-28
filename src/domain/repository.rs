use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use super::project::{CreateProjectParams, Project};
use super::task::{
    CreateTaskParams, ListTasksFilter, Task, UpdateTaskArrayParams, UpdateTaskParams,
};
use super::user::{
    AddProjectMemberParams, ApiKey, ApiKeyWithSecret, CreateUserParams, ProjectMember, Role, User,
};

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task>;
    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task>;
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
    async fn save(&self, task: &Task) -> Result<()>;
}

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project>;
    async fn get_project(&self, id: i64) -> Result<Project>;
    async fn get_project_by_name(&self, name: &str) -> Result<Project>;
    async fn list_projects(&self) -> Result<Vec<Project>>;
    async fn delete_project(&self, id: i64) -> Result<()>;

    // User management
    async fn create_user(&self, params: &CreateUserParams) -> Result<User>;
    async fn get_user(&self, id: i64) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<User>;
    async fn list_users(&self) -> Result<Vec<User>>;
    async fn delete_user(&self, id: i64) -> Result<()>;

    // Project membership
    async fn add_project_member(&self, project_id: i64, params: &AddProjectMemberParams) -> Result<ProjectMember>;
    async fn remove_project_member(&self, project_id: i64, user_id: i64) -> Result<()>;
    async fn list_project_members(&self, project_id: i64) -> Result<Vec<ProjectMember>>;
    async fn get_project_member(&self, project_id: i64, user_id: i64) -> Result<ProjectMember>;
    async fn update_member_role(&self, project_id: i64, user_id: i64, role: Role) -> Result<ProjectMember>;

    // API key management
    async fn create_api_key(&self, user_id: i64, name: &str) -> Result<ApiKeyWithSecret>;
    async fn get_user_by_api_key(&self, key: &str) -> Result<User>;
    async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>>;
    async fn delete_api_key(&self, key_id: i64) -> Result<()>;
}

/// Combined trait for backends that implement both TaskRepository and ProjectRepository.
/// Backends automatically implement TaskBackend via the blanket impl.
pub trait TaskBackend: TaskRepository + ProjectRepository {}

impl<T: TaskRepository + ProjectRepository> TaskBackend for T {}
