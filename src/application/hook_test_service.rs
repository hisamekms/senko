use std::sync::Arc;

use anyhow::Result;

use crate::domain::task::{Priority, Task, TaskStatus};

use super::hook_trigger::SelectResult;
use super::port::{HookDataSource, HookTestPort};

/// Result of a single hook command execution.
pub struct HookCommandResult {
    pub command: String,
    pub index: usize,
    pub total: usize,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

/// Output of a hook test operation.
pub enum HookTestOutput {
    /// Dry-run mode: returns the pretty-printed envelope JSON.
    DryRun { envelope_json: String },
    /// Hook commands were executed.
    Executed { results: Vec<HookCommandResult> },
    /// No hooks are configured for the event.
    NoHooksConfigured,
}

/// Application service for testing hook events.
/// Orchestrates envelope construction, command lookup, and synchronous execution.
pub struct HookTestService {
    backend: Arc<dyn HookDataSource>,
    hook_test: Arc<dyn HookTestPort>,
}

impl HookTestService {
    pub fn new(backend: Arc<dyn HookDataSource>, hook_test: Arc<dyn HookTestPort>) -> Self {
        Self { backend, hook_test }
    }

    /// Test a hook event by building the envelope and either returning it (dry-run)
    /// or executing configured hook commands synchronously.
    pub async fn test_event(
        &self,
        project_id: i64,
        event_name: &str,
        task_id: Option<crate::domain::task::TaskId>,
        dry_run: bool,
    ) -> Result<HookTestOutput> {
        // Build the envelope JSON
        let envelope_json = if event_name == "task_select" {
            // Task select envelope defaults to the `selected` branch for test
            // output unless the user constructs one explicitly; the structure
            // is the same either way.
            let result = match task_id {
                Some(_) => SelectResult::Selected,
                None => SelectResult::None,
            };
            self.hook_test
                .build_task_select_envelope(project_id, result)
                .await?
        } else {
            let task = self.resolve_task(project_id, task_id).await?;
            self.hook_test
                .build_task_event_envelope(project_id, event_name, &task)
                .await?
        };

        if dry_run {
            return Ok(HookTestOutput::DryRun { envelope_json });
        }

        // Get commands for the event
        let commands = self
            .hook_test
            .get_commands(event_name)
            .expect("already validated event name");

        if commands.is_empty() {
            return Ok(HookTestOutput::NoHooksConfigured);
        }

        // Build compact JSON for execution
        let compact_json: serde_json::Value = serde_json::from_str(&envelope_json)?;
        let compact_json = serde_json::to_string(&compact_json)?;

        // Execute each hook command synchronously
        let total = commands.len();
        let mut results = Vec::with_capacity(total);
        for (i, cmd) in commands.iter().enumerate() {
            let result = match self.hook_test.execute_sync(cmd, &compact_json) {
                Ok(status) => HookCommandResult {
                    command: cmd.clone(),
                    index: i + 1,
                    total,
                    exit_code: status.code(),
                    error: None,
                },
                Err(e) => HookCommandResult {
                    command: cmd.clone(),
                    index: i + 1,
                    total,
                    exit_code: None,
                    error: Some(format!("{e:#}")),
                },
            };
            results.push(result);
        }

        Ok(HookTestOutput::Executed { results })
    }

    /// Resolve the task for the hook test: fetch by ID or create a sample task.
    async fn resolve_task(
        &self,
        project_id: i64,
        task_id: Option<crate::domain::task::TaskId>,
    ) -> Result<Task> {
        if let Some(id) = task_id {
            self.backend.get_task(project_id, id).await
        } else {
            Ok(Task::new(
                crate::domain::task::TaskId(0),
                project_id,
                "Sample task".into(),
                None,
                Some("This is a sample task for hook testing".into()),
                None,
                Priority::P2,
                TaskStatus::Todo,
                None,
                None,
                chrono::Utc::now().to_rfc3339(),
                chrono::Utc::now().to_rfc3339(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
            ))
        }
    }
}
