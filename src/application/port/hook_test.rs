use anyhow::Result;
use async_trait::async_trait;

use crate::application::hook_trigger::SelectResult;
use crate::domain::project::ProjectId;
use crate::domain::task::Task;

/// Port trait for hook test operations (CLI `hooks test` command).
/// Abstracts envelope construction, command lookup, and synchronous hook execution.
#[async_trait]
pub trait HookTestPort: Send + Sync {
    /// Build a JSON envelope for a task event (pretty-printed).
    async fn build_task_event_envelope(
        &self,
        project_id: ProjectId,
        event_name: &str,
        task: &Task,
    ) -> Result<String>;

    /// Build a JSON envelope for a `task_select` event (pretty-printed).
    async fn build_task_select_envelope(
        &self,
        project_id: ProjectId,
        result: SelectResult,
    ) -> Result<String>;

    /// Get configured hook commands for the given event name.
    /// Returns `None` for unrecognized event names.
    fn get_commands(&self, event_name: &str) -> Option<Vec<String>>;

    /// Execute a hook command synchronously with the given JSON payload.
    fn execute_sync(&self, command: &str, json: &str) -> Result<std::process::ExitStatus>;
}
