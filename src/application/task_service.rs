use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

use crate::application::port::TaskBackend;
use crate::domain::error::DomainError;
use crate::domain::task::{
    self, CompletionPolicy, CreateTaskParams, ListTasksFilter, Task, TaskEvent, TaskStatus,
    UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::validator::has_cycle_async;

use super::HookTrigger;
use super::port::{HookExecutor, PreviewResult, PrVerifier, TaskOperations};

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

    /// Find tasks that would become ready if the given task were completed.
    async fn compute_would_be_unblocked(
        &self,
        project_id: i64,
        completing_task_id: i64,
    ) -> Result<Vec<Task>> {
        let all_tasks = self
            .backend
            .list_tasks(project_id, &ListTasksFilter::default())
            .await?;
        let mut result = Vec::new();

        for t in &all_tasks {
            if !t.dependencies().contains(&completing_task_id) {
                continue;
            }
            // Only consider tasks that are waiting (draft or todo)
            if t.status() != TaskStatus::Draft && t.status() != TaskStatus::Todo {
                continue;
            }
            // Check if all other deps are completed
            let all_other_done = t
                .dependencies()
                .iter()
                .filter(|&&dep_id| dep_id != completing_task_id)
                .all(|&dep_id| {
                    all_tasks
                        .iter()
                        .find(|tt| tt.id() == dep_id)
                        .is_some_and(|tt| tt.status() == TaskStatus::Completed)
                });
            if all_other_done {
                result.push(t.clone());
            }
        }

        Ok(result)
    }
}

#[async_trait]
impl TaskOperations for TaskService {
    fn backend(&self) -> &dyn TaskBackend {
        self.backend.as_ref()
    }

    // --- Task CRUD with business logic ---

    async fn create_task(
        &self,
        project_id: i64,
        params: &CreateTaskParams,
    ) -> Result<Task> {
        let task = self.backend.create_task(project_id, params).await?;

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

    async fn ready_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let prev_status = self.backend.get_task(project_id, id).await?.status();
        let task = self.backend.ready_task(project_id, id).await?;

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

    async fn start_task(
        &self,
        project_id: i64,
        id: i64,
        session_id: Option<String>,
        user_id: Option<i64>,
    ) -> Result<Task> {
        let prev_status = self.backend.get_task(project_id, id).await?.status();
        let task = self.backend.start_task(project_id, id, session_id, user_id).await?;

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

    async fn next_task(
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
            self.backend.start_task(project_id, task.id(), session_id, user_id).await?
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

    async fn complete_task(
        &self,
        project_id: i64,
        id: i64,
        skip_pr_check: bool,
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, id).await?;

        // PR workflow checks (domain policy decides whether to check).
        // For HttpBackend mode, NoOpPrVerifier is used since the API server
        // handles PR verification server-side via its own GhCliPrVerifier.
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
        // TaskTransitionPort::complete_task handles both local (domain complete + save)
        // and HttpBackend (POST /complete with server-side PR verification).
        let task = self.backend.complete_task(project_id, id, skip_pr_check).await?;

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

    async fn cancel_task(
        &self,
        project_id: i64,
        id: i64,
        reason: Option<String>,
    ) -> Result<Task> {
        let prev_status = self.backend.get_task(project_id, id).await?.status();
        let task = self.backend.cancel_task(project_id, id, reason).await?;

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

    async fn preview_transition(
        &self,
        project_id: i64,
        task_id: i64,
        target: TaskStatus,
    ) -> Result<PreviewResult> {
        let task = self.backend.get_task(project_id, task_id).await?;
        let mut operations = Vec::new();

        // Check basic transition validity
        let allowed = task.status().can_transition_to(target);
        if !allowed {
            return Ok(PreviewResult {
                allowed: false,
                reason: Some(format!(
                    "invalid status transition: {} → {}",
                    task.status(),
                    target
                )),
                task,
                target_status: target,
                operations,
                unblocked_tasks: vec![],
            });
        }

        operations.push(format!(
            "Change task #{} status: {} → {}",
            task_id,
            task.status(),
            target
        ));

        // For completion: check DoD items
        if target == TaskStatus::Completed {
            let unchecked = task
                .definition_of_done()
                .iter()
                .filter(|d| !d.checked())
                .count();
            if unchecked > 0 {
                return Ok(PreviewResult {
                    allowed: false,
                    reason: Some(format!("{} unchecked DoD item(s)", unchecked)),
                    task,
                    target_status: target,
                    operations,
                    unblocked_tasks: vec![],
                });
            }

            // Check PR requirements
            match self.completion_policy.required_pr_url(&task, false) {
                Err(e) => {
                    return Ok(PreviewResult {
                        allowed: false,
                        reason: Some(e.to_string()),
                        task,
                        target_status: target,
                        operations,
                        unblocked_tasks: vec![],
                    });
                }
                Ok(Some(pr_url)) => {
                    operations.push(format!("Verify PR status: {}", pr_url));
                }
                Ok(None) => {}
            }
        }

        // For completion: compute would-be-unblocked tasks
        let unblocked_tasks = if target == TaskStatus::Completed {
            self.compute_would_be_unblocked(project_id, task_id)
                .await
                .unwrap_or_default()
        } else {
            vec![]
        };

        for t in &unblocked_tasks {
            operations.push(format!("Unblock task #{}: \"{}\"", t.id(), t.title()));
        }

        Ok(PreviewResult {
            allowed: true,
            reason: None,
            task,
            target_status: target,
            operations,
            unblocked_tasks,
        })
    }

    async fn preview_next(&self, project_id: i64) -> Result<PreviewResult> {
        let task = match self.backend.next_task(project_id).await? {
            Some(t) => t,
            None => return Err(DomainError::NoEligibleTask.into()),
        };

        let operations = vec![
            format!(
                "Start next eligible task #{}: \"{}\"",
                task.id(),
                task.title()
            ),
            format!("Change status: {} → in_progress", task.status()),
        ];

        Ok(PreviewResult {
            allowed: true,
            reason: None,
            task,
            target_status: TaskStatus::InProgress,
            operations,
            unblocked_tasks: vec![],
        })
    }

    // --- Passthrough methods (no hooks) ---

    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        self.backend.get_task(project_id, id).await
    }

    async fn list_tasks(
        &self,
        project_id: i64,
        filter: &ListTasksFilter,
    ) -> Result<Vec<Task>> {
        self.backend.list_tasks(project_id, filter).await
    }

    async fn list_all_tags(&self, project_id: i64) -> Result<Vec<String>> {
        let tasks = self
            .backend
            .list_tasks(project_id, &ListTasksFilter::default())
            .await?;
        let tags: Vec<String> = tasks
            .iter()
            .flat_map(|t| t.tags().iter().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(tags)
    }

    async fn task_stats(
        &self,
        project_id: i64,
    ) -> Result<std::collections::HashMap<String, i64>> {
        self.backend.task_stats(project_id).await
    }

    async fn edit_task(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskParams,
    ) -> Result<Task> {
        self.backend.update_task(project_id, id, params).await
    }

    async fn edit_task_arrays(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskArrayParams,
    ) -> Result<()> {
        self.backend.update_task_arrays(project_id, id, params).await
    }

    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()> {
        self.backend.delete_task(project_id, id).await
    }

    async fn check_dod(
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

    async fn uncheck_dod(
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

    async fn add_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, task_id).await?;
        // Verify dep exists
        let _ = self.backend.get_task(project_id, dep_id).await?;

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

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.add_dependency(dep_id, Some(now))?;
        self.backend.save(&task).await?;
        self.backend.get_task(project_id, task_id).await
    }

    async fn remove_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, task_id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.remove_dependency(dep_id, Some(now))?;
        self.backend.save(&task).await?;
        self.backend.get_task(project_id, task_id).await
    }

    async fn set_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
        dep_ids: &[i64],
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, task_id).await?;

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

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, _events) = task.set_dependencies(dep_ids, Some(now))?;
        self.backend.save(&task).await?;
        self.backend.get_task(project_id, task_id).await
    }

    async fn list_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
    ) -> Result<Vec<Task>> {
        self.backend.list_dependencies(project_id, task_id).await
    }

    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        self.backend.list_ready_tasks(project_id).await
    }

    async fn ready_count(&self, project_id: i64) -> Result<i64> {
        self.backend.ready_count(project_id).await
    }
}

