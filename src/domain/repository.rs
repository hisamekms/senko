use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

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

    /// Transition a task to "ready" (todo) status. Default impl does local transition + save.
    /// HTTP backend overrides to call the server's dedicated endpoint.
    async fn ready_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let task = self.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.ready(now)?;
        self.save(&task).await?;
        Ok(task)
    }

    /// Transition a task to "in_progress" status.
    async fn start_task(
        &self,
        project_id: i64,
        id: i64,
        session_id: Option<String>,
        user_id: Option<i64>,
    ) -> Result<Task> {
        let task = self.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.start(session_id, user_id, now)?;
        self.save(&task).await?;
        Ok(task)
    }

    /// Transition a task to "completed" status.
    async fn complete_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let task = self.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.complete(now)?;
        self.save(&task).await?;
        Ok(task)
    }

    /// Transition a task to "canceled" status.
    async fn cancel_task(&self, project_id: i64, id: i64, reason: Option<String>) -> Result<Task> {
        let task = self.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.cancel(now, reason)?;
        self.save(&task).await?;
        Ok(task)
    }

    /// Check a DoD item.
    async fn check_dod(&self, project_id: i64, id: i64, index: usize) -> Result<Task> {
        let task = self.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.check_dod(index, now)?;
        self.save(&task).await?;
        Ok(task)
    }

    /// Uncheck a DoD item.
    async fn uncheck_dod(&self, project_id: i64, id: i64, index: usize) -> Result<Task> {
        let task = self.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.uncheck_dod(index, now)?;
        self.save(&task).await?;
        Ok(task)
    }
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
    async fn create_api_key(&self, user_id: i64, name: &str, new_key: &NewApiKey) -> Result<ApiKeyWithSecret>;
    async fn get_user_by_api_key(&self, key_hash: &str) -> Result<User>;
    async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>>;
    async fn delete_api_key(&self, key_id: i64) -> Result<()>;
}

