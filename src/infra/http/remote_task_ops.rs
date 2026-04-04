use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::application::port::task_operations::{CompleteResult, PreviewResult};
use crate::application::port::{HookExecutor, TaskOperations};
use crate::application::HookTrigger;
use crate::domain::error::DomainError;
use crate::domain::task::{
    CreateTaskParams, ListTasksFilter, Priority, Task, TaskEvent, TaskStatus, UnblockedTask,
    UpdateTaskArrayParams, UpdateTaskParams,
};

use super::{
    array_params_to_json, check_success, extract_error, read_json_or_error, update_params_to_json,
};

/// HTTP client implementing `TaskOperations` directly.
///
/// Each method maps to a single API endpoint call. Domain logic is executed
/// server-side; this client only handles HTTP transport and optional
/// client-side hook firing.
pub struct RemoteTaskOperations {
    base_url: String,
    client: reqwest::Client,
    api_key: Option<String>,
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
    id: i64,
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
    id: i64,
    title: String,
    status: String,
    priority: String,
}

impl RemoteTaskOperations {
    pub fn new(
        base_url: &str,
        api_key: Option<String>,
        hooks: Arc<dyn HookExecutor>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            api_key,
            hooks,
        }
    }

    fn project_url(&self, project_id: i64, path: &str) -> String {
        format!("{}/api/v1/projects/{project_id}{path}", self.base_url)
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => builder.bearer_auth(key),
            None => builder,
        }
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

    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task> {
        let resp = self
            .auth(
                self.client
                    .post(self.project_url(project_id, "/tasks"))
                    .json(params),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

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
        let prev_status = self.get_task(project_id, id).await?.status();

        let resp = self
            .auth(
                self.client
                    .post(self.project_url(project_id, &format!("/tasks/{id}/ready"))),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

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
        metadata: Option<serde_json::Value>,
    ) -> Result<Task> {
        let prev_status = self.get_task(project_id, id).await?.status();

        let resp = self
            .auth(
                self.client
                    .post(self.project_url(project_id, &format!("/tasks/{id}/start")))
                    .json(&json!({ "session_id": session_id, "user_id": user_id, "metadata": metadata })),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

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
        metadata: Option<serde_json::Value>,
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client
                    .post(self.project_url(project_id, "/tasks/next"))
                    .json(&json!({ "session_id": session_id, "user_id": user_id, "metadata": metadata })),
            )
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
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

        if !resp.status().is_success() {
            bail!("{}", extract_error(resp).await);
        }

        let task: Task = resp.json().await?;

        self.hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Started),
                Some(&task),
                Some(TaskStatus::Todo),
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
    ) -> Result<CompleteResult> {
        let prev_status = self.get_task(project_id, id).await?.status();

        let body = if skip_pr_check {
            json!({ "skip_pr_check": true })
        } else {
            json!({})
        };
        let resp = self
            .auth(
                self.client
                    .post(self.project_url(project_id, &format!("/tasks/{id}/complete")))
                    .json(&body),
            )
            .send()
            .await?;
        let api_resp: CompleteApiResponse = read_json_or_error(resp).await?;
        let unblocked = parse_unblocked(api_resp.unblocked_tasks);

        self.hooks
            .fire(
                &HookTrigger::Task(TaskEvent::Completed),
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
        project_id: i64,
        id: i64,
        reason: Option<String>,
    ) -> Result<Task> {
        let prev_status = self.get_task(project_id, id).await?.status();

        let body = match reason {
            Some(ref r) => json!({ "reason": r }),
            None => json!({}),
        };
        let resp = self
            .auth(
                self.client
                    .post(self.project_url(project_id, &format!("/tasks/{id}/cancel")))
                    .json(&body),
            )
            .send()
            .await?;
        let task: Task = read_json_or_error(resp).await?;

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

    // --- Preview ---

    async fn preview_transition(
        &self,
        project_id: i64,
        task_id: i64,
        target: TaskStatus,
    ) -> Result<PreviewResult> {
        let task = self.get_task(project_id, task_id).await?;

        let resp = self
            .auth(self.client.get(self.project_url(
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
                    None, None, None,
                    priority,
                    status,
                    None, None,
                    String::new(), String::new(),
                    None, None, None, None,
                    None, None, None,
                    Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(),
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

    async fn preview_next(&self, project_id: i64) -> Result<PreviewResult> {
        let resp = self
            .auth(
                self.client
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

    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let resp = self
            .auth(
                self.client
                    .get(self.project_url(project_id, &format!("/tasks/{id}"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn list_tasks(
        &self,
        project_id: i64,
        filter: &ListTasksFilter,
    ) -> Result<Vec<Task>> {
        let mut url = self.project_url(project_id, "/tasks");
        let mut params: Vec<String> = Vec::new();

        for status in &filter.statuses {
            params.push(format!("status={}", status.to_string().to_lowercase()));
        }
        for tag in &filter.tags {
            params.push(format!("tag={tag}"));
        }
        if let Some(dep) = filter.depends_on {
            params.push(format!("depends_on={dep}"));
        }
        if filter.ready {
            params.push("ready=true".into());
        }

        if !params.is_empty() {
            url = format!("{url}?{}", params.join("&"));
        }

        let resp = self.auth(self.client.get(&url)).send().await?;
        read_json_or_error(resp).await
    }

    async fn list_all_tags(&self, project_id: i64) -> Result<Vec<String>> {
        let tasks = self
            .list_tasks(project_id, &ListTasksFilter::default())
            .await?;
        let mut tags: Vec<String> = tasks
            .iter()
            .flat_map(|t| t.tags().iter().cloned())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        tags.sort();
        Ok(tags)
    }

    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>> {
        let resp = self
            .auth(
                self.client
                    .get(self.project_url(project_id, "/stats")),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    // --- Edit ---

    async fn edit_task(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskParams,
    ) -> Result<Task> {
        let body = update_params_to_json(params);
        let resp = self
            .auth(
                self.client
                    .put(self.project_url(project_id, &format!("/tasks/{id}")))
                    .json(&body),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn edit_task_arrays(
        &self,
        project_id: i64,
        id: i64,
        params: &UpdateTaskArrayParams,
    ) -> Result<()> {
        let body = array_params_to_json(params);
        let resp = self
            .auth(
                self.client
                    .put(self.project_url(project_id, &format!("/tasks/{id}")))
                    .json(&body),
            )
            .send()
            .await?;
        read_json_or_error::<Task>(resp).await?;
        Ok(())
    }

    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()> {
        let resp = self
            .auth(
                self.client
                    .delete(self.project_url(project_id, &format!("/tasks/{id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    // --- Definition of Done ---

    async fn check_dod(
        &self,
        project_id: i64,
        task_id: i64,
        index: usize,
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client.post(self.project_url(
                    project_id,
                    &format!("/tasks/{task_id}/dod/{index}/check"),
                )),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn uncheck_dod(
        &self,
        project_id: i64,
        task_id: i64,
        index: usize,
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client.post(self.project_url(
                    project_id,
                    &format!("/tasks/{task_id}/dod/{index}/uncheck"),
                )),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    // --- Dependencies ---

    async fn add_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client
                    .post(self.project_url(project_id, &format!("/tasks/{task_id}/deps")))
                    .json(&json!({ "dep_id": dep_id })),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn remove_dependency(
        &self,
        project_id: i64,
        task_id: i64,
        dep_id: i64,
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client.delete(self.project_url(
                    project_id,
                    &format!("/tasks/{task_id}/deps/{dep_id}"),
                )),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn set_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
        dep_ids: &[i64],
    ) -> Result<Task> {
        let resp = self
            .auth(
                self.client
                    .put(self.project_url(project_id, &format!("/tasks/{task_id}/deps")))
                    .json(&json!({ "dep_ids": dep_ids })),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn list_dependencies(
        &self,
        project_id: i64,
        task_id: i64,
    ) -> Result<Vec<Task>> {
        let resp = self
            .auth(
                self.client
                    .get(self.project_url(project_id, &format!("/tasks/{task_id}/deps"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        self.list_tasks(
            project_id,
            &ListTasksFilter {
                ready: true,
                ..Default::default()
            },
        )
        .await
    }

    async fn ready_count(&self, project_id: i64) -> Result<i64> {
        let tasks = self.list_ready_tasks(project_id).await?;
        Ok(tasks.len() as i64)
    }
}
