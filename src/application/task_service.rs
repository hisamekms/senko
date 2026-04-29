use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

use crate::application::port::TaskBackend;
use crate::domain::contract::ContractId;
use crate::domain::error::DomainError;
use crate::domain::metadata_field::{ListMetadataFieldsFilter, MetadataField, MetadataFieldType};
use crate::domain::pagination::ListPage;
use crate::domain::project::ProjectId;
use crate::domain::task::{
    self, CompletionPolicy, CreateTaskParams, ListTaskDepsFilter, ListTasksFilter, ListTasksPage,
    MetadataUpdate, Task, TaskEvent, TaskId, TaskStatus, UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::UserId;
use crate::domain::validator::{has_cycle_async, validate_metadata, validate_metadata_on_complete};

use super::HookTrigger;
use super::hook_trigger::SelectResult;
use super::port::{CompleteResult, HookExecutor, PrVerifier, PreviewResult, TaskOperations};
use crate::infra::config::HookWhen;
use crate::infra::hook::FireOutcome;

/// Emit Contract #8 business events for a `LocalTaskOperations` mutation that
/// returns a `Vec<TaskEvent>` (`edit_task`, `edit_task_arrays`, DoD ops, dep
/// ops). State-transition events (Created/Published/Started/Completed/Canceled)
/// are emitted inline by the caller so that `from_status` / `to_status` /
/// `cancel_reason` can be sourced from the surrounding scope.
///
/// `contract_id` is the **post-mutation** `Option<ContractId>` taken from the
/// just-saved Task; `senko.task.updated` therefore carries the new value when
/// the update itself changed `contract_id`.
fn emit_task_events(
    project_id: ProjectId,
    task_id: TaskId,
    contract_id: Option<ContractId>,
    events: &[TaskEvent],
) {
    for ev in events {
        match ev {
            TaskEvent::Updated { changed_fields } => {
                let changed_fields_json = serde_json::to_string(changed_fields).unwrap_or_default();
                crate::emit_task_event!(
                    "senko.task.updated",
                    contract_id = contract_id,
                    senko.task.id = task_id.0,
                    senko.project.id = project_id.0,
                    changed_fields = changed_fields_json.as_str(),
                );
            }
            TaskEvent::DodChecked { index } => {
                crate::emit_task_event!(
                    "senko.task.dod_checked",
                    contract_id = contract_id,
                    senko.task.id = task_id.0,
                    senko.project.id = project_id.0,
                    dod_index = *index as i64,
                );
            }
            TaskEvent::DodUnchecked { index } => {
                crate::emit_task_event!(
                    "senko.task.dod_unchecked",
                    contract_id = contract_id,
                    senko.task.id = task_id.0,
                    senko.project.id = project_id.0,
                    dod_index = *index as i64,
                );
            }
            TaskEvent::DependencyAdded { dep_id } => {
                crate::emit_task_event!(
                    "senko.task.dependency_added",
                    contract_id = contract_id,
                    senko.task.id = task_id.0,
                    senko.project.id = project_id.0,
                    dep_id = dep_id.0,
                );
            }
            TaskEvent::DependencyRemoved { dep_id } => {
                crate::emit_task_event!(
                    "senko.task.dependency_removed",
                    contract_id = contract_id,
                    senko.task.id = task_id.0,
                    senko.project.id = project_id.0,
                    dep_id = dep_id.0,
                );
            }
            TaskEvent::DependenciesSet { dep_ids } => {
                let deps_json =
                    serde_json::to_string(&dep_ids.iter().map(|d| d.0).collect::<Vec<_>>())
                        .unwrap_or_default();
                crate::emit_task_event!(
                    "senko.task.dependencies_set",
                    contract_id = contract_id,
                    senko.task.id = task_id.0,
                    senko.project.id = project_id.0,
                    deps = deps_json.as_str(),
                );
            }
            // State-transition events emitted inline at the mutation site.
            TaskEvent::Created
            | TaskEvent::Published
            | TaskEvent::Started
            | TaskEvent::Resumed
            | TaskEvent::Completed
            | TaskEvent::Canceled => {}
        }
    }
}

pub struct LocalTaskOperations {
    backend: Arc<dyn TaskBackend>,
    hooks: Arc<dyn HookExecutor>,
    pr_verifier: Arc<dyn PrVerifier>,
    completion_policy: CompletionPolicy,
}

impl LocalTaskOperations {
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
        project_id: ProjectId,
        completing_task_id: TaskId,
    ) -> Result<Vec<Task>> {
        let all_tasks = self
            .backend
            .list_tasks(project_id, &ListTasksFilter::default())
            .await?
            .items;
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
impl TaskOperations for LocalTaskOperations {
    // --- Task CRUD with business logic ---

    async fn create_task(&self, project_id: ProjectId, params: &CreateTaskParams) -> Result<Task> {
        params.validate()?;
        if let Some(ref metadata) = params.metadata {
            validate_metadata(metadata)?;
        }

        // Pre hook: a preview task object is not available; pass None.
        let trigger = HookTrigger::Task(TaskEvent::Created);
        if self
            .hooks
            .fire(&trigger, HookWhen::Pre, None, None, None)
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "task_add".into(),
            }
            .into());
        }

        let task = self.backend.create_task(project_id, params).await?;

        let _ = self
            .hooks
            .fire(&trigger, HookWhen::Post, Some(&task), None, None)
            .await;

        crate::emit_task_event!(
            "senko.task.created",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
        );

        Ok(task)
    }

    async fn publish_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task> {
        let prev = self.backend.get_task(project_id, id).await?;
        let prev_status = prev.status();

        let trigger = HookTrigger::Task(TaskEvent::Published);
        if self
            .hooks
            .fire(
                &trigger,
                HookWhen::Pre,
                Some(&prev),
                Some(prev_status),
                None,
            )
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "task_publish".into(),
            }
            .into());
        }

        let task = self.backend.publish_task(project_id, id).await?;

        let _ = self
            .hooks
            .fire(
                &trigger,
                HookWhen::Post,
                Some(&task),
                Some(prev_status),
                None,
            )
            .await;

        crate::emit_task_event!(
            "senko.task.published",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            from_status = %prev_status,
            to_status = %task.status(),
        );

        Ok(task)
    }

    async fn start_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        session_id: Option<String>,
        user_id: Option<UserId>,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task> {
        if let Some(ref sid) = session_id {
            crate::domain::validator::validate_string_length(
                "session_id",
                sid,
                crate::domain::validator::MAX_SESSION_ID_LEN,
            )?;
        }
        match &metadata {
            Some(MetadataUpdate::Merge(v)) | Some(MetadataUpdate::Replace(v)) => {
                validate_metadata(v)?
            }
            _ => {}
        }
        let prev = self.backend.get_task(project_id, id).await?;
        let prev_status = prev.status();

        let trigger = HookTrigger::Task(TaskEvent::Started);
        if self
            .hooks
            .fire(
                &trigger,
                HookWhen::Pre,
                Some(&prev),
                Some(prev_status),
                None,
            )
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "task_start".into(),
            }
            .into());
        }

        let task = self
            .backend
            .start_task(project_id, id, session_id, user_id, metadata)
            .await?;

        let _ = self
            .hooks
            .fire(
                &trigger,
                HookWhen::Post,
                Some(&task),
                Some(prev_status),
                None,
            )
            .await;

        crate::emit_task_event!(
            "senko.task.started",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            from_status = %prev_status,
            to_status = %task.status(),
        );

        Ok(task)
    }

    async fn resume_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        session_id: Option<String>,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task> {
        if let Some(ref sid) = session_id {
            crate::domain::validator::validate_string_length(
                "session_id",
                sid,
                crate::domain::validator::MAX_SESSION_ID_LEN,
            )?;
        }
        match &metadata {
            Some(MetadataUpdate::Merge(v)) | Some(MetadataUpdate::Replace(v)) => {
                validate_metadata(v)?
            }
            _ => {}
        }
        let prev = self.backend.get_task(project_id, id).await?;
        let prev_status = prev.status();

        let trigger = HookTrigger::Task(TaskEvent::Resumed);
        if self
            .hooks
            .fire(
                &trigger,
                HookWhen::Pre,
                Some(&prev),
                Some(prev_status),
                None,
            )
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "task_resume".into(),
            }
            .into());
        }

        let task = self
            .backend
            .resume_task(project_id, id, session_id, metadata)
            .await?;

        let _ = self
            .hooks
            .fire(
                &trigger,
                HookWhen::Post,
                Some(&task),
                Some(prev_status),
                None,
            )
            .await;

        crate::emit_task_event!(
            "senko.task.resumed",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            from_status = %prev_status,
            to_status = %task.status(),
        );

        Ok(task)
    }

    async fn next_task(
        &self,
        project_id: ProjectId,
        session_id: Option<String>,
        user_id: Option<UserId>,
        include_unassigned: bool,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task> {
        if let Some(ref sid) = session_id {
            crate::domain::validator::validate_string_length(
                "session_id",
                sid,
                crate::domain::validator::MAX_SESSION_ID_LEN,
            )?;
        }
        match &metadata {
            Some(MetadataUpdate::Merge(v)) | Some(MetadataUpdate::Replace(v)) => {
                validate_metadata(v)?
            }
            _ => {}
        }
        let task = match self
            .backend
            .next_task(project_id, user_id, include_unassigned)
            .await?
        {
            Some(t) => t,
            None => {
                let _ = self
                    .hooks
                    .fire(
                        &HookTrigger::TaskSelect {
                            project_id,
                            result: SelectResult::None,
                        },
                        HookWhen::Post,
                        None,
                        None,
                        None,
                    )
                    .await;
                return Err(DomainError::NoEligibleTask.into());
            }
        };

        let prev_status = task.status();

        // task_select post-hook fires on success before the start transition.
        let _ = self
            .hooks
            .fire(
                &HookTrigger::TaskSelect {
                    project_id,
                    result: SelectResult::Selected,
                },
                HookWhen::Post,
                Some(&task),
                None,
                None,
            )
            .await;

        // Remote server returns already-started tasks; skip start() in that case
        let task = if task.status() == TaskStatus::InProgress {
            task
        } else {
            let start_trigger = HookTrigger::Task(TaskEvent::Started);
            if self
                .hooks
                .fire(
                    &start_trigger,
                    HookWhen::Pre,
                    Some(&task),
                    Some(prev_status),
                    None,
                )
                .await
                == FireOutcome::Abort
            {
                return Err(DomainError::HookAborted {
                    event: "task_start".into(),
                }
                .into());
            }
            self.backend
                .start_task(project_id, task.id(), session_id, user_id, metadata)
                .await?
        };

        let _ = self
            .hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Started),
                HookWhen::Post,
                Some(&task),
                Some(prev_status),
                None,
            )
            .await;

        // Only emit `senko.task.started` when the selection actually performed
        // the transition. If the upstream already returned an InProgress task
        // (Relay path), the corresponding emit happened on the upstream side.
        if prev_status != TaskStatus::InProgress {
            crate::emit_task_event!(
                "senko.task.started",
                contract_id = task.contract_id(),
                senko.task.id = task.id().0,
                senko.project.id = project_id.0,
                from_status = %prev_status,
                to_status = %task.status(),
            );
        }

        Ok(task)
    }

    async fn complete_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        skip_pr_check: bool,
    ) -> Result<CompleteResult> {
        let task = self.backend.get_task(project_id, id).await?;

        // PR workflow checks (domain policy decides whether to check).
        if let Some(pr_url) = self
            .completion_policy
            .required_pr_url(&task, skip_pr_check)
            .map_err(|e| DomainError::CannotCompleteTask {
                task_id: id,
                reason: e.to_string(),
            })?
        {
            self.pr_verifier.verify_pr_status(pr_url).map_err(|e| {
                DomainError::CannotCompleteTask {
                    task_id: id,
                    reason: e.to_string(),
                }
            })?;
        }

        // Metadata field validation
        let metadata_fields = self
            .backend
            .list_metadata_fields(project_id, &ListMetadataFieldsFilter::default())
            .await?
            .items;
        validate_metadata_on_complete(task.metadata(), &metadata_fields, id)?;

        // Capture ready tasks before completion for unblocked detection
        let prev_ready_ids: HashSet<TaskId> = self
            .backend
            .list_ready_tasks(project_id)
            .await?
            .iter()
            .map(|t| t.id())
            .collect();

        let prev_status = task.status();

        let complete_trigger = HookTrigger::Task(TaskEvent::Completed);
        if self
            .hooks
            .fire(
                &complete_trigger,
                HookWhen::Pre,
                Some(&task),
                Some(prev_status),
                None,
            )
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "task_complete".into(),
            }
            .into());
        }

        // TaskTransitionPort::complete_task handles both local (domain complete + save)
        // and RemoteTaskOperations (POST /complete with server-side PR verification).
        let task = self
            .backend
            .complete_task(project_id, id, skip_pr_check)
            .await?;

        // Compute unblocked tasks
        let curr_ready = self
            .backend
            .list_ready_tasks(project_id)
            .await
            .unwrap_or_default();
        let unblocked = task::compute_unblocked(&curr_ready, &prev_ready_ids);
        let unblocked_opt = if unblocked.is_empty() {
            None
        } else {
            Some(unblocked.clone())
        };

        let _ = self
            .hooks
            .fire(
                &complete_trigger,
                HookWhen::Post,
                Some(&task),
                Some(prev_status),
                unblocked_opt,
            )
            .await;

        crate::emit_task_event!(
            "senko.task.completed",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            from_status = %prev_status,
            to_status = %task.status(),
        );

        Ok(CompleteResult { task, unblocked })
    }

    async fn cancel_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        reason: Option<String>,
    ) -> Result<Task> {
        if let Some(ref r) = reason {
            crate::domain::validator::validate_string_length(
                "reason",
                r,
                crate::domain::validator::MAX_LONG_TEXT_LEN,
            )?;
        }
        let prev = self.backend.get_task(project_id, id).await?;
        let prev_status = prev.status();

        let trigger = HookTrigger::Task(TaskEvent::Canceled);
        if self
            .hooks
            .fire(
                &trigger,
                HookWhen::Pre,
                Some(&prev),
                Some(prev_status),
                None,
            )
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "task_cancel".into(),
            }
            .into());
        }

        let task = self
            .backend
            .cancel_task(project_id, id, reason.clone())
            .await?;

        let _ = self
            .hooks
            .fire(
                &trigger,
                HookWhen::Post,
                Some(&task),
                Some(prev_status),
                None,
            )
            .await;

        let cancel_reason = reason.unwrap_or_default();
        crate::emit_task_event!(
            "senko.task.canceled",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            from_status = %prev_status,
            to_status = %task.status(),
            cancel_reason = cancel_reason.as_str(),
        );

        Ok(task)
    }

    async fn preview_transition(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        target: TaskStatus,
    ) -> Result<PreviewResult> {
        let task = self.backend.get_task(project_id, task_id).await?;
        let mut operations = Vec::new();

        // Resume preview: in_progress → in_progress is not a real transition
        // but a session/metadata refresh. `task resume` reuses this preview
        // path, so allow it explicitly with a Resumed-shaped operations list.
        if task.status() == TaskStatus::InProgress && target == TaskStatus::InProgress {
            operations.push(format!(
                "Resume task #{} (session/metadata refresh)",
                task_id
            ));
            return Ok(PreviewResult {
                allowed: true,
                reason: None,
                task,
                target_status: target,
                operations,
                unblocked_tasks: vec![],
            });
        }

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

            // Check metadata field requirements
            let metadata_fields = self
                .backend
                .list_metadata_fields(project_id, &ListMetadataFieldsFilter::default())
                .await?
                .items;
            if let Err(e) =
                validate_metadata_on_complete(task.metadata(), &metadata_fields, task_id)
            {
                return Ok(PreviewResult {
                    allowed: false,
                    reason: Some(e.to_string()),
                    task,
                    target_status: target,
                    operations,
                    unblocked_tasks: vec![],
                });
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

    async fn preview_next(&self, project_id: ProjectId) -> Result<PreviewResult> {
        let task = match self.backend.next_task(project_id, None, false).await? {
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

    async fn get_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task> {
        self.backend.get_task(project_id, id).await
    }

    async fn list_tasks(
        &self,
        project_id: ProjectId,
        filter: &ListTasksFilter,
    ) -> Result<ListTasksPage> {
        if filter.metadata.is_empty() {
            return self.backend.list_tasks(project_id, filter).await;
        }
        let fields = self
            .backend
            .list_metadata_fields(project_id, &ListMetadataFieldsFilter::default())
            .await?
            .items;
        let mut resolved_filter = filter.clone();
        resolved_filter.metadata = resolve_metadata_filter_types(&filter.metadata, &fields);
        self.backend.list_tasks(project_id, &resolved_filter).await
    }

    async fn list_all_tags(&self, project_id: ProjectId) -> Result<Vec<String>> {
        let tasks = self
            .backend
            .list_tasks(project_id, &ListTasksFilter::default())
            .await?
            .items;
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
        project_id: ProjectId,
    ) -> Result<std::collections::HashMap<String, i64>> {
        self.backend.task_stats(project_id).await
    }

    async fn edit_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskParams,
    ) -> Result<Task> {
        params.validate()?;
        match &params.metadata {
            Some(MetadataUpdate::Merge(v)) | Some(MetadataUpdate::Replace(v)) => {
                validate_metadata(v)?
            }
            _ => {}
        }
        let prev = self.backend.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (_aggregate_after, events) = prev.apply_update(params, now);
        let task = self.backend.update_task(project_id, id, params).await?;
        emit_task_events(project_id, id, task.contract_id(), &events);
        Ok(task)
    }

    async fn edit_task_arrays(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskArrayParams,
    ) -> Result<()> {
        params.validate()?;
        let prev = self.backend.get_task(project_id, id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (aggregate_after, events) = prev.apply_array_update(params, now);
        self.backend
            .update_task_arrays(project_id, id, params)
            .await?;
        emit_task_events(project_id, id, aggregate_after.contract_id(), &events);
        Ok(())
    }

    async fn delete_task(&self, project_id: ProjectId, id: TaskId) -> Result<()> {
        self.backend.delete_task(project_id, id).await
    }

    async fn save_task(&self, _project_id: ProjectId, _id: TaskId, task: &Task) -> Result<()> {
        self.backend.save(task).await
    }

    async fn check_dod(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        index: usize,
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, task_id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, events) = task.check_dod(index, now)?;
        self.backend.save(&task).await?;
        emit_task_events(project_id, task_id, task.contract_id(), &events);
        Ok(task)
    }

    async fn uncheck_dod(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        index: usize,
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, task_id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, events) = task.uncheck_dod(index, now)?;
        self.backend.save(&task).await?;
        emit_task_events(project_id, task_id, task.contract_id(), &events);
        Ok(task)
    }

    async fn add_dependency(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_id: TaskId,
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
                    .list_dependencies(project_id, id, &ListTaskDepsFilter::default())
                    .await
                    .map(|page| page.items.iter().map(|t| t.id()).collect())
                    .unwrap_or_default()
            }
        })
        .await
        {
            return Err(DomainError::DependencyCycle { dep_id }.into());
        }

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, events) = task.add_dependency(dep_id, Some(now))?;
        self.backend.save(&task).await?;
        emit_task_events(project_id, task_id, task.contract_id(), &events);
        self.backend.get_task(project_id, task_id).await
    }

    async fn remove_dependency(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_id: TaskId,
    ) -> Result<Task> {
        let task = self.backend.get_task(project_id, task_id).await?;
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, events) = task.remove_dependency(dep_id, Some(now))?;
        self.backend.save(&task).await?;
        emit_task_events(project_id, task_id, task.contract_id(), &events);
        self.backend.get_task(project_id, task_id).await
    }

    async fn set_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_ids: &[TaskId],
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
                        .list_dependencies(project_id, id, &ListTaskDepsFilter::default())
                        .await
                        .map(|page| page.items.iter().map(|t| t.id()).collect())
                        .unwrap_or_default()
                }
            })
            .await
            {
                return Err(DomainError::DependencyCycle { dep_id }.into());
            }
        }

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let (task, events) = task.set_dependencies(dep_ids, Some(now))?;
        self.backend.save(&task).await?;
        emit_task_events(project_id, task_id, task.contract_id(), &events);
        self.backend.get_task(project_id, task_id).await
    }

    async fn list_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        filter: &ListTaskDepsFilter,
    ) -> Result<ListPage<Task>> {
        self.backend
            .list_dependencies(project_id, task_id, filter)
            .await
    }

    async fn list_ready_tasks(&self, project_id: ProjectId) -> Result<Vec<Task>> {
        self.backend.list_ready_tasks(project_id).await
    }

    async fn ready_count(&self, project_id: ProjectId) -> Result<i64> {
        self.backend.ready_count(project_id).await
    }
}

/// Resolve metadata filter value types using metadata field definitions.
///
/// Values from the presentation layer arrive as `Value::String`. This function
/// converts them to the appropriate JSON type based on the field's declared type
/// in `metadata_fields`. Undefined fields remain as strings.
pub fn resolve_metadata_filter_types(
    raw: &HashMap<String, serde_json::Value>,
    fields: &[MetadataField],
) -> HashMap<String, serde_json::Value> {
    let field_types: HashMap<&str, MetadataFieldType> =
        fields.iter().map(|f| (f.name(), f.field_type())).collect();

    raw.iter()
        .map(|(key, value)| {
            let resolved = match (field_types.get(key.as_str()), value) {
                (Some(MetadataFieldType::Number), serde_json::Value::String(s)) => {
                    if let Ok(n) = s.parse::<i64>() {
                        serde_json::Value::Number(n.into())
                    } else if let Ok(f) = s.parse::<f64>() {
                        serde_json::Number::from_f64(f)
                            .map(serde_json::Value::Number)
                            .unwrap_or_else(|| value.clone())
                    } else {
                        value.clone()
                    }
                }
                (Some(MetadataFieldType::Boolean), serde_json::Value::String(s)) => {
                    match s.as_str() {
                        "true" => serde_json::Value::Bool(true),
                        "false" => serde_json::Value::Bool(false),
                        _ => value.clone(),
                    }
                }
                _ => value.clone(),
            };
            (key.clone(), resolved)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Tests for `senko.contract.id` attribution on `senko.task.*` events
    //! (task #383). Other behaviour of `LocalTaskOperations` is exercised by
    //! the e2e suite and the SQLite-backend integration tests.

    use std::sync::Arc;

    use anyhow::Result;
    use opentelemetry::logs::AnyValue;
    use opentelemetry_sdk::logs::SdkLogRecord;
    use tempfile::tempdir;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::application::contract_service::LocalContractOperations;
    use crate::application::port::contract_operations::ContractOperations;
    use crate::application::port::{NoOpHookExecutor, PrVerifier, TaskBackend, TaskOperations};
    use crate::application::telemetry::test_support::{
        build_capture_provider, capture_layer, lookup_attr,
    };
    use crate::domain::contract::CreateContractParams;
    use crate::domain::project::ProjectId;
    use crate::domain::task::{
        CompletionPolicy, CreateTaskParams, MergeVia, UpdateTaskArrayParams, UpdateTaskParams,
    };
    use crate::infra::sqlite::SqliteBackend;

    use super::{HookExecutor, LocalTaskOperations};

    /// PrVerifier that always succeeds. The test scenarios use
    /// `merge_via = Direct`, so `verify_pr_status` is never reached, but the
    /// trait is still required for `LocalTaskOperations::new`.
    struct AlwaysOkPrVerifier;
    impl PrVerifier for AlwaysOkPrVerifier {
        fn verify_pr_status(&self, _pr_url: &str) -> Result<()> {
            Ok(())
        }
    }

    fn noop_hooks() -> Arc<dyn HookExecutor> {
        Arc::new(NoOpHookExecutor)
    }

    async fn new_backend() -> (tempfile::TempDir, Arc<dyn TaskBackend>, ProjectId) {
        let dir = tempdir().unwrap();
        let backend = SqliteBackend::new(
            dir.path(),
            Some(&dir.path().join("data.db")),
            None,
            &crate::infra::xdg::XdgDirs::default(),
        )
        .unwrap();
        let backend: Arc<dyn TaskBackend> = Arc::new(backend);
        (dir, backend, ProjectId(1))
    }

    fn task_ops(backend: Arc<dyn TaskBackend>) -> LocalTaskOperations {
        LocalTaskOperations::new(
            backend,
            noop_hooks(),
            Arc::new(AlwaysOkPrVerifier),
            CompletionPolicy::new(MergeVia::Direct),
        )
    }

    fn contract_ops(backend: Arc<dyn TaskBackend>) -> LocalContractOperations {
        LocalContractOperations::new(backend, noop_hooks())
    }

    fn create_params() -> CreateTaskParams {
        CreateTaskParams {
            title: "t".into(),
            background: None,
            description: None,
            priority: None,
            definition_of_done: vec!["dod-1".into()],
            in_scope: vec![],
            out_of_scope: vec![],
            branch: None,
            pr_url: None,
            metadata: None,
            tags: vec![],
            dependencies: vec![],
            assignee_user_id: None,
            contract_id: None,
        }
    }

    /// Drive `body` under a tracing subscriber that bridges
    /// `emit_business_event!` calls into an in-memory OTel exporter.
    fn with_records<F, Fut>(body: F) -> Vec<SdkLogRecord>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let _g = tracing::subscriber::set_default(subscriber);
            body().await;
        });

        provider.force_flush().ok();
        exporter
            .get_emitted_logs()
            .unwrap()
            .into_iter()
            .map(|d| d.record)
            .collect()
    }

    fn one<'a>(records: &'a [SdkLogRecord], name: &str) -> &'a SdkLogRecord {
        records
            .iter()
            .find(|r| r.event_name() == Some(name))
            .unwrap_or_else(|| panic!("expected at least one {name} record"))
    }

    // --- None case: no senko.task.* event carries senko.contract.id --------

    #[test]
    fn no_event_carries_contract_id_when_task_has_no_contract() {
        let records = with_records(|| async {
            let (_dir, backend, project_id) = new_backend().await;
            let ops = task_ops(backend);

            let t = ops.create_task(project_id, &create_params()).await.unwrap();
            ops.publish_task(project_id, t.id()).await.unwrap();
            ops.start_task(project_id, t.id(), Some("s".into()), None, None)
                .await
                .unwrap();
            ops.resume_task(project_id, t.id(), Some("s2".into()), None)
                .await
                .unwrap();
            // edit_task → senko.task.updated
            let upd = UpdateTaskParams {
                description: Some(Some("d".into())),
                ..empty_update_params()
            };
            ops.edit_task(project_id, t.id(), &upd).await.unwrap();
            // edit_task_arrays → senko.task.updated
            let arr = UpdateTaskArrayParams {
                add_tags: vec!["x".into()],
                ..empty_array_params()
            };
            ops.edit_task_arrays(project_id, t.id(), &arr)
                .await
                .unwrap();
            // dod
            ops.check_dod(project_id, t.id(), 1).await.unwrap();
            ops.uncheck_dod(project_id, t.id(), 1).await.unwrap();
            // deps — need a second task as the dependency
            let dep = ops.create_task(project_id, &create_params()).await.unwrap();
            ops.add_dependency(project_id, t.id(), dep.id())
                .await
                .unwrap();
            ops.remove_dependency(project_id, t.id(), dep.id())
                .await
                .unwrap();
            ops.set_dependencies(project_id, t.id(), &[dep.id()])
                .await
                .unwrap();
            // complete → first need to check the only DoD
            ops.check_dod(project_id, t.id(), 1).await.unwrap();
            ops.complete_task(project_id, t.id(), false).await.unwrap();
        });

        let saw_task_event = records
            .iter()
            .any(|r| r.event_name().is_some_and(|n| n.starts_with("senko.task.")));
        assert!(saw_task_event, "expected at least one senko.task.* record");

        for r in &records {
            if let Some(name) = r.event_name()
                && name.starts_with("senko.task.")
            {
                assert!(
                    lookup_attr(r, "senko.contract.id").is_none(),
                    "{name} unexpectedly carried senko.contract.id when contract_id is None",
                );
            }
        }
    }

    // --- Some case: every senko.task.* event carries senko.contract.id -----

    #[test]
    fn every_event_carries_contract_id_when_task_links_a_contract() {
        let records = with_records(|| async {
            let (_dir, backend, project_id) = new_backend().await;
            let cops = contract_ops(backend.clone());
            let contract = cops
                .create_contract(
                    project_id,
                    &CreateContractParams {
                        title: "c".into(),
                        description: None,
                        definition_of_done: vec![],
                        tags: vec![],
                        metadata: None,
                    },
                )
                .await
                .unwrap();

            let ops = task_ops(backend);
            let mut params = create_params();
            params.contract_id = Some(contract.id());
            let t = ops.create_task(project_id, &params).await.unwrap();
            ops.publish_task(project_id, t.id()).await.unwrap();
            ops.start_task(project_id, t.id(), Some("s".into()), None, None)
                .await
                .unwrap();
            ops.resume_task(project_id, t.id(), Some("s2".into()), None)
                .await
                .unwrap();
            let upd = UpdateTaskParams {
                description: Some(Some("d".into())),
                ..empty_update_params()
            };
            ops.edit_task(project_id, t.id(), &upd).await.unwrap();
            let arr = UpdateTaskArrayParams {
                add_tags: vec!["x".into()],
                ..empty_array_params()
            };
            ops.edit_task_arrays(project_id, t.id(), &arr)
                .await
                .unwrap();
            ops.check_dod(project_id, t.id(), 1).await.unwrap();
            ops.uncheck_dod(project_id, t.id(), 1).await.unwrap();
            // deps need a sibling task linked to the same contract is fine
            let dep_params = CreateTaskParams {
                title: "dep".into(),
                ..create_params()
            };
            let dep = ops.create_task(project_id, &dep_params).await.unwrap();
            ops.add_dependency(project_id, t.id(), dep.id())
                .await
                .unwrap();
            ops.remove_dependency(project_id, t.id(), dep.id())
                .await
                .unwrap();
            ops.set_dependencies(project_id, t.id(), &[dep.id()])
                .await
                .unwrap();
            ops.check_dod(project_id, t.id(), 1).await.unwrap();
            ops.complete_task(project_id, t.id(), false).await.unwrap();

            // separately exercise cancel via a fresh task (cancel rejects completed)
            let mut params2 = create_params();
            params2.contract_id = Some(contract.id());
            let t2 = ops.create_task(project_id, &params2).await.unwrap();
            ops.publish_task(project_id, t2.id()).await.unwrap();
            ops.cancel_task(project_id, t2.id(), Some("nope".into()))
                .await
                .unwrap();
        });

        let cid = records
            .iter()
            .find(|r| r.event_name() == Some("senko.task.created"))
            .and_then(|r| lookup_attr(r, "senko.contract.id"))
            .expect("first senko.task.created should carry the contract id");
        let AnyValue::Int(cid_val) = cid else {
            panic!("senko.contract.id is not an Int");
        };

        let expected = [
            "senko.task.created",
            "senko.task.published",
            "senko.task.started",
            "senko.task.resumed",
            "senko.task.completed",
            "senko.task.canceled",
            "senko.task.updated",
            "senko.task.dod_checked",
            "senko.task.dod_unchecked",
            "senko.task.dependency_added",
            "senko.task.dependency_removed",
            "senko.task.dependencies_set",
        ];
        for name in expected {
            // Filter to only contract-bearing tasks (the dep task has no contract,
            // so its `senko.task.created` would have None).
            let with_cid = records.iter().find(|r| {
                r.event_name() == Some(name)
                    && lookup_attr(r, "senko.contract.id") == Some(AnyValue::Int(cid_val))
            });
            assert!(
                with_cid.is_some(),
                "no `{name}` record carries senko.contract.id={cid_val}",
            );
        }
    }

    // --- Updated post-state semantics: contract_id update emits new value --

    #[test]
    fn updated_event_uses_post_update_contract_id() {
        let records = with_records(|| async {
            let (_dir, backend, project_id) = new_backend().await;
            let cops = contract_ops(backend.clone());
            let contract = cops
                .create_contract(
                    project_id,
                    &CreateContractParams {
                        title: "c".into(),
                        description: None,
                        definition_of_done: vec![],
                        tags: vec![],
                        metadata: None,
                    },
                )
                .await
                .unwrap();
            let ops = task_ops(backend);
            // Task starts contract-less.
            let t = ops.create_task(project_id, &create_params()).await.unwrap();
            // Update sets contract_id to Some(contract.id()).
            let upd = UpdateTaskParams {
                contract_id: Some(Some(contract.id())),
                ..empty_update_params()
            };
            ops.edit_task(project_id, t.id(), &upd).await.unwrap();
        });

        let updated = one(&records, "senko.task.updated");
        let AnyValue::Int(cid) = lookup_attr(updated, "senko.contract.id")
            .expect("senko.task.updated must carry senko.contract.id after the update sets it")
        else {
            panic!("senko.contract.id is not Int");
        };
        assert!(
            cid > 0,
            "senko.contract.id should be the post-update contract id, got {cid}",
        );
        assert_eq!(
            lookup_attr(updated, "changed_fields"),
            Some(AnyValue::String("[\"contract_id\"]".into())),
        );
    }

    // --- Helpers ------------------------------------------------------------

    fn empty_update_params() -> UpdateTaskParams {
        UpdateTaskParams {
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
            branch: None,
            pr_url: None,
            contract_id: None,
            metadata: None,
        }
    }

    fn empty_array_params() -> UpdateTaskArrayParams {
        UpdateTaskArrayParams {
            set_tags: None,
            add_tags: Vec::new(),
            remove_tags: Vec::new(),
            set_definition_of_done: None,
            add_definition_of_done: Vec::new(),
            remove_definition_of_done: Vec::new(),
            set_in_scope: None,
            add_in_scope: Vec::new(),
            remove_in_scope: Vec::new(),
            set_out_of_scope: None,
            add_out_of_scope: Vec::new(),
            remove_out_of_scope: Vec::new(),
        }
    }
}
