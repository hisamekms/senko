use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

use crate::domain::TaskRepository;
use crate::domain::task::{MetadataUpdate, Task, TaskId};

/// Port for task state transitions.
///
/// Local backends (Sqlite, Postgres) use the default implementation
/// via `impl_task_transition_default!`, which performs get → domain transition → save.
/// Remote mode uses `RemoteTaskOperations` which calls the server's POST endpoints directly.
#[async_trait]
pub trait TaskTransitionPort: Send + Sync {
    async fn publish_task(&self, project_id: i64, id: TaskId) -> Result<Task>;
    async fn start_task(
        &self,
        project_id: i64,
        id: TaskId,
        session_id: Option<String>,
        user_id: Option<i64>,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task>;
    async fn complete_task(&self, project_id: i64, id: TaskId, skip_pr_check: bool)
    -> Result<Task>;
    async fn cancel_task(
        &self,
        project_id: i64,
        id: TaskId,
        reason: Option<String>,
    ) -> Result<Task>;
}

fn now_rfc3339() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub async fn default_publish_task(
    repo: &(dyn TaskRepository + Sync),
    project_id: i64,
    id: TaskId,
) -> Result<Task> {
    let task = repo.get_task(project_id, id).await?;
    let (task, _events) = task.publish(now_rfc3339())?;
    repo.save(&task).await?;
    Ok(task)
}

pub async fn default_start_task(
    repo: &(dyn TaskRepository + Sync),
    project_id: i64,
    id: TaskId,
    session_id: Option<String>,
    user_id: Option<i64>,
    metadata: Option<MetadataUpdate>,
) -> Result<Task> {
    let task = repo.get_task(project_id, id).await?;
    let (task, _events) = task.start(session_id, user_id, now_rfc3339(), metadata)?;
    repo.save(&task).await?;
    Ok(task)
}

pub async fn default_complete_task(
    repo: &(dyn TaskRepository + Sync),
    project_id: i64,
    id: TaskId,
) -> Result<Task> {
    let task = repo.get_task(project_id, id).await?;
    let (task, _events) = task.complete(now_rfc3339())?;
    repo.save(&task).await?;
    Ok(task)
}

pub async fn default_cancel_task(
    repo: &(dyn TaskRepository + Sync),
    project_id: i64,
    id: TaskId,
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
            async fn publish_task(
                &self,
                project_id: i64,
                id: $crate::domain::task::TaskId,
            ) -> anyhow::Result<$crate::domain::task::Task> {
                $crate::application::port::task_transition::default_publish_task(
                    self, project_id, id,
                )
                .await
            }
            async fn start_task(
                &self,
                project_id: i64,
                id: $crate::domain::task::TaskId,
                session_id: Option<String>,
                user_id: Option<i64>,
                metadata: Option<$crate::domain::task::MetadataUpdate>,
            ) -> anyhow::Result<$crate::domain::task::Task> {
                $crate::application::port::task_transition::default_start_task(
                    self, project_id, id, session_id, user_id, metadata,
                )
                .await
            }
            async fn complete_task(
                &self,
                project_id: i64,
                id: $crate::domain::task::TaskId,
                _skip_pr_check: bool,
            ) -> anyhow::Result<$crate::domain::task::Task> {
                $crate::application::port::task_transition::default_complete_task(
                    self, project_id, id,
                )
                .await
            }
            async fn cancel_task(
                &self,
                project_id: i64,
                id: $crate::domain::task::TaskId,
                reason: Option<String>,
            ) -> anyhow::Result<$crate::domain::task::Task> {
                $crate::application::port::task_transition::default_cancel_task(
                    self, project_id, id, reason,
                )
                .await
            }
        }
    };
}
