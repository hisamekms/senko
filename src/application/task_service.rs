use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;

use crate::application::port::TaskBackend;
use crate::domain::error::DomainError;
use crate::domain::task::{
    self, CompletionPolicy, CreateTaskParams, ListTasksFilter, Task, TaskEvent, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::validator::has_cycle_async;

use super::HookTrigger;
use super::port::{HookExecutor, PrVerifier};

pub struct TaskService {
    backend: Arc<dyn TaskBackend>,
    hooks: Arc<dyn HookExecutor>,
    pr_verifier: Arc<dyn PrVerifier>,
    completion_policy: CompletionPolicy,
}

impl TaskService {
    pub fn new(
        backend: Arc<dyn TaskBackend>,
        hooks: Arc<dyn HookExecutor>,
        pr_verifier: Arc<dyn PrVerifier>,
        completion_policy: CompletionPolicy,
    ) -> Self {
        Self {
            backend,
            hooks,
            pr_verifier,
            completion_policy,
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
            let expanded = task::expand_branch_template(
                branch_template.as_deref().unwrap(),
                created.id(),
            );
            self.backend
                .update_task(
                    project_id,
                    created.id(),
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
            .fire(
                &HookTrigger::Task(TaskEvent::Created),
                Some(&task),
                None,
                None,
            )
            .await;

        Ok(task)
    }

    pub async fn ready_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let task = self.backend.get_task(project_id, id).await?;
        let prev_status = task.status();
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.ready(now)?;
        self.backend.save(&task).await?;

        self.hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Readied),
                Some(&task),
                Some(prev_status),
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
        let task = self.backend.get_task(project_id, id).await?;
        let prev_status = task.status();
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.start(session_id, user_id, now)?;
        self.backend.save(&task).await?;

        self.hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Started),
                Some(&task),
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
        let task = match self.backend.next_task(project_id).await? {
            Some(t) => t,
            None => {
                self.hooks
                    .fire(
                        &HookTrigger::NoEligibleTask { project_id },
                        None,
                        None,
                        None,
                    )
                    .await;
                return Err(DomainError::NoEligibleTask.into());
            }
        };

        let prev_status = task.status();

        // HttpBackend returns already-started tasks; skip start() in that case
        let task = if task.status() == TaskStatus::InProgress {
            task
        } else {
            let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            let (task, _events) = task.start(session_id, user_id, now)?;
            self.backend.save(&task).await?;
            task
        };

        self.hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Started),
                Some(&task),
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
        let task = self.backend.get_task(project_id, id).await?;

        // PR workflow checks (domain policy decides whether to check)
        if let Some(pr_url) = self.completion_policy.required_pr_url(&task, skip_pr_check)
            .map_err(|e| DomainError::CannotCompleteTask {
                task_id: id,
                reason: e.to_string(),
            })? {
            self.pr_verifier
                .verify_pr_status(pr_url, self.completion_policy.auto_merge())
                .map_err(|e| DomainError::CannotCompleteTask {
                    task_id: id,
                    reason: e.to_string(),
                })?;
        }

        // Capture ready tasks before completion for unblocked detection
        let prev_ready_ids: HashSet<i64> = self
            .backend
            .list_ready_tasks(project_id)
            .await?
            .iter()
            .map(|t| t.id())
            .collect();

        let prev_status = task.status();
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.complete(now)?;
        self.backend.save(&task).await?;

        // Compute unblocked tasks
        let curr_ready = self.backend.list_ready_tasks(project_id).await.unwrap_or_default();
        let unblocked = task::compute_unblocked(&curr_ready, &prev_ready_ids);
        let unblocked_opt = if unblocked.is_empty() {
            None
        } else {
            Some(unblocked)
        };

        self.hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Completed),
                Some(&task),
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
        let task = self.backend.get_task(project_id, id).await?;
        let prev_status = task.status();
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.cancel(now, reason)?;
        self.backend.save(&task).await?;

        self.hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Canceled),
                Some(&task),
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
        let task = self.backend.get_task(project_id, task_id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.check_dod(index, now)?;
        self.backend.save(&task).await?;
        Ok(task)
    }

    pub async fn uncheck_dod(
        &self,
        project_id: i64,
        task_id: i64,
        index: usize,
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, task_id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.uncheck_dod(index, now)?;
        self.backend.save(&task).await?;
        Ok(task)
    }

    pub async fn add_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, task_id).await?;
        // Verify dep exists
        let _ = self.backend.get_task(project_id, dep_id).await?;
        // Domain validation (self-dep check, consumed)
        let _ = task.add_dependency(dep_id, None).map(|(_, events)| events)?;

        // Cycle detection
        let backend = self.backend.clone();
        if has_cycle_async(task_id, dep_id, |id| {
            let backend = backend.clone();
            async move {
                backend
                    .list_dependencies(project_id, id)
                    .await
                    .map(|tasks| tasks.iter().map(|t| t.id()).collect())
                    .unwrap_or_default()
            }
        })
        .await
        {
            return Err(DomainError::DependencyCycle { dep_id }.into());
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
        let task = self.backend.get_task(project_id, task_id).await?;
        // Domain validation (self-dep check, consumed)
        let _ = task.set_dependencies(dep_ids, None).map(|(_, events)| events)?;

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
                        .map(|tasks| tasks.iter().map(|t| t.id()).collect())
                        .unwrap_or_default()
                }
            })
            .await
            {
                return Err(DomainError::DependencyCycle { dep_id }.into());
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

