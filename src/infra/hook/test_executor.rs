use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::application::hook_trigger::SelectResult;
use crate::application::port::{HookDataSource, HookTestPort};
use crate::domain::project::ProjectId;
use crate::domain::task::Task;
use crate::infra::config::Config;

use super::{
    BackendInfo, HookEnvelope, RuntimeMode, build_event, build_task_select_event,
    execute_hook_sync, get_commands_for_event, resolve_envelope_context,
};

/// Shell-based implementation of [`HookTestPort`].
pub struct ShellHookTestExecutor {
    config: Config,
    runtime_mode: RuntimeMode,
    backend_info: BackendInfo,
    backend: Arc<dyn HookDataSource>,
}

impl ShellHookTestExecutor {
    pub fn new(
        config: Config,
        runtime_mode: RuntimeMode,
        backend_info: BackendInfo,
        backend: Arc<dyn HookDataSource>,
    ) -> Self {
        Self {
            config,
            runtime_mode,
            backend_info,
            backend,
        }
    }
}

#[async_trait]
impl HookTestPort for ShellHookTestExecutor {
    async fn build_task_event_envelope(
        &self,
        _project_id: ProjectId,
        event_name: &str,
        task: &Task,
    ) -> Result<String> {
        let (envelope_project, envelope_user) =
            resolve_envelope_context(&self.config, self.backend.as_ref()).await;
        let event = build_event(event_name, task, self.backend.as_ref(), None, None).await;
        let envelope = HookEnvelope {
            runtime: self.runtime_mode,
            backend: self.backend_info.clone(),
            project: envelope_project,
            user: envelope_user,
            event,
        };
        Ok(serde_json::to_string_pretty(&envelope)?)
    }

    async fn build_task_select_envelope(
        &self,
        project_id: ProjectId,
        result: SelectResult,
    ) -> Result<String> {
        let (envelope_project, envelope_user) =
            resolve_envelope_context(&self.config, self.backend.as_ref()).await;
        let event = build_task_select_event(result, self.backend.as_ref(), project_id).await;
        let envelope = HookEnvelope {
            runtime: self.runtime_mode,
            backend: self.backend_info.clone(),
            project: envelope_project,
            user: envelope_user,
            event,
        };
        Ok(serde_json::to_string_pretty(&envelope)?)
    }

    fn get_commands(&self, event_name: &str) -> Option<Vec<String>> {
        get_commands_for_event(&self.config, event_name)
    }

    fn execute_sync(&self, command: &str, json: &str) -> Result<std::process::ExitStatus> {
        execute_hook_sync(command, json)
    }
}
