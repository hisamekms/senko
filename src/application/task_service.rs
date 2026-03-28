use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{bail, Result};
use chrono::Utc;

use crate::backend::TaskBackend;
use crate::domain::config::{CompletionMode, WorkflowConfig};
use crate::domain::task::{
    CreateTaskParams, ListTasksFilter, Task, TaskStatus, UnblockedTask, UpdateTaskArrayParams,
    UpdateTaskParams,
};
use crate::domain::validator::has_cycle_async;

use super::port::{HookExecutor, PrVerifier};

pub struct TaskService {
    backend: Arc<dyn TaskBackend>,
    hooks: Arc<dyn HookExecutor>,
    pr_verifier: Arc<dyn PrVerifier>,
    workflow: WorkflowConfig,
}

impl TaskService {
    pub fn new(
        backend: Arc<dyn TaskBackend>,
        hooks: Arc<dyn HookExecutor>,
        pr_verifier: Arc<dyn PrVerifier>,
        workflow: WorkflowConfig,
    ) -> Self {
        Self {
            backend,
            hooks,
            pr_verifier,
            workflow,
        }
    }

    pub fn backend(&self) -> &dyn TaskBackend {
        self.backend.as_ref()
    }

    // --- Task CRUD with business logic ---

    pub async fn create_task(
        &self,
        project_id: i64,
        params: &CreateTaskParams,
    ) -> Result<Task> {
        let needs_template = params
            .branch
            .as_ref()
            .is_some_and(|b| b.contains("${task_id}"));

        let task = if needs_template {
            let branch_template = params.branch.clone();
            let mut params_without_branch = params.clone();
            params_without_branch.branch = None;
            let created = self
                .backend
                .create_task(project_id, &params_without_branch)
                .await?;
            let expanded = expand_branch_template(
                branch_template.as_deref().unwrap(),
                created.id,
            );
            self.backend
                .update_task(
                    project_id,
                    created.id,
                    &UpdateTaskParams {
                        title: None,
                        background: None,
                        description: None,
                        plan: None,
                        priority: None,
                        assignee_session_id: None,
                        assignee_user_id: None,
                        started_at: None,
                        completed_at: None,
                        canceled_at: None,
                        cancel_reason: None,
                        branch: Some(Some(expanded)),
                        pr_url: None,
                        metadata: None,
                    },
                )
                .await?
        } else {
            self.backend.create_task(project_id, params).await?
        };

        self.hooks
            .fire_task_hook("task_added", &task, self.backend.as_ref(), None, None)
            .await;

        Ok(task)
    }

    pub async fn ready_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let mut task = self.backend.get_task(project_id, id).await?;
        task.ready()?;
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.backend.save(&task).await?;

        self.hooks
            .fire_task_hook(
                "task_ready",
                &task,
                self.backend.as_ref(),
                Some(TaskStatus::Draft),
                None,
            )
            .await;

        Ok(task)
    }

    pub async fn start_task(
        &self,
        project_id: i64,
        id: i64,
        session_id: Option<String>,
        user_id: Option<i64>,
    ) -> Result<Task> {
        let mut task = self.backend.get_task(project_id, id).await?;
        let prev_status = task.status;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        task.start(session_id, user_id, now.clone())?;
        task.updated_at = now;
        self.backend.save(&task).await?;

        self.hooks
            .fire_task_hook(
                "task_started",
                &task,
                self.backend.as_ref(),
                Some(prev_status),
                None,
            )
            .await;

        Ok(task)
    }

    pub async fn next_task(
        &self,
        project_id: i64,
        session_id: Option<String>,
        user_id: Option<i64>,
    ) -> Result<Task> {
        let mut task = match self.backend.next_task(project_id).await? {
            Some(t) => t,
            None => {
                self.hooks
                    .fire_no_eligible_task_hook(self.backend.as_ref(), project_id)
                    .await;
                bail!("no eligible task found");
            }
        };

        let prev_status = task.status;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        task.start(session_id, user_id, now.clone())?;
        task.updated_at = now;
        self.backend.save(&task).await?;

        self.hooks
            .fire_task_hook(
                "task_started",
                &task,
                self.backend.as_ref(),
                Some(prev_status),
                None,
            )
            .await;

        Ok(task)
    }

    pub async fn complete_task(
        &self,
        project_id: i64,
        id: i64,
        skip_pr_check: bool,
    ) -> Result<Task> {
        let mut task = self.backend.get_task(project_id, id).await?;
        task.status.transition_to(TaskStatus::Completed)?;

        // Validate all DoD items are checked
        let unchecked: Vec<_> = task
            .definition_of_done
            .iter()
            .filter(|d| !d.checked)
            .collect();
        if !unchecked.is_empty() {
            bail!(
                "cannot complete task #{}: {} unchecked DoD item(s)",
                id,
                unchecked.len()
            );
        }

        // PR workflow checks
        if !skip_pr_check
            && self.workflow.completion_mode == CompletionMode::PrThenComplete
        {
            let pr_url = task.pr_url.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot complete task #{}: completion_mode is pr_then_complete but no pr_url is set. \
                     Use `localflow edit {} --pr-url <url>` to set it.",
                    id,
                    id
                )
            })?;

            self.pr_verifier
                .verify_pr_status(pr_url, self.workflow.auto_merge)?;
        }

        // Capture ready tasks before completion for unblocked detection
        let prev_ready_ids: HashSet<i64> = self
            .backend
            .list_ready_tasks(project_id)
            .await?
            .iter()
            .map(|t| t.id)
            .collect();

        let prev_status = task.status;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        task.complete(now.clone())?;
        task.updated_at = now;
        self.backend.save(&task).await?;

        // Compute unblocked tasks
        let unblocked = compute_unblocked(self.backend.as_ref(), project_id, &prev_ready_ids).await;
        let unblocked_opt = if unblocked.is_empty() {
            None
        } else {
            Some(unblocked)
        };

        self.hooks
            .fire_task_hook(
                "task_completed",
                &task,
                self.backend.as_ref(),
                Some(prev_status),
                unblocked_opt,
            )
            .await;

        Ok(task)
    }

    pub async fn cancel_task(
        &self,
        project_id: i64,
        id: i64,
        reason: Option<String>,
    ) -> Result<Task> {
        let mut task = self.backend.get_task(project_id, id).await?;
        let prev_status = task.status;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        task.cancel(now.clone(), reason)?;
        task.updated_at = now;
        self.backend.save(&task).await?;

        self.hooks
            .fire_task_hook(
                "task_canceled",
                &task,
                self.backend.as_ref(),
                Some(prev_status),
                None,
            )
            .await;

        Ok(task)
    }

    // --- Passthrough methods (no hooks) ---

    pub async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        self.backend.get_task(project_id, id).await
    }

    pub async fn list_tasks(
        &self,
        project_id: i64,
        filter: &ListTasksFilter,
    ) -> Result<Vec<Task>> {
        self.backend.list_tasks(project_id, filter).await
    }

    pub async fn task_stats(
        &self,
        project_id: i64,
    ) -> Result<std::collections::HashMap<String, i64>> {
        self.backend.task_stats(project_id).await
    }

    pub async fn edit_task(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskParams,
    ) -> Result<Task> {
        self.backend.update_task(project_id, id, params).await
    }

    pub async fn edit_task_arrays(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskArrayParams,
    ) -> Result<()> {
        self.backend.update_task_arrays(project_id, id, params).await
    }

    pub async fn delete_task(&self, project_id: i64, id: i64) -> Result<()> {
        self.backend.delete_task(project_id, id).await
    }

    pub async fn check_dod(
        &self,
        project_id: i64,
        task_id: i64,
        index: usize,
    ) -> Result<Task> {
        let mut task = self.backend.get_task(project_id, task_id).await?;
        task.check_dod(index)?;
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.backend.save(&task).await?;
        Ok(task)
    }

    pub async fn uncheck_dod(
        &self,
        project_id: i64,
        task_id: i64,
        index: usize,
    ) -> Result<Task> {
        let mut task = self.backend.get_task(project_id, task_id).await?;
        task.uncheck_dod(index)?;
        task.updated_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.backend.save(&task).await?;
        Ok(task)
    }

    pub async fn add_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        let mut task = self.backend.get_task(project_id, task_id).await?;
        // Verify dep exists
        let _ = self.backend.get_task(project_id, dep_id).await?;
        // Domain validation (self-dep check)
        task.add_dependency(dep_id)?;

        // Cycle detection
        let backend = self.backend.clone();
        if has_cycle_async(task_id, dep_id, |id| {
            let backend = backend.clone();
            async move {
                backend
                    .list_dependencies(project_id, id)
                    .await
                    .map(|tasks| tasks.iter().map(|t| t.id).collect())
                    .unwrap_or_default()
            }
        })
        .await
        {
            bail!("adding this dependency would create a cycle");
        }

        self.backend
            .add_dependency(project_id, task_id, dep_id)
            .await
    }

    pub async fn remove_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        self.backend
            .remove_dependency(project_id, task_id, dep_id)
            .await
    }

    pub async fn set_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
        dep_ids: &[i64],
    ) -> Result<Task> {
        let mut task = self.backend.get_task(project_id, task_id).await?;
        // Domain validation (self-dep check)
        task.set_dependencies(dep_ids)?;

        // Verify all deps exist
        for &dep_id in dep_ids {
            let _ = self.backend.get_task(project_id, dep_id).await?;
        }

        // Cycle detection for each new dependency
        for &dep_id in dep_ids {
            let backend = self.backend.clone();
            if has_cycle_async(task_id, dep_id, |id| {
                let backend = backend.clone();
                async move {
                    backend
                        .list_dependencies(project_id, id)
                        .await
                        .map(|tasks| tasks.iter().map(|t| t.id).collect())
                        .unwrap_or_default()
                }
            })
            .await
            {
                bail!("adding dependency on {} would create a cycle", dep_id);
            }
        }

        self.backend
            .set_dependencies(project_id, task_id, dep_ids)
            .await
    }

    pub async fn list_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
    ) -> Result<Vec<Task>> {
        self.backend.list_dependencies(project_id, task_id).await
    }

    pub async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        self.backend.list_ready_tasks(project_id).await
    }

    pub async fn ready_count(&self, project_id: i64) -> Result<i64> {
        self.backend.ready_count(project_id).await
    }
}

fn expand_branch_template(branch: &str, task_id: i64) -> String {
    branch.replace("${task_id}", &task_id.to_string())
}

async fn compute_unblocked(
    backend: &dyn TaskBackend,
    project_id: i64,
    prev_ready_ids: &HashSet<i64>,
) -> Vec<UnblockedTask> {
    let curr_ready = backend
        .list_ready_tasks(project_id)
        .await
        .unwrap_or_default();
    curr_ready
        .iter()
        .filter(|t| !prev_ready_ids.contains(&t.id))
        .map(|t| UnblockedTask {
            id: t.id,
            title: t.title.clone(),
            priority: t.priority,
            metadata: t.metadata.clone(),
        })
        .collect()
}
