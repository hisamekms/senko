use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

use crate::domain::task::Task;
use crate::domain::TaskRepository;

/// Port for task state transitions.
///
/// Local backends (Sqlite, Postgres, DynamoDB) use the default implementation
/// via `impl_task_transition_default!`, which performs get → domain transition → save.
/// HttpBackend overrides to call the server's dedicated POST endpoints.
#[async_trait]
pub trait TaskTransitionPort: Send + Sync {
    async fn ready_task(&self, project_id: i64, id: i64) -> Result<Task>;
    async fn start_task(
        &self,
        project_id: i64,
        id: i64,
        session_id: Option<String>,
        user_id: Option<i64>,
        metadata: Option<serde_json::Value>,
    ) -> Result<Task>;
    async fn complete_task(
        &self,
        project_id: i64,
        id: i64,
        skip_pr_check: bool,
    ) -> Result<Task>;
    async fn cancel_task(
        &self,
        project_id: i64,
        id: i64,
        reason: Option<String>,
    ) -> Result<Task>;
}

fn now_rfc3339() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub async fn default_ready_task(
    repo: &(dyn TaskRepository + Sync),
    project_id: i64,
    id: i64,
) -> Result<Task> {
    let task = repo.get_task(project_id, id).await?;
    let (task, _events) = task.ready(now_rfc3339())?;
    repo.save(&task).await?;
    Ok(task)
}

pub async fn default_start_task(
    repo: &(dyn TaskRepository + Sync),
    project_id: i64,
    id: i64,
    session_id: Option<String>,
    user_id: Option<i64>,
    metadata: Option<serde_json::Value>,
) -> Result<Task> {
    let task = repo.get_task(project_id, id).await?;
    let (task, _events) = task.start(session_id, user_id, now_rfc3339(), metadata)?;
    repo.save(&task).await?;
    Ok(task)
}

pub async fn default_complete_task(
    repo: &(dyn TaskRepository + Sync),
    project_id: i64,
    id: i64,
) -> Result<Task> {
    let task = repo.get_task(project_id, id).await?;
    let (task, _events) = task.complete(now_rfc3339())?;
    repo.save(&task).await?;
    Ok(task)
}

pub async fn default_cancel_task(
    repo: &(dyn TaskRepository + Sync),
    project_id: i64,
    id: i64,
    reason: Option<String>,
) -> Result<Task> {
    let task = repo.get_task(project_id, id).await?;
    let (task, _events) = task.cancel(now_rfc3339(), reason)?;
    repo.save(&task).await?;
    Ok(task)
}

#[macro_export]
macro_rules! impl_task_transition_default {
    ($ty:ty) => {
        #[async_trait::async_trait]
        impl $crate::application::port::task_transition::TaskTransitionPort for $ty {
            async fn ready_task(&self, project_id: i64, id: i64) -> anyhow::Result<$crate::domain::task::Task> {
                $crate::application::port::task_transition::default_ready_task(self, project_id, id).await
            }
            async fn start_task(
                &self,
                project_id: i64,
                id: i64,
                session_id: Option<String>,
                user_id: Option<i64>,
                metadata: Option<serde_json::Value>,
            ) -> anyhow::Result<$crate::domain::task::Task> {
                $crate::application::port::task_transition::default_start_task(self, project_id, id, session_id, user_id, metadata).await
            }
            async fn complete_task(
                &self,
                project_id: i64,
                id: i64,
                _skip_pr_check: bool,
            ) -> anyhow::Result<$crate::domain::task::Task> {
                $crate::application::port::task_transition::default_complete_task(self, project_id, id).await
            }
            async fn cancel_task(
                &self,
                project_id: i64,
                id: i64,
                reason: Option<String>,
            ) -> anyhow::Result<$crate::domain::task::Task> {
                $crate::application::port::task_transition::default_cancel_task(self, project_id, id, reason).await
            }
        }
    };
}
