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
use crate::domain::project::ProjectId;
use crate::domain::task::{
    CreateTaskParams, Cursor, ListTasksFilter, ListTasksPage, MetadataUpdate, Priority, Task,
    TaskEvent, TaskId, TaskStatus, UnblockedTask, UpdateTaskArrayParams, UpdateTaskParams,
};
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
    pub fn new(base_url: &str, api_key: Option<String>, hooks: Arc<dyn HookExecutor>) -> Self {
        Self {
            http: HttpClient::new(base_url, api_key),
            hooks,
        }
    }

    fn project_url(&self, project_id: ProjectId, path: &str) -> String {
        self.http.project_url(project_id, path)
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        self.http.auth(builder)
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

#[async_trait]
impl TaskOperations for RemoteTaskOperations {
    // --- State transitions ---

    async fn create_task(&self, project_id: ProjectId, params: &CreateTaskParams) -> Result<Task> {
        let resp = self
            .auth(
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

        Ok(task)
    }

    async fn publish_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task> {
        let prev_status = self.get_task(project_id, id).await?.status();

        let resp = self
            .auth(
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

        Ok(task)
    }

    async fn start_task(
        &self,
        project_id: ProjectId,
        id: TaskId,
        session_id: Option<String>,
        _user_id: Option<i64>,
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
            .auth(
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

        Ok(task)
    }

    async fn next_task(
        &self,
        project_id: ProjectId,
        session_id: Option<String>,
        _user_id: Option<i64>,
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
            .auth(
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
            .auth(
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
            .auth(
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
            .auth(self.client().get(self.project_url(
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
            .auth(
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
            .auth(
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
        if let Some(after) = filter.after {
            params.push(format!(
                "after={}",
                utf8_percent_encode(&Cursor::encode(after), NON_ALPHANUMERIC)
            ));
        }

        if !params.is_empty() {
            url = format!("{url}?{}", params.join("&"));
        }

        let resp = self.auth(self.client().get(&url)).send().await?;
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
            .auth(self.client().get(self.project_url(project_id, "/stats")))
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
            .auth(
                self.client()
                    .put(self.project_url(project_id, &format!("/tasks/{id}")))
                    .json(&body),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn edit_task_arrays(
        &self,
        project_id: ProjectId,
        id: TaskId,
        params: &UpdateTaskArrayParams,
    ) -> Result<()> {
        let body = array_params_to_json(params);
        let resp = self
            .auth(
                self.client()
                    .put(self.project_url(project_id, &format!("/tasks/{id}")))
                    .json(&body),
            )
            .send()
            .await?;
        read_json_or_error::<Task>(resp).await?;
        Ok(())
    }

    async fn delete_task(&self, project_id: ProjectId, id: TaskId) -> Result<()> {
        let resp = self
            .auth(
                self.client()
                    .delete(self.project_url(project_id, &format!("/tasks/{id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    async fn save_task(&self, project_id: ProjectId, id: TaskId, task: &Task) -> Result<()> {
        let resp = self
            .auth(
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
            self.auth(self.client().post(
                self.project_url(project_id, &format!("/tasks/{task_id}/dod/{index}/check")),
            ))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn uncheck_dod(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        index: usize,
    ) -> Result<Task> {
        let resp = self
            .auth(self.client().post(
                self.project_url(project_id, &format!("/tasks/{task_id}/dod/{index}/uncheck")),
            ))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    // --- Dependencies ---

    async fn add_dependency(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_id: TaskId,
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client()
                    .post(self.project_url(project_id, &format!("/tasks/{task_id}/deps")))
                    .json(&json!({ "dep_id": dep_id })),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn remove_dependency(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_id: TaskId,
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client().delete(
                    self.project_url(project_id, &format!("/tasks/{task_id}/deps/{dep_id}")),
                ),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn set_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        dep_ids: &[TaskId],
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client()
                    .put(self.project_url(project_id, &format!("/tasks/{task_id}/deps")))
                    .json(&json!({ "dep_ids": dep_ids })),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn list_dependencies(&self, project_id: ProjectId, task_id: TaskId) -> Result<Vec<Task>> {
        let resp = self
            .auth(
                self.client()
                    .get(self.project_url(project_id, &format!("/tasks/{task_id}/deps"))),
            )
            .send()
            .await?;
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
