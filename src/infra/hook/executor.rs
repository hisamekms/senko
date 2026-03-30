use async_trait::async_trait;

use crate::application::port::HookExecutor;
use crate::application::HookTrigger;
use crate::domain::repository::TaskBackend;
use crate::infra::config::Config;
use crate::domain::task::{Task, TaskStatus, UnblockedTask};

use super::{fire_hooks, fire_no_eligible_task_hooks, RuntimeMode, BackendInfo};

/// Shell-based hook executor that spawns hook commands as child processes.
/// Respects the `should_fire` flag to control whether hooks actually execute.
pub struct ShellHookExecutor {
    config: Config,
    should_fire: bool,
    runtime_mode: RuntimeMode,
    backend_info: BackendInfo,
}

impl ShellHookExecutor {
    pub fn new(config: Config, should_fire: bool, runtime_mode: RuntimeMode, backend_info: BackendInfo) -> Self {
        Self {
            config,
            should_fire,
            runtime_mode,
            backend_info,
        }
    }
}

#[async_trait]
impl HookExecutor for ShellHookExecutor {
    async fn fire(
        &self,
        trigger: &HookTrigger,
        task: Option<&Task>,
        backend: &dyn TaskBackend,
        from_status: Option<TaskStatus>,
        unblocked: Option<Vec<UnblockedTask>>,
    ) {
        if !self.should_fire {
            return;
        }
        let Some(event_name) = trigger.event_name() else {
            return;
        };
        match trigger {
            HookTrigger::Task(_) => {
                let task = task.expect("task required for Task hook trigger");
                fire_hooks(
                    &self.config, event_name, task, backend,
                    from_status, unblocked,
                    &self.runtime_mode, &self.backend_info,
                ).await;
            }
            HookTrigger::NoEligibleTask { project_id } => {
                fire_no_eligible_task_hooks(
                    &self.config, backend, *project_id,
                    &self.runtime_mode, &self.backend_info,
                ).await;
            }
        }
    }
}
