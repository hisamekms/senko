use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::json;

use crate::application::HookTrigger;
use crate::application::hook_trigger::SelectResult;
use crate::application::port::task_operations::{CompleteResult, PreviewResult};
use crate::application::port::{HookExecutor, TaskOperations};
use crate::domain::error::DomainError;
use crate::domain::pagination::{Cursor, ListPage};
use crate::domain::project::ProjectId;
use crate::domain::task::{
    CreateTaskParams, ListTaskDepsFilter, ListTasksFilter, ListTasksPage, MetadataUpdate, Priority,
    Task, TaskEvent, TaskId, TaskStatus, UnblockedTask, UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::UserId;
use crate::infra::config::HookWhen;

use super::client::HttpClient;
use super::{
    array_params_to_json, check_success, extract_error, read_json_or_error, update_params_to_json,
};

/// HTTP client implementing `TaskOperations` directly.
///
/// Each method maps to a single API endpoint call. Domain logic is executed
/// server-side; this client only handles HTTP transport and optional
/// client-side hook firing.
pub struct RemoteTaskOperations {
    http: HttpClient,
    hooks: Arc<dyn HookExecutor>,
}

/// Deserialization wrapper for the complete-task API response.
#[derive(Deserialize)]
struct CompleteApiResponse {
    task: Task,
    unblocked_tasks: Vec<UnblockedApiInfo>,
}

#[derive(Deserialize)]
struct UnblockedApiInfo {
    id: TaskId,
    title: String,
    #[allow(dead_code)]
    status: String,
    priority: String,
}

/// Deserialization wrapper for the preview-transition API response.
#[derive(Deserialize)]
struct PreviewApiResponse {
    allowed: bool,
    reason: Option<String>,
    target_status: String,
    operations: Vec<String>,
    unblocked_tasks: Vec<UnblockedPreviewInfo>,
}

#[derive(Deserialize)]
struct UnblockedPreviewInfo {
    id: TaskId,
    title: String,
    status: String,
    priority: String,
}

impl RemoteTaskOperations {
    pub fn new(
        base_url: &str,
        api_key: Option<String>,
        attributes: std::collections::BTreeMap<String, String>,
        hooks: Arc<dyn HookExecutor>,
    ) -> Self {
        Self {
            http: HttpClient::new(base_url, api_key, attributes),
            hooks,
        }
    }

    fn project_url(&self, project_id: ProjectId, path: &str) -> String {
        self.http.project_url(project_id, path)
    }

    /// Attach Bearer auth + W3C trace-propagation headers (traceparent always,
    /// baggage when attributes are present) to the request builder.
    fn prepare(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        self.http.propagate(self.http.auth(builder))
    }

    fn client(&self) -> &reqwest::Client {
        self.http.reqwest()
    }
}

fn parse_unblocked(items: Vec<UnblockedApiInfo>) -> Vec<UnblockedTask> {
    items
        .into_iter()
        .map(|u| {
            let priority = u.priority.parse::<Priority>().unwrap_or(Priority::P2);
            UnblockedTask::new(u.id, u.title, priority, None)
        })
        .collect()
}

/// Derive `senko.task.updated::changed_fields` from `UpdateTaskParams`.
///
/// Mirrors `Task::apply_update`'s push order in `src/domain/task.rs:519-632`,
/// so the Remote-side `senko.task.updated` LogRecord uses the same field-name
/// vocabulary and ordering as the Local-side emit (and therefore the same
/// ordering produced by the upstream server's own emit). A field is reported
/// as changed iff the caller supplied any value for it — prev fetch is
/// intentionally avoided per Contract #8 B3 #354.
fn updated_changed_fields(params: &UpdateTaskParams) -> Vec<String> {
    let mut changed: Vec<String> = Vec::new();
    if params.title.is_some() {
        changed.push("title".into());
    }
    if params.background.is_some() {
        changed.push("background".into());
    }
    if params.description.is_some() {
        changed.push("description".into());
    }
    if params.plan.is_some() {
        changed.push("plan".into());
    }
    if params.priority.is_some() {
        changed.push("priority".into());
    }
    if params.assignee_session_id.is_some() {
        changed.push("assignee_session_id".into());
    }
    if params.assignee_user_id.is_some() {
        changed.push("assignee_user_id".into());
    }
    if params.started_at.is_some() {
        changed.push("started_at".into());
    }
    if params.completed_at.is_some() {
        changed.push("completed_at".into());
    }
    if params.canceled_at.is_some() {
        changed.push("canceled_at".into());
    }
    if params.cancel_reason.is_some() {
        changed.push("cancel_reason".into());
    }
    if params.branch.is_some() {
        changed.push("branch".into());
    }
    if params.pr_url.is_some() {
        changed.push("pr_url".into());
    }
    if params.contract_id.is_some() {
        changed.push("contract_id".into());
    }
    if params.metadata.is_some() {
        changed.push("metadata".into());
    }
    changed
}

/// Derive `senko.task.updated::changed_fields` from `UpdateTaskArrayParams`.
///
/// Mirrors `Task::apply_array_update` in `src/domain/task.rs:634-720`: a
/// collection field is reported as changed iff the caller supplied any
/// non-empty `set_/add_/remove_` for it.
fn array_updated_changed_fields(params: &UpdateTaskArrayParams) -> Vec<String> {
    let mut changed: Vec<String> = Vec::new();
    if params.set_tags.is_some() || !params.add_tags.is_empty() || !params.remove_tags.is_empty() {
        changed.push("tags".into());
    }
    if params.set_definition_of_done.is_some()
        || !params.add_definition_of_done.is_empty()
        || !params.remove_definition_of_done.is_empty()
    {
        changed.push("definition_of_done".into());
    }
    if params.set_in_scope.is_some()
        || !params.add_in_scope.is_empty()
        || !params.remove_in_scope.is_empty()
    {
        changed.push("in_scope".into());
    }
    if params.set_out_of_scope.is_some()
        || !params.add_out_of_scope.is_empty()
        || !params.remove_out_of_scope.is_empty()
    {
        changed.push("out_of_scope".into());
    }
    changed
}

#[async_trait]
impl TaskOperations for RemoteTaskOperations {
    // --- State transitions ---

    async fn create_task(&self, project_id: ProjectId, params: &CreateTaskParams) -> Result<Task> {
        let resp = self
            .prepare(
                self.client()
                    .post(self.project_url(project_id, "/tasks"))
                    .json(params),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        let _ = self
            .hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Created),
                HookWhen::Post,
                Some(&task),
                None,
                None,
            )
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
        let prev_status = self.get_task(project_id, id).await?.status();

        let resp = self
            .prepare(
                self.client()
                    .post(self.project_url(project_id, &format!("/tasks/{id}/publish"))),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        let _ = self
            .hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Published),
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
        _user_id: Option<UserId>,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task> {
        let prev_status = self.get_task(project_id, id).await?.status();

        // user_id is resolved server-side from the authenticated request — the
        // client no longer sends it in the body. See #330.
        let mut body = json!({ "session_id": session_id });
        if let Some(ref meta_update) = metadata {
            match meta_update {
                MetadataUpdate::Clear => {
                    body["clear_metadata"] = json!(true);
                }
                MetadataUpdate::Merge(v) => {
                    body["metadata"] = json!(v);
                }
                MetadataUpdate::Replace(v) => {
                    body["replace_metadata"] = json!(v);
                }
            }
        }

        let resp = self
            .prepare(
                self.client()
                    .post(self.project_url(project_id, &format!("/tasks/{id}/start")))
                    .json(&body),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

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
        let prev_status = self.get_task(project_id, id).await?.status();

        let mut body = json!({ "session_id": session_id });
        if let Some(ref meta_update) = metadata {
            match meta_update {
                MetadataUpdate::Clear => {
                    body["clear_metadata"] = json!(true);
                }
                MetadataUpdate::Merge(v) => {
                    body["metadata"] = json!(v);
                }
                MetadataUpdate::Replace(v) => {
                    body["replace_metadata"] = json!(v);
                }
            }
        }

        let resp = self
            .prepare(
                self.client()
                    .post(self.project_url(project_id, &format!("/tasks/{id}/resume")))
                    .json(&body),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        let _ = self
            .hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Resumed),
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
        _user_id: Option<UserId>,
        include_unassigned: bool,
        metadata: Option<MetadataUpdate>,
    ) -> Result<Task> {
        // user_id is resolved server-side from the authenticated request — the
        // client no longer sends it in the body. See #330.
        let mut body =
            json!({ "session_id": session_id, "include_unassigned": include_unassigned });
        if let Some(ref meta_update) = metadata {
            match meta_update {
                MetadataUpdate::Clear => {
                    body["clear_metadata"] = json!(true);
                }
                MetadataUpdate::Merge(v) => {
                    body["metadata"] = json!(v);
                }
                MetadataUpdate::Replace(v) => {
                    body["replace_metadata"] = json!(v);
                }
            }
        }

        let resp = self
            .prepare(
                self.client()
                    .post(self.project_url(project_id, "/tasks/next"))
                    .json(&body),
            )
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
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

        if !resp.status().is_success() {
            bail!("{}", extract_error(resp).await);
        }

        let task: Task = resp.json().await?;

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

        let _ = self
            .hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Started),
                HookWhen::Post,
                Some(&task),
                Some(TaskStatus::Todo),
                None,
            )
            .await;

        crate::emit_task_event!(
            "senko.task.started",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            from_status = %TaskStatus::Todo,
            to_status = %task.status(),
        );

        Ok(task)
    }

    async fn complete_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        skip_pr_check: bool,
    ) -> Result<CompleteResult> {
        let prev_status = self.get_task(project_id, id).await?.status();

        let body = if skip_pr_check {
            json!({ "skip_pr_check": true })
        } else {
            json!({})
        };
        let resp = self
            .prepare(
                self.client()
                    .post(self.project_url(project_id, &format!("/tasks/{id}/complete")))
                    .json(&body),
            )
            .send()
            .await?;
        let api_resp: CompleteApiResponse = read_json_or_error(resp).await?;
        let unblocked = parse_unblocked(api_resp.unblocked_tasks);

        let _ = self
            .hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Completed),
                HookWhen::Post,
                Some(&api_resp.task),
                Some(prev_status),
                Some(unblocked.clone()),
            )
            .await;

        crate::emit_task_event!(
            "senko.task.completed",
            contract_id = api_resp.task.contract_id(),
            senko.task.id = api_resp.task.id().0,
            senko.project.id = project_id.0,
            from_status = %prev_status,
            to_status = %api_resp.task.status(),
        );

        Ok(CompleteResult {
            task: api_resp.task,
            unblocked,
        })
    }

    async fn cancel_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        reason: Option<String>,
    ) -> Result<Task> {
        let prev_status = self.get_task(project_id, id).await?.status();

        let body = match reason {
            Some(ref r) => json!({ "reason": r }),
            None => json!({}),
        };
        let resp = self
            .prepare(
                self.client()
                    .post(self.project_url(project_id, &format!("/tasks/{id}/cancel")))
                    .json(&body),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        let _ = self
            .hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Canceled),
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

    // --- Preview ---

    async fn preview_transition(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        target: TaskStatus,
    ) -> Result<PreviewResult> {
        let task = self.get_task(project_id, task_id).await?;

        let resp = self
            .prepare(self.client().get(self.project_url(
                project_id,
                &format!("/tasks/{task_id}/preview-transition?target={target}"),
            )))
            .send()
            .await?;
        let api: PreviewApiResponse = read_json_or_error(resp).await?;

        let target_status = api.target_status.parse::<TaskStatus>()?;
        let unblocked_tasks = api
            .unblocked_tasks
            .into_iter()
            .filter_map(|u| {
                let priority = u.priority.parse::<Priority>().ok()?;
                let status = u.status.parse::<TaskStatus>().ok()?;
                Some(Task::new(
                    u.id,
                    project_id,
                    u.title,
                    None,
                    None,
                    None,
                    priority,
                    status,
                    None,
                    None,
                    String::new(),
                    String::new(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ))
            })
            .collect();

        Ok(PreviewResult {
            allowed: api.allowed,
            reason: api.reason,
            task,
            target_status,
            operations: api.operations,
            unblocked_tasks,
        })
    }

    async fn preview_next(&self, project_id: ProjectId) -> Result<PreviewResult> {
        let resp = self
            .prepare(
                self.client()
                    .get(self.project_url(project_id, "/tasks/preview-next")),
            )
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(DomainError::NoEligibleTask.into());
        }

        let api: PreviewApiResponse = read_json_or_error(resp).await?;

        // Fetch the next eligible task for the PreviewResult.task field
        let ready_tasks = self.list_ready_tasks(project_id).await?;
        let task = ready_tasks
            .into_iter()
            .next()
            .ok_or(DomainError::NoEligibleTask)?;

        let target_status = api.target_status.parse::<TaskStatus>()?;

        Ok(PreviewResult {
            allowed: api.allowed,
            reason: api.reason,
            task,
            target_status,
            operations: api.operations,
            unblocked_tasks: vec![],
        })
    }

    // --- Queries ---

    async fn get_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task> {
        let resp = self
            .prepare(
                self.client()
                    .get(self.project_url(project_id, &format!("/tasks/{id}"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn list_tasks(
        &self,
        project_id: ProjectId,
        filter: &ListTasksFilter,
    ) -> Result<ListTasksPage> {
        let mut url = self.project_url(project_id, "/tasks");
        let mut params: Vec<String> = Vec::new();

        for status in &filter.statuses {
            params.push(format!("status={}", status.to_string().to_lowercase()));
        }
        for tag in &filter.tags {
            params.push(format!(
                "tag={}",
                utf8_percent_encode(tag, NON_ALPHANUMERIC)
            ));
        }
        if let Some(dep) = filter.depends_on {
            params.push(format!("depends_on={dep}"));
        }
        if filter.ready {
            params.push("ready=true".into());
        }
        if filter.assignee_self {
            // Unresolved "self" intent — let the upstream resolve it from auth.
            params.push("assignee_user_id=self".into());
        } else if let Some(uid) = filter.assignee_user_id {
            params.push(format!("assignee_user_id={uid}"));
        }
        if filter.include_unassigned {
            params.push("include_unassigned=true".into());
        }
        for (key, value) in &filter.metadata {
            let v = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            params.push(format!(
                "metadata={}:{}",
                utf8_percent_encode(key, NON_ALPHANUMERIC),
                utf8_percent_encode(&v, NON_ALPHANUMERIC),
            ));
        }
        if let Some(c) = filter.contract_id {
            params.push(format!("contract={c}"));
        }
        if let Some(n) = filter.id_min {
            params.push(format!("id_min={n}"));
        }
        if let Some(n) = filter.id_max {
            params.push(format!("id_max={n}"));
        }
        if let Some(n) = filter.limit {
            params.push(format!("limit={n}"));
        }
        if let Some(after) = filter.after.as_ref() {
            params.push(format!(
                "after={}",
                utf8_percent_encode(&Cursor::encode_payload(after), NON_ALPHANUMERIC)
            ));
        }
        // Forward sort parameters when non-default. Default values would be
        // accepted by the upstream too, but emitting them only on demand keeps
        // the wire log simple to read.
        match filter.order_by {
            crate::domain::task::TaskOrderBy::Id => {}
            crate::domain::task::TaskOrderBy::UpdatedAt => {
                params.push("order_by=updated_at".into())
            }
            crate::domain::task::TaskOrderBy::Priority => params.push("order_by=priority".into()),
        }
        match filter.order {
            crate::domain::task::ListOrder::Asc => {}
            crate::domain::task::ListOrder::Desc => params.push("order=desc".into()),
        }

        if !params.is_empty() {
            url = format!("{url}?{}", params.join("&"));
        }

        let resp = self.prepare(self.client().get(&url)).send().await?;
        read_json_or_error(resp).await
    }

    async fn list_all_tags(&self, project_id: ProjectId) -> Result<Vec<String>> {
        let tasks = self
            .list_tasks(project_id, &ListTasksFilter::default())
            .await?
            .items;
        let mut tags: Vec<String> = tasks
            .iter()
            .flat_map(|t| t.tags().iter().cloned())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        tags.sort();
        Ok(tags)
    }

    async fn task_stats(&self, project_id: ProjectId) -> Result<HashMap<String, i64>> {
        let resp = self
            .prepare(self.client().get(self.project_url(project_id, "/stats")))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    // --- Edit ---

    async fn edit_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskParams,
    ) -> Result<Task> {
        let body = update_params_to_json(params);
        let resp = self
            .prepare(
                self.client()
                    .put(self.project_url(project_id, &format!("/tasks/{id}")))
                    .json(&body),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        // Derive `changed_fields` from the request params (mirrors
        // `Task::apply_update`'s push order in src/domain/task.rs:519-632).
        // Prev fetch is intentionally avoided per Contract #8 B3 #354 — the
        // server-side authoritative diff lives in the upstream's own emit.
        let changed_fields = updated_changed_fields(params);
        if !changed_fields.is_empty() {
            let changed_fields_json = serde_json::to_string(&changed_fields).unwrap_or_default();
            crate::emit_task_event!(
                "senko.task.updated",
                contract_id = task.contract_id(),
                senko.task.id = task.id().0,
                senko.project.id = project_id.0,
                changed_fields = changed_fields_json.as_str(),
            );
        }

        Ok(task)
    }

    async fn edit_task_arrays(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskArrayParams,
    ) -> Result<()> {
        let body = array_params_to_json(params);
        let resp = self
            .prepare(
                self.client()
                    .put(self.project_url(project_id, &format!("/tasks/{id}")))
                    .json(&body),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        let changed_fields = array_updated_changed_fields(params);
        if !changed_fields.is_empty() {
            let changed_fields_json = serde_json::to_string(&changed_fields).unwrap_or_default();
            crate::emit_task_event!(
                "senko.task.updated",
                contract_id = task.contract_id(),
                senko.task.id = task.id().0,
                senko.project.id = project_id.0,
                changed_fields = changed_fields_json.as_str(),
            );
        }

        Ok(())
    }

    async fn delete_task(&self, project_id: ProjectId, id: TaskId) -> Result<()> {
        let resp = self
            .prepare(
                self.client()
                    .delete(self.project_url(project_id, &format!("/tasks/{id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    async fn save_task(&self, project_id: ProjectId, id: TaskId, task: &Task) -> Result<()> {
        let resp = self
            .prepare(
                self.client()
                    .put(self.project_url(project_id, &format!("/tasks/{id}/_save")))
                    .json(task),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    // --- Definition of Done ---

    async fn check_dod(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        index: usize,
    ) -> Result<Task> {
        let resp =
            self.prepare(self.client().post(
                self.project_url(project_id, &format!("/tasks/{task_id}/dod/{index}/check")),
            ))
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        crate::emit_task_event!(
            "senko.task.dod_checked",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            dod_index = index as i64,
        );

        Ok(task)
    }

    async fn uncheck_dod(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        index: usize,
    ) -> Result<Task> {
        let resp = self
            .prepare(self.client().post(
                self.project_url(project_id, &format!("/tasks/{task_id}/dod/{index}/uncheck")),
            ))
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        crate::emit_task_event!(
            "senko.task.dod_unchecked",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            dod_index = index as i64,
        );

        Ok(task)
    }

    // --- Dependencies ---

    async fn add_dependency(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_id: TaskId,
    ) -> Result<Task> {
        let resp = self
            .prepare(
                self.client()
                    .post(self.project_url(project_id, &format!("/tasks/{task_id}/deps")))
                    .json(&json!({ "dep_id": dep_id })),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        crate::emit_task_event!(
            "senko.task.dependency_added",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            dep_id = dep_id.0,
        );

        Ok(task)
    }

    async fn remove_dependency(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_id: TaskId,
    ) -> Result<Task> {
        let resp = self
            .prepare(
                self.client().delete(
                    self.project_url(project_id, &format!("/tasks/{task_id}/deps/{dep_id}")),
                ),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        crate::emit_task_event!(
            "senko.task.dependency_removed",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            dep_id = dep_id.0,
        );

        Ok(task)
    }

    async fn set_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_ids: &[TaskId],
    ) -> Result<Task> {
        let resp = self
            .prepare(
                self.client()
                    .put(self.project_url(project_id, &format!("/tasks/{task_id}/deps")))
                    .json(&json!({ "dep_ids": dep_ids })),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

        let deps_json = serde_json::to_string(&dep_ids.iter().map(|d| d.0).collect::<Vec<_>>())
            .unwrap_or_default();
        crate::emit_task_event!(
            "senko.task.dependencies_set",
            contract_id = task.contract_id(),
            senko.task.id = task.id().0,
            senko.project.id = project_id.0,
            deps = deps_json.as_str(),
        );

        Ok(task)
    }

    async fn list_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        filter: &ListTaskDepsFilter,
    ) -> Result<ListPage<Task>> {
        let mut url = self.project_url(project_id, &format!("/tasks/{task_id}/deps"));
        let mut params: Vec<String> = Vec::new();
        if let Some(l) = filter.limit {
            params.push(format!("limit={l}"));
        }
        if let Some(after) = filter.after {
            params.push(format!(
                "after={}",
                utf8_percent_encode(&Cursor::encode(after), NON_ALPHANUMERIC)
            ));
        }
        if !params.is_empty() {
            url = format!("{url}?{}", params.join("&"));
        }
        let resp = self.prepare(self.client().get(&url)).send().await?;
        read_json_or_error(resp).await
    }

    async fn list_ready_tasks(&self, project_id: ProjectId) -> Result<Vec<Task>> {
        Ok(self
            .list_tasks(
                project_id,
                &ListTasksFilter {
                    ready: true,
                    ..Default::default()
                },
            )
            .await?
            .items)
    }

    async fn ready_count(&self, project_id: ProjectId) -> Result<i64> {
        let tasks = self.list_ready_tasks(project_id).await?;
        Ok(tasks.len() as i64)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use axum::Json;
    use axum::Router;
    use axum::routing::any;
    use opentelemetry::logs::AnyValue;
    use opentelemetry_sdk::logs::SdkLogRecord;
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::application::port::TaskOperations;
    use crate::application::port::hook_executor::NoOpHookExecutor;
    use crate::application::telemetry::test_support::{
        build_capture_provider, capture_layer, lookup_attr,
    };
    use crate::domain::project::ProjectId;
    use crate::domain::task::{Priority, TaskId, UpdateTaskArrayParams, UpdateTaskParams};

    use super::RemoteTaskOperations;

    /// Build a `Task` JSON with the given `contract_id`. Only `id` /
    /// `project_id` / `contract_id` matter for assertions — they round-trip
    /// through `read_json_or_error` into the captured LogRecord's attrs.
    fn mock_task_with_contract(contract_id: Option<i64>) -> Value {
        json!({
            "id": 1,
            "project_id": 7,
            "title": "mock",
            "background": null,
            "description": null,
            "plan": null,
            "priority": "P2",
            "status": "todo",
            "assignee_session_id": null,
            "assignee_user_id": null,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "started_at": null,
            "completed_at": null,
            "canceled_at": null,
            "cancel_reason": null,
            "branch": null,
            "pr_url": null,
            "contract_id": contract_id,
            "metadata": null,
            "definition_of_done": [],
            "in_scope": [],
            "out_of_scope": [],
            "tags": [],
            "dependencies": []
        })
    }

    /// Spawn a minimal upstream that returns the given `Task` JSON for every
    /// path and method. The `/complete` path returns the
    /// `{ task, unblocked_tasks }` envelope expected by `complete_task`'s
    /// `CompleteApiResponse`. Listening on a kernel-assigned port so tests
    /// can run in parallel.
    async fn spawn_mock_upstream_with_contract(contract_id: Option<i64>) -> String {
        let app: Router = Router::new().route(
            "/{*rest}",
            any(
                move |req: axum::http::Request<axum::body::Body>| async move {
                    let path = req.uri().path().to_string();
                    let task = mock_task_with_contract(contract_id);
                    if path.ends_with("/complete") {
                        Json(json!({ "task": task, "unblocked_tasks": [] }))
                    } else {
                        Json(task)
                    }
                },
            ),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    async fn spawn_mock_upstream() -> String {
        spawn_mock_upstream_with_contract(None).await
    }

    fn make_remote_ops(base_url: &str) -> RemoteTaskOperations {
        RemoteTaskOperations::new(base_url, None, BTreeMap::new(), Arc::new(NoOpHookExecutor))
    }

    /// Drive `body` under a tracing subscriber that bridges
    /// `emit_business_event!` calls into an in-memory OTel exporter. Returns
    /// every captured `LogRecord`. Uses a per-test current-thread runtime so
    /// the `set_default` thread-local guard does not leak across parallel
    /// tests.
    fn with_business_records<F, Fut>(body: F) -> Vec<SdkLogRecord>
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
        let matching: Vec<&SdkLogRecord> = records
            .iter()
            .filter(|r| r.event_name() == Some(name))
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "expected exactly one {name} record, got {}",
            matching.len()
        );
        matching[0]
    }

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

    // --- senko.task.updated -------------------------------------------------

    #[test]
    fn edit_task_emits_updated_with_changed_fields() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            let params = UpdateTaskParams {
                description: Some(Some("x".into())),
                priority: Some(Priority::P0),
                ..empty_update_params()
            };
            ops.edit_task(ProjectId(7), TaskId(1), &params)
                .await
                .unwrap();
        });

        let r = one(&records, "senko.task.updated");
        assert_eq!(lookup_attr(r, "senko.task.id"), Some(AnyValue::Int(1)));
        assert_eq!(lookup_attr(r, "senko.project.id"), Some(AnyValue::Int(7)));
        // Mirrors the push order of `Task::apply_update`.
        assert_eq!(
            lookup_attr(r, "changed_fields"),
            Some(AnyValue::String("[\"description\",\"priority\"]".into())),
        );
    }

    #[test]
    fn edit_task_emits_nothing_when_params_are_all_none() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            ops.edit_task(ProjectId(7), TaskId(1), &empty_update_params())
                .await
                .unwrap();
        });

        assert!(
            !records
                .iter()
                .any(|r| r.event_name() == Some("senko.task.updated")),
            "no senko.task.updated should be emitted when no fields changed"
        );
    }

    #[test]
    fn edit_task_arrays_emits_updated_with_array_changed_fields() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            let params = UpdateTaskArrayParams {
                set_tags: Some(vec!["a".into()]),
                add_definition_of_done: vec!["b".into()],
                ..empty_array_params()
            };
            ops.edit_task_arrays(ProjectId(7), TaskId(1), &params)
                .await
                .unwrap();
        });

        let r = one(&records, "senko.task.updated");
        assert_eq!(
            lookup_attr(r, "changed_fields"),
            Some(AnyValue::String("[\"tags\",\"definition_of_done\"]".into())),
        );
    }

    #[test]
    fn edit_task_arrays_emits_nothing_when_params_are_empty() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            ops.edit_task_arrays(ProjectId(7), TaskId(1), &empty_array_params())
                .await
                .unwrap();
        });

        assert!(
            !records
                .iter()
                .any(|r| r.event_name() == Some("senko.task.updated")),
            "no senko.task.updated should be emitted when no arrays changed"
        );
    }

    // --- senko.task.dod_checked / dod_unchecked -----------------------------

    #[test]
    fn check_dod_emits_dod_checked_with_index() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            ops.check_dod(ProjectId(7), TaskId(1), 2).await.unwrap();
        });

        let r = one(&records, "senko.task.dod_checked");
        assert_eq!(lookup_attr(r, "senko.task.id"), Some(AnyValue::Int(1)));
        assert_eq!(lookup_attr(r, "senko.project.id"), Some(AnyValue::Int(7)));
        assert_eq!(lookup_attr(r, "dod_index"), Some(AnyValue::Int(2)));
    }

    #[test]
    fn uncheck_dod_emits_dod_unchecked_with_index() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            ops.uncheck_dod(ProjectId(7), TaskId(1), 0).await.unwrap();
        });

        let r = one(&records, "senko.task.dod_unchecked");
        assert_eq!(lookup_attr(r, "dod_index"), Some(AnyValue::Int(0)));
    }

    // --- senko.task.dependency_{added,removed} / dependencies_set ------------

    #[test]
    fn add_dependency_emits_dependency_added_with_dep_id() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            ops.add_dependency(ProjectId(7), TaskId(1), TaskId(99))
                .await
                .unwrap();
        });

        let r = one(&records, "senko.task.dependency_added");
        assert_eq!(lookup_attr(r, "senko.task.id"), Some(AnyValue::Int(1)));
        assert_eq!(lookup_attr(r, "senko.project.id"), Some(AnyValue::Int(7)));
        assert_eq!(lookup_attr(r, "dep_id"), Some(AnyValue::Int(99)));
    }

    #[test]
    fn remove_dependency_emits_dependency_removed_with_dep_id() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            ops.remove_dependency(ProjectId(7), TaskId(1), TaskId(88))
                .await
                .unwrap();
        });

        let r = one(&records, "senko.task.dependency_removed");
        assert_eq!(lookup_attr(r, "dep_id"), Some(AnyValue::Int(88)));
    }

    #[test]
    fn set_dependencies_emits_dependencies_set_with_json_deps() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream().await;
            let ops = make_remote_ops(&base);
            ops.set_dependencies(
                ProjectId(7),
                TaskId(1),
                &[TaskId(10), TaskId(20), TaskId(30)],
            )
            .await
            .unwrap();
        });

        let r = one(&records, "senko.task.dependencies_set");
        assert_eq!(
            lookup_attr(r, "deps"),
            Some(AnyValue::String("[10,20,30]".into())),
        );
    }

    // --- senko.contract.id attribute (task #383) ----------------------------
    //
    // Per Contract #8 the attribute key is fixed (`senko.contract.id`, i64).
    // The Remote-side emit reads `task.contract_id` from the HTTP-response
    // Task — i.e. the upstream's authoritative post-state. The wrapper macro
    // `emit_task_event!` omits the key entirely when `contract_id` is `None`.

    use crate::domain::task::{CreateTaskParams, Priority as DomPriority};

    fn empty_create_params() -> CreateTaskParams {
        CreateTaskParams {
            title: "t".into(),
            background: None,
            description: None,
            priority: Some(DomPriority::P2),
            definition_of_done: vec![],
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

    /// All 12 events emitted by RemoteTaskOperations under a contract-less
    /// upstream Task: assert `senko.contract.id` is absent on every record.
    #[test]
    fn no_event_carries_contract_id_when_upstream_task_has_none() {
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream_with_contract(None).await;
            let ops = make_remote_ops(&base);
            ops.create_task(ProjectId(7), &empty_create_params())
                .await
                .unwrap();
            ops.publish_task(ProjectId(7), TaskId(1)).await.unwrap();
            ops.start_task(ProjectId(7), TaskId(1), None, None, None)
                .await
                .unwrap();
            ops.resume_task(ProjectId(7), TaskId(1), None, None)
                .await
                .unwrap();
            ops.next_task(ProjectId(7), None, None, true, None)
                .await
                .unwrap();
            ops.complete_task(ProjectId(7), TaskId(1), false)
                .await
                .unwrap();
            ops.cancel_task(ProjectId(7), TaskId(1), Some("r".into()))
                .await
                .unwrap();
            let upd = UpdateTaskParams {
                description: Some(Some("x".into())),
                ..empty_update_params()
            };
            ops.edit_task(ProjectId(7), TaskId(1), &upd).await.unwrap();
            let arr_upd = UpdateTaskArrayParams {
                set_tags: Some(vec!["a".into()]),
                ..empty_array_params()
            };
            ops.edit_task_arrays(ProjectId(7), TaskId(1), &arr_upd)
                .await
                .unwrap();
            ops.check_dod(ProjectId(7), TaskId(1), 1).await.unwrap();
            ops.uncheck_dod(ProjectId(7), TaskId(1), 1).await.unwrap();
            ops.add_dependency(ProjectId(7), TaskId(1), TaskId(9))
                .await
                .unwrap();
            ops.remove_dependency(ProjectId(7), TaskId(1), TaskId(9))
                .await
                .unwrap();
            ops.set_dependencies(ProjectId(7), TaskId(1), &[TaskId(9)])
                .await
                .unwrap();
        });

        let task_events: Vec<&str> = records
            .iter()
            .filter_map(|r| r.event_name())
            .filter(|n| n.starts_with("senko.task."))
            .collect();
        assert!(
            !task_events.is_empty(),
            "expected at least one senko.task.* record"
        );
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

    /// Same battery, but with the upstream returning `contract_id = 42`.
    /// Assert every emitted `senko.task.*` record carries the post-response
    /// `senko.contract.id = 42`.
    #[test]
    fn every_event_carries_contract_id_from_upstream_task() {
        const CID: i64 = 42;
        let records = with_business_records(|| async {
            let base = spawn_mock_upstream_with_contract(Some(CID)).await;
            let ops = make_remote_ops(&base);
            ops.create_task(ProjectId(7), &empty_create_params())
                .await
                .unwrap();
            ops.publish_task(ProjectId(7), TaskId(1)).await.unwrap();
            ops.start_task(ProjectId(7), TaskId(1), None, None, None)
                .await
                .unwrap();
            ops.resume_task(ProjectId(7), TaskId(1), None, None)
                .await
                .unwrap();
            ops.next_task(ProjectId(7), None, None, true, None)
                .await
                .unwrap();
            ops.complete_task(ProjectId(7), TaskId(1), false)
                .await
                .unwrap();
            ops.cancel_task(ProjectId(7), TaskId(1), Some("r".into()))
                .await
                .unwrap();
            let upd = UpdateTaskParams {
                description: Some(Some("x".into())),
                ..empty_update_params()
            };
            ops.edit_task(ProjectId(7), TaskId(1), &upd).await.unwrap();
            let arr_upd = UpdateTaskArrayParams {
                set_tags: Some(vec!["a".into()]),
                ..empty_array_params()
            };
            ops.edit_task_arrays(ProjectId(7), TaskId(1), &arr_upd)
                .await
                .unwrap();
            ops.check_dod(ProjectId(7), TaskId(1), 1).await.unwrap();
            ops.uncheck_dod(ProjectId(7), TaskId(1), 1).await.unwrap();
            ops.add_dependency(ProjectId(7), TaskId(1), TaskId(9))
                .await
                .unwrap();
            ops.remove_dependency(ProjectId(7), TaskId(1), TaskId(9))
                .await
                .unwrap();
            ops.set_dependencies(ProjectId(7), TaskId(1), &[TaskId(9)])
                .await
                .unwrap();
        });

        let expected_events = [
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

        for name in expected_events {
            let r = records
                .iter()
                .find(|r| r.event_name() == Some(name))
                .unwrap_or_else(|| panic!("expected at least one {name} record"));
            assert_eq!(
                lookup_attr(r, "senko.contract.id"),
                Some(AnyValue::Int(CID)),
                "{name} missing or wrong senko.contract.id",
            );
        }

        // Also check the `started` records — both `start_task` and `next_task`
        // emit `senko.task.started`; both must carry the contract id.
        let started_records: Vec<&SdkLogRecord> = records
            .iter()
            .filter(|r| r.event_name() == Some("senko.task.started"))
            .collect();
        assert!(
            started_records.len() >= 2,
            "expected `senko.task.started` from both start_task and next_task",
        );
        for r in started_records {
            assert_eq!(
                lookup_attr(r, "senko.contract.id"),
                Some(AnyValue::Int(CID)),
            );
        }
    }
}
