use async_trait::async_trait;

use crate::domain::repository::TaskBackend;
use crate::domain::task::{Task, TaskStatus, UnblockedTask};

/// Port trait for firing hook events after task state changes.
/// The implementation decides whether/how to actually fire hooks
/// (e.g., shell scripts, HTTP callbacks, no-op for tests).
#[async_trait]
pub trait HookExecutor: Send + Sync {
    async fn fire_task_hook(
        &self,
        event: &str,
        task: &Task,
        backend: &dyn TaskBackend,
        from_status: Option<TaskStatus>,
        unblocked: Option<Vec<UnblockedTask>>,
    );

    async fn fire_no_eligible_task_hook(
        &self,
        backend: &dyn TaskBackend,
        project_id: i64,
    );
}

/// No-op implementation for testing or when hooks are disabled.
pub struct NoOpHookExecutor;

#[async_trait]
impl HookExecutor for NoOpHookExecutor {
    async fn fire_task_hook(
        &self,
        _event: &str,
        _task: &Task,
        _backend: &dyn TaskBackend,
        _from_status: Option<TaskStatus>,
        _unblocked: Option<Vec<UnblockedTask>>,
    ) {
    }

    async fn fire_no_eligible_task_hook(
        &self,
        _backend: &dyn TaskBackend,
        _project_id: i64,
    ) {
    }
}
