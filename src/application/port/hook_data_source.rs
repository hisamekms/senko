use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::project::Project;
use crate::domain::task::Task;
use crate::domain::user::User;

/// Narrow trait covering only the data that the hook system needs.
///
/// Concrete `TaskBackend` types (e.g. `SqliteBackend`) satisfy this via the
/// blanket impl. For `Arc<dyn TaskBackend>` (erased), use `BackendHookData`.
/// Remote mode uses `RemoteHookDataSource` (HTTP-based).
#[async_trait]
pub trait HookDataSource: Send + Sync {
    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>>;
    async fn ready_count(&self, project_id: i64) -> Result<i64>;
    async fn get_project(&self, id: i64) -> Result<Project>;
    async fn get_project_by_name(&self, name: &str) -> Result<Project>;
    async fn get_user(&self, id: i64) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<User>;
    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task>;
    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>>;
    async fn is_task_ready(&self, project_id: i64, task_id: i64) -> Result<bool>;
}

/// Blanket impl: any concrete type that implements `TaskBackend` automatically
/// implements `HookDataSource` by delegating to the relevant repository/port methods.
#[async_trait]
impl<T: super::TaskBackend + ?Sized> HookDataSource for T {
    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>> {
        super::TaskQueryPort::task_stats(self, project_id).await
    }
    async fn ready_count(&self, project_id: i64) -> Result<i64> {
        super::TaskQueryPort::ready_count(self, project_id).await
    }
    async fn get_project(&self, id: i64) -> Result<Project> {
        crate::domain::ProjectRepository::get_project(self, id).await
    }
    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        crate::domain::ProjectRepository::get_project_by_name(self, name).await
    }
    async fn get_user(&self, id: i64) -> Result<User> {
        crate::domain::UserRepository::get_user(self, id).await
    }
    async fn get_user_by_username(&self, username: &str) -> Result<User> {
        crate::domain::UserRepository::get_user_by_username(self, username).await
    }
    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        crate::domain::TaskRepository::get_task(self, project_id, id).await
    }
    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        super::TaskQueryPort::list_ready_tasks(self, project_id).await
    }
    async fn is_task_ready(&self, project_id: i64, task_id: i64) -> Result<bool> {
        super::TaskQueryPort::is_task_ready(self, project_id, task_id).await
    }
}

/// Adapter that wraps `Arc<dyn TaskBackend>` to provide `HookDataSource`.
///
/// Needed because Rust cannot upcast `Arc<dyn TaskBackend>` to
/// `Arc<dyn HookDataSource>` even with the blanket impl above.
pub struct BackendHookData(pub Arc<dyn super::TaskBackend>);

#[async_trait]
impl HookDataSource for BackendHookData {
    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>> {
        super::TaskQueryPort::task_stats(&*self.0, project_id).await
    }
    async fn ready_count(&self, project_id: i64) -> Result<i64> {
        super::TaskQueryPort::ready_count(&*self.0, project_id).await
    }
    async fn get_project(&self, id: i64) -> Result<Project> {
        crate::domain::ProjectRepository::get_project(&*self.0, id).await
    }
    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        crate::domain::ProjectRepository::get_project_by_name(&*self.0, name).await
    }
    async fn get_user(&self, id: i64) -> Result<User> {
        crate::domain::UserRepository::get_user(&*self.0, id).await
    }
    async fn get_user_by_username(&self, username: &str) -> Result<User> {
        crate::domain::UserRepository::get_user_by_username(&*self.0, username).await
    }
    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        crate::domain::TaskRepository::get_task(&*self.0, project_id, id).await
    }
    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        super::TaskQueryPort::list_ready_tasks(&*self.0, project_id).await
    }
    async fn is_task_ready(&self, project_id: i64, task_id: i64) -> Result<bool> {
        super::TaskQueryPort::is_task_ready(&*self.0, project_id, task_id).await
    }
}
