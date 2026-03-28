use async_trait::async_trait;

use crate::application::port::HookExecutor;
use crate::domain::repository::TaskBackend;
use crate::domain::config::Config;
use crate::domain::task::{Task, TaskStatus, UnblockedTask};

use super::{fire_hooks, fire_no_eligible_task_hooks};

/// Shell-based hook executor that spawns hook commands as child processes.
/// Respects the `should_fire` flag to control whether hooks actually execute.
pub struct ShellHookExecutor {
    config: Config,
    should_fire: bool,
}

impl ShellHookExecutor {
    pub fn new(config: Config, should_fire: bool) -> Self {
        Self {
            config,
            should_fire,
        }
    }
}

#[async_trait]
impl HookExecutor for ShellHookExecutor {
    async fn fire_task_hook(
        &self,
        event: &str,
        task: &Task,
        backend: &dyn TaskBackend,
        from_status: Option<TaskStatus>,
        unblocked: Option<Vec<UnblockedTask>>,
    ) {
        if !self.should_fire {
            return;
        }
        fire_hooks(&self.config, event, task, backend, from_status, unblocked).await;
    }

    async fn fire_no_eligible_task_hook(
        &self,
        backend: &dyn TaskBackend,
        project_id: i64,
    ) {
        if !self.should_fire {
            return;
        }
        fire_no_eligible_task_hooks(&self.config, backend, project_id).await;
    }
}
