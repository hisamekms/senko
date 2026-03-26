use std::collections::HashMap;

use anyhow::{bail, Result};
use serde_json::json;
use ureq::http::Response;
use ureq::{Agent, Body};

use crate::db::TaskBackend;
use crate::models::{
    CreateTaskParams, ListTasksFilter, Task, UpdateTaskArrayParams, UpdateTaskParams,
};

pub struct HttpBackend {
    base_url: String,
    agent: Agent,
}

impl HttpBackend {
    pub fn new(base_url: &str) -> Self {
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            agent,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

/// Extract error message from a JSON error response body.
fn extract_error(resp: Response<Body>) -> String {
    resp.into_body()
        .read_json::<serde_json::Value>()
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .unwrap_or_else(|| "unknown error".into())
}

/// Read a successful JSON response, or bail with the error body on non-2xx.
fn read_json_or_error<T: serde::de::DeserializeOwned>(resp: Response<Body>) -> Result<T> {
    if resp.status().is_success() {
        Ok(resp.into_body().read_json()?)
    } else {
        bail!("{}", extract_error(resp));
    }
}

/// Check that a response is successful (2xx), ignoring the body. Bail on error.
fn check_success(resp: Response<Body>) -> Result<()> {
    if resp.status().is_success() {
        Ok(())
    } else {
        bail!("{}", extract_error(resp));
    }
}

/// Build the JSON body for `PUT /api/v1/tasks/{id}` from `UpdateTaskParams`.
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

    serde_json::Value::Object(map)
}

/// Build the JSON body for `PUT /api/v1/tasks/{id}` from `UpdateTaskArrayParams`.
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

impl TaskBackend for HttpBackend {
    fn create_task(&self, params: &CreateTaskParams) -> Result<Task> {
        let resp = self
            .agent
            .post(&self.url("/api/v1/tasks"))
            .send_json(params)?;
        read_json_or_error(resp)
    }

    fn get_task(&self, id: i64) -> Result<Task> {
        let resp = self
            .agent
            .get(&self.url(&format!("/api/v1/tasks/{id}")))
            .call()?;
        read_json_or_error(resp)
    }

    fn ready_task(&self, id: i64) -> Result<Task> {
        let resp = self
            .agent
            .post(&self.url(&format!("/api/v1/tasks/{id}/ready")))
            .send_empty()?;
        read_json_or_error(resp)
    }

    fn start_task(
        &self,
        id: i64,
        assignee_session_id: Option<String>,
        _started_at: &str,
    ) -> Result<Task> {
        let resp = self
            .agent
            .post(&self.url(&format!("/api/v1/tasks/{id}/start")))
            .send_json(&json!({ "session_id": assignee_session_id }))?;
        read_json_or_error(resp)
    }

    fn complete_task(&self, id: i64, _completed_at: &str) -> Result<Task> {
        let resp = self
            .agent
            .post(&self.url(&format!("/api/v1/tasks/{id}/complete")))
            .send_json(&json!({}))?;
        read_json_or_error(resp)
    }

    fn cancel_task(&self, id: i64, _canceled_at: &str, reason: Option<String>) -> Result<Task> {
        let resp = self
            .agent
            .post(&self.url(&format!("/api/v1/tasks/{id}/cancel")))
            .send_json(&json!({ "reason": reason }))?;
        read_json_or_error(resp)
    }

    fn update_task(&self, id: i64, params: &UpdateTaskParams) -> Result<Task> {
        let body = update_params_to_json(params);
        let resp = self
            .agent
            .put(&self.url(&format!("/api/v1/tasks/{id}")))
            .send_json(&body)?;
        read_json_or_error(resp)
    }

    fn update_task_arrays(&self, id: i64, params: &UpdateTaskArrayParams) -> Result<()> {
        let body = array_params_to_json(params);
        let resp = self
            .agent
            .put(&self.url(&format!("/api/v1/tasks/{id}")))
            .send_json(&body)?;
        read_json_or_error::<Task>(resp)?;
        Ok(())
    }

    fn delete_task(&self, id: i64) -> Result<()> {
        let resp = self
            .agent
            .delete(&self.url(&format!("/api/v1/tasks/{id}")))
            .call()?;
        check_success(resp)
    }

    fn list_tasks(&self, filter: &ListTasksFilter) -> Result<Vec<Task>> {
        let mut url = self.url("/api/v1/tasks");
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

        let resp = self.agent.get(&url).call()?;
        read_json_or_error(resp)
    }

    fn next_task(&self) -> Result<Option<Task>> {
        let resp = self
            .agent
            .post(&self.url("/api/v1/tasks/next"))
            .send_json(&json!({}))?;
        if resp.status() == 404 {
            return Ok(None);
        }
        if resp.status().is_success() {
            Ok(Some(resp.into_body().read_json()?))
        } else {
            bail!("{}", extract_error(resp));
        }
    }

    fn task_stats(&self) -> Result<HashMap<String, i64>> {
        let resp = self.agent.get(&self.url("/api/v1/stats")).call()?;
        read_json_or_error(resp)
    }

    fn ready_count(&self) -> Result<i64> {
        let tasks = self.list_tasks(&ListTasksFilter {
            ready: true,
            ..Default::default()
        })?;
        Ok(tasks.len() as i64)
    }

    fn list_ready_tasks(&self) -> Result<Vec<Task>> {
        self.list_tasks(&ListTasksFilter {
            ready: true,
            ..Default::default()
        })
    }

    fn add_dependency(&self, task_id: i64, dep_id: i64) -> Result<Task> {
        let resp = self
            .agent
            .post(&self.url(&format!("/api/v1/tasks/{task_id}/deps")))
            .send_json(&json!({ "dep_id": dep_id }))?;
        read_json_or_error(resp)
    }

    fn remove_dependency(&self, task_id: i64, dep_id: i64) -> Result<Task> {
        let resp = self
            .agent
            .delete(&self.url(&format!("/api/v1/tasks/{task_id}/deps/{dep_id}")))
            .call()?;
        read_json_or_error(resp)
    }

    fn set_dependencies(&self, task_id: i64, dep_ids: &[i64]) -> Result<Task> {
        let current_deps = self.list_dependencies(task_id)?;
        let current_ids: std::collections::HashSet<i64> =
            current_deps.iter().map(|t| t.id).collect();
        let desired: std::collections::HashSet<i64> = dep_ids.iter().copied().collect();

        for id in current_ids.difference(&desired) {
            self.remove_dependency(task_id, *id)?;
        }
        for id in desired.difference(&current_ids) {
            self.add_dependency(task_id, *id)?;
        }

        self.get_task(task_id)
    }

    fn list_dependencies(&self, task_id: i64) -> Result<Vec<Task>> {
        let resp = self
            .agent
            .get(&self.url(&format!("/api/v1/tasks/{task_id}/deps")))
            .call()?;
        read_json_or_error(resp)
    }

    fn check_dod(&self, task_id: i64, index: usize) -> Result<Task> {
        let resp = self
            .agent
            .post(&self.url(&format!("/api/v1/tasks/{task_id}/dod/{index}/check")))
            .send_empty()?;
        read_json_or_error(resp)
    }

    fn uncheck_dod(&self, task_id: i64, index: usize) -> Result<Task> {
        let resp = self
            .agent
            .post(&self.url(&format!(
                "/api/v1/tasks/{task_id}/dod/{index}/uncheck"
            )))
            .send_empty()?;
        read_json_or_error(resp)
    }
}
