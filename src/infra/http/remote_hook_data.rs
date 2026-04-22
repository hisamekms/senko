use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::application::port::HookDataSource;
use crate::domain::project::{Project, ProjectId};
use crate::domain::task::{Task, TaskId, TaskStatus};
use crate::domain::user::{User, UserId};

use super::client::HttpClient;
use super::read_json_or_error;

/// HTTP-based implementation of [`HookDataSource`] for remote/proxy mode.
///
/// Provides only the data the hook system needs (stats, project/user/task
/// lookups) without implementing the full `TaskBackend` super-trait.
pub struct RemoteHookDataSource {
    http: HttpClient,
}

impl RemoteHookDataSource {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        Self {
            http: HttpClient::new(base_url, api_key),
        }
    }
}

#[async_trait]
impl HookDataSource for RemoteHookDataSource {
    async fn task_stats(&self, project_id: ProjectId) -> Result<HashMap<String, i64>> {
        let resp = self
            .http
            .auth(
                self.http
                    .reqwest()
                    .get(self.http.project_url(project_id, "/stats")),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn ready_count(&self, project_id: ProjectId) -> Result<i64> {
        let tasks: Vec<Task> = {
            let url = format!("{}?ready=true", self.http.project_url(project_id, "/tasks"));
            let resp = self.http.auth(self.http.reqwest().get(&url)).send().await?;
            read_json_or_error(resp).await?
        };
        Ok(tasks.len() as i64)
    }

    async fn get_project(&self, id: ProjectId) -> Result<Project> {
        let resp = self
            .http
            .auth(
                self.http
                    .reqwest()
                    .get(self.http.url(&format!("/api/v1/projects/{id}"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        let projects: Vec<Project> = {
            let resp = self
                .http
                .auth(self.http.reqwest().get(self.http.url("/api/v1/projects")))
                .send()
                .await?;
            read_json_or_error(resp).await?
        };
        projects
            .into_iter()
            .find(|p| p.name() == name)
            .ok_or_else(|| anyhow::anyhow!("project not found"))
    }

    async fn get_user(&self, id: UserId) -> Result<User> {
        let resp = self
            .http
            .auth(
                self.http
                    .reqwest()
                    .get(self.http.url(&format!("/api/v1/users/{id}"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_user_by_username(&self, username: &str) -> Result<User> {
        let users: Vec<User> = {
            let resp = self
                .http
                .auth(self.http.reqwest().get(self.http.url("/api/v1/users")))
                .send()
                .await?;
            read_json_or_error(resp).await?
        };
        users
            .into_iter()
            .find(|u| u.username() == username)
            .ok_or_else(|| anyhow::anyhow!("user not found"))
    }

    async fn get_task(&self, project_id: ProjectId, id: TaskId) -> Result<Task> {
        let resp = self
            .http
            .auth(
                self.http
                    .reqwest()
                    .get(self.http.project_url(project_id, &format!("/tasks/{id}"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn list_ready_tasks(&self, project_id: ProjectId) -> Result<Vec<Task>> {
        let url = format!("{}?ready=true", self.http.project_url(project_id, "/tasks"));
        let resp = self.http.auth(self.http.reqwest().get(&url)).send().await?;
        read_json_or_error(resp).await
    }

    async fn is_task_ready(&self, project_id: ProjectId, id: TaskId) -> Result<bool> {
        let task = match self.get_task(project_id, id).await {
            Ok(t) => t,
            Err(e) => {
                let task_number: i64 = id.into();
                let project_id_value: i64 = project_id.into();
                tracing::warn!(
                    project_id = project_id_value,
                    task_number,
                    error = %e,
                    "RemoteHookDataSource::is_task_ready: get_task failed, returning false"
                );
                return Ok(false);
            }
        };
        let mut dep_statuses: HashMap<TaskId, TaskStatus> = HashMap::new();
        for dep_task_number in task.dependencies() {
            if let Ok(dep) = self.get_task(project_id, *dep_task_number).await {
                dep_statuses.insert(dep.id(), dep.status());
            }
        }
        Ok(task.is_ready(&dep_statuses))
    }
}
