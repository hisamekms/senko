use std::collections::HashMap;

use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::json;

use crate::application::port::TaskQueryPort;
use crate::domain::error::DomainError;
use crate::domain::repository::{ApiKeyRepository, ProjectRepository, TaskRepository, UserRepository};
use crate::domain::project::{CreateProjectParams, Project};
use crate::domain::task::{
    CreateTaskParams, ListTasksFilter, Task, UpdateTaskArrayParams, UpdateTaskParams,
};
use crate::domain::user::{
    AddProjectMemberParams, ApiKey, ApiKeyWithSecret, CreateUserParams, NewApiKey, ProjectMember,
    Role, User,
};

pub struct HttpBackend {
    base_url: String,
    client: reqwest::Client,
    api_key: Option<String>,
}

impl HttpBackend {
    pub fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            api_key: None,
        }
    }

    pub fn with_api_key(base_url: &str, api_key: String) -> Self {
        let mut backend = Self::new(base_url);
        backend.api_key = Some(api_key);
        backend
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
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

/// Extract error message from a JSON error response body.
async fn extract_error(resp: reqwest::Response) -> String {
    resp.json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .unwrap_or_else(|| "unknown error".into())
}

/// Read a successful JSON response, or bail with the error body on non-2xx.
async fn read_json_or_error<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    if resp.status().is_success() {
        Ok(resp.json().await?)
    } else {
        bail!("{}", extract_error(resp).await);
    }
}

/// Check that a response is successful (2xx), ignoring the body. Bail on error.
async fn check_success(resp: reqwest::Response) -> Result<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        bail!("{}", extract_error(resp).await);
    }
}

/// Build the JSON body for `PUT /tasks/{id}` from `UpdateTaskParams`.
fn update_params_to_json(params: &UpdateTaskParams) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    if let Some(ref title) = params.title {
        map.insert("title".into(), json!(title));
    }
    if let Some(ref priority) = params.priority {
        map.insert("priority".into(), json!(priority));
    }

    macro_rules! clearable {
        ($field:ident) => {
            if let Some(ref outer) = params.$field {
                match outer {
                    None => {
                        map.insert(concat!("clear_", stringify!($field)).into(), json!(true));
                    }
                    Some(val) => {
                        map.insert(stringify!($field).into(), json!(val));
                    }
                }
            }
        };
    }

    clearable!(background);
    clearable!(description);
    clearable!(plan);
    clearable!(branch);
    clearable!(pr_url);
    clearable!(metadata);
    clearable!(cancel_reason);
    clearable!(assignee_session_id);
    clearable!(started_at);
    clearable!(completed_at);
    clearable!(canceled_at);

    if let Some(ref outer) = params.assignee_user_id {
        match outer {
            None => {
                map.insert("clear_assignee_user_id".into(), json!(true));
            }
            Some(val) => {
                map.insert("assignee_user_id".into(), json!(val));
            }
        }
    }

    serde_json::Value::Object(map)
}

/// Build the JSON body for `PUT /tasks/{id}` from `UpdateTaskArrayParams`.
fn array_params_to_json(params: &UpdateTaskArrayParams) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    macro_rules! array_field {
        ($set:ident, $add:ident, $remove:ident) => {
            if let Some(ref v) = params.$set {
                map.insert(stringify!($set).into(), json!(v));
            }
            if !params.$add.is_empty() {
                map.insert(stringify!($add).into(), json!(params.$add));
            }
            if !params.$remove.is_empty() {
                map.insert(stringify!($remove).into(), json!(params.$remove));
            }
        };
    }

    array_field!(set_tags, add_tags, remove_tags);
    array_field!(
        set_definition_of_done,
        add_definition_of_done,
        remove_definition_of_done
    );
    array_field!(set_in_scope, add_in_scope, remove_in_scope);
    array_field!(set_out_of_scope, add_out_of_scope, remove_out_of_scope);

    serde_json::Value::Object(map)
}

#[async_trait]
impl ProjectRepository for HttpBackend {
    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project> {
        let resp = self.auth(self
            .client
            .post(self.url("/api/v1/projects"))
            .json(params))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_project(&self, id: i64) -> Result<Project> {
        let resp = self.auth(self
            .client
            .get(self.url(&format!("/api/v1/projects/{id}"))))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        let projects: Vec<Project> = {
            let resp = self.auth(self
                .client
                .get(self.url("/api/v1/projects")))
                .send()
                .await?;
            read_json_or_error(resp).await?
        };
        projects
            .into_iter()
            .find(|p| p.name() == name)
            .ok_or_else(|| anyhow::anyhow!("project not found"))
    }

    async fn list_projects(&self) -> Result<Vec<Project>> {
        let resp = self.auth(self
            .client
            .get(self.url("/api/v1/projects")))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn delete_project(&self, id: i64) -> Result<()> {
        let resp = self.auth(self
            .client
            .delete(self.url(&format!("/api/v1/projects/{id}"))))
            .send()
            .await?;
        check_success(resp).await
    }

    async fn add_project_member(
        &self,
        project_id: i64,
        params: &AddProjectMemberParams,
    ) -> Result<ProjectMember> {
        let resp = self.auth(
            self.client.post(self.project_url(project_id, "/members"))
                .json(&json!({ "user_id": params.user_id, "role": params.role }))
        ).send().await?;
        read_json_or_error(resp).await
    }

    async fn remove_project_member(&self, project_id: i64, user_id: i64) -> Result<()> {
        let resp = self.auth(self
            .client
            .delete(self.project_url(project_id, &format!("/members/{user_id}"))))
            .send()
            .await?;
        check_success(resp).await
    }

    async fn list_project_members(&self, project_id: i64) -> Result<Vec<ProjectMember>> {
        let resp = self.auth(self
            .client
            .get(self.project_url(project_id, "/members")))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_project_member(&self, project_id: i64, user_id: i64) -> Result<ProjectMember> {
        let resp = self.auth(self
            .client
            .get(self.project_url(project_id, &format!("/members/{user_id}"))))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn update_member_role(
        &self,
        project_id: i64,
        user_id: i64,
        role: Role,
    ) -> Result<ProjectMember> {
        let resp = self.auth(self
            .client
            .put(self.project_url(project_id, &format!("/members/{user_id}")))
            .json(&json!({ "role": role })))
            .send()
            .await?;
        read_json_or_error(resp).await
    }
}

#[async_trait]
impl UserRepository for HttpBackend {
    async fn create_user(&self, params: &CreateUserParams) -> Result<User> {
        let resp = self.auth(self
            .client
            .post(self.url("/api/v1/users"))
            .json(params))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_user(&self, id: i64) -> Result<User> {
        let resp = self.auth(self
            .client
            .get(self.url(&format!("/api/v1/users/{id}"))))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_user_by_username(&self, username: &str) -> Result<User> {
        let users: Vec<User> = {
            let resp = self.auth(self
                .client
                .get(self.url("/api/v1/users")))
                .send()
                .await?;
            read_json_or_error(resp).await?
        };
        users
            .into_iter()
            .find(|u| u.username() == username)
            .ok_or_else(|| anyhow::anyhow!("user not found"))
    }

    async fn list_users(&self) -> Result<Vec<User>> {
        let resp = self.auth(self
            .client
            .get(self.url("/api/v1/users")))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn delete_user(&self, id: i64) -> Result<()> {
        let resp = self.auth(self
            .client
            .delete(self.url(&format!("/api/v1/users/{id}"))))
            .send()
            .await?;
        check_success(resp).await
    }
}

#[async_trait]
impl ApiKeyRepository for HttpBackend {
    fn supports_api_key_auth(&self) -> bool {
        false
    }

    async fn create_api_key(&self, user_id: i64, name: &str, _new_key: &NewApiKey) -> Result<ApiKeyWithSecret> {
        let resp = self.auth(
            self.client.post(self.url(&format!("/api/v1/users/{user_id}/api-keys")))
                .json(&json!({ "name": name }))
        ).send().await?;
        read_json_or_error(resp).await
    }

    async fn get_user_by_api_key(&self, _key_hash: &str) -> Result<User> {
        Err(DomainError::UnsupportedOperation {
            operation: "get_user_by_api_key".into(),
        }.into())
    }

    async fn list_api_keys(&self, user_id: i64) -> Result<Vec<ApiKey>> {
        let resp = self.auth(self.client.get(self.url(&format!("/api/v1/users/{user_id}/api-keys"))))
            .send().await?;
        read_json_or_error(resp).await
    }

    async fn delete_api_key(&self, key_id: i64) -> Result<()> {
        let resp = self.auth(self.client.delete(self.url(&format!("/api/v1/users/0/api-keys/{key_id}"))))
            .send().await?;
        check_success(resp).await
    }
}

#[async_trait]
impl TaskRepository for HttpBackend {
    async fn create_task(&self, project_id: i64, params: &CreateTaskParams) -> Result<Task> {
        let resp = self.auth(self
            .client
            .post(self.project_url(project_id, "/tasks"))
            .json(params))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_task(&self, project_id: i64, id: i64) -> Result<Task> {
        let resp = self.auth(self
            .client
            .get(self.project_url(project_id, &format!("/tasks/{id}"))))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn update_task(&self, project_id: i64, id: i64, params: &UpdateTaskParams) -> Result<Task> {
        let body = update_params_to_json(params);
        let resp = self.auth(self
            .client
            .put(self.project_url(project_id, &format!("/tasks/{id}")))
            .json(&body))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn update_task_arrays(&self, project_id: i64, id: i64, params: &UpdateTaskArrayParams) -> Result<()> {
        let body = array_params_to_json(params);
        let resp = self.auth(self
            .client
            .put(self.project_url(project_id, &format!("/tasks/{id}")))
            .json(&body))
            .send()
            .await?;
        read_json_or_error::<Task>(resp).await?;
        Ok(())
    }

    async fn delete_task(&self, project_id: i64, id: i64) -> Result<()> {
        let resp = self.auth(self
            .client
            .delete(self.project_url(project_id, &format!("/tasks/{id}"))))
            .send()
            .await?;
        check_success(resp).await
    }

    async fn add_dependency(&self, project_id: i64, task_id: i64, dep_id: i64) -> Result<Task> {
        let resp = self.auth(self
            .client
            .post(self.project_url(project_id, &format!("/tasks/{task_id}/deps")))
            .json(&json!({ "dep_id": dep_id })))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn remove_dependency(&self, project_id: i64, task_id: i64, dep_id: i64) -> Result<Task> {
        let resp = self.auth(self
            .client
            .delete(self.project_url(project_id, &format!("/tasks/{task_id}/deps/{dep_id}"))))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn set_dependencies(&self, project_id: i64, task_id: i64, dep_ids: &[i64]) -> Result<Task> {
        let current_deps = self.list_dependencies(project_id, task_id).await?;
        let current_ids: std::collections::HashSet<i64> =
            current_deps.iter().map(|t| t.id()).collect();
        let desired: std::collections::HashSet<i64> = dep_ids.iter().copied().collect();

        for id in current_ids.difference(&desired) {
            self.remove_dependency(project_id, task_id, *id).await?;
        }
        for id in desired.difference(&current_ids) {
            self.add_dependency(project_id, task_id, *id).await?;
        }

        self.get_task(project_id, task_id).await
    }

    async fn list_dependencies(&self, project_id: i64, task_id: i64) -> Result<Vec<Task>> {
        let resp = self.auth(self
            .client
            .get(self.project_url(project_id, &format!("/tasks/{task_id}/deps"))))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn save(&self, task: &Task) -> Result<()> {
        let resp = self.auth(self
            .client
            .put(self.project_url(task.project_id(), &format!("/tasks/{}", task.id())))
            .json(task))
            .send()
            .await?;
        read_json_or_error::<Task>(resp).await?;
        Ok(())
    }

}

#[async_trait]
impl TaskQueryPort for HttpBackend {
    async fn list_tasks(&self, project_id: i64, filter: &ListTasksFilter) -> Result<Vec<Task>> {
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

    async fn next_task(&self, project_id: i64) -> Result<Option<Task>> {
        let resp = self.auth(self
            .client
            .post(self.project_url(project_id, "/tasks/next"))
            .json(&json!({})))
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if resp.status().is_success() {
            Ok(Some(resp.json().await?))
        } else {
            bail!("{}", extract_error(resp).await);
        }
    }

    async fn task_stats(&self, project_id: i64) -> Result<HashMap<String, i64>> {
        let resp = self.auth(self
            .client
            .get(self.project_url(project_id, "/stats")))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn ready_count(&self, project_id: i64) -> Result<i64> {
        let tasks = self.list_tasks(project_id, &ListTasksFilter {
            ready: true,
            ..Default::default()
        }).await?;
        Ok(tasks.len() as i64)
    }

    async fn list_ready_tasks(&self, project_id: i64) -> Result<Vec<Task>> {
        self.list_tasks(project_id, &ListTasksFilter {
            ready: true,
            ..Default::default()
        }).await
    }
}
