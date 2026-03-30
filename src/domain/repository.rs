use anyhow::Result;
use async_trait::async_trait;

use super::project::{CreateProjectParams, Project};
use super::task::{CreateTaskParams, Task, UpdateTaskArrayParams, UpdateTaskParams};
use super::user::{
    AddProjectMemberParams, ApiKey, ApiKeyWithSecret, CreateUserParams, NewApiKey, ProjectMember,
    Role, User,
};

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task>;
    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task>;
    async fn update_task(&self, project_id: i64, id: i64, params: &UpdateTaskParams) -> Result<Task>;
    async fn update_task_arrays(&self, project_id: i64, id: i64, params: &UpdateTaskArrayParams) -> Result<()>;
    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()>;
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

    // Project membership
    async fn add_project_member(&self, project_id: i64, params: &AddProjectMemberParams) -> Result<ProjectMember>;
    async fn remove_project_member(&self, project_id: i64, user_id: i64) -> Result<()>;
    async fn list_project_members(&self, project_id: i64) -> Result<Vec<ProjectMember>>;
    async fn get_project_member(&self, project_id: i64, user_id: i64) -> Result<ProjectMember>;
    async fn update_member_role(&self, project_id: i64, user_id: i64, role: Role) -> Result<ProjectMember>;
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create_user(&self, params: &CreateUserParams) -> Result<User>;
    async fn get_user(&self, id: i64) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<User>;
    async fn list_users(&self) -> Result<Vec<User>>;
    async fn delete_user(&self, id: i64) -> Result<()>;
}

#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    /// Whether this backend supports API key CRUD (create, list, delete).
    fn supports_api_key_management(&self) -> bool {
        true
    }

    /// Whether this backend supports API key authentication (lookup by hash).
    fn supports_api_key_auth(&self) -> bool {
        true
    }

    async fn create_api_key(&self, user_id: i64, name: &str, new_key: &NewApiKey) -> Result<ApiKeyWithSecret>;
    async fn get_user_by_api_key(&self, key_hash: &str) -> Result<User>;
    async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>>;
    async fn delete_api_key(&self, key_id: i64) -> Result<()>;
}

