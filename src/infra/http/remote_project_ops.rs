use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::application::port::ProjectOperations;
use crate::domain::error::DomainError;
use crate::domain::project::{CreateProjectParams, Project, ProjectId};
use crate::domain::user::{AddProjectMemberParams, ProjectMember, Role};

use super::client::HttpClient;
use super::{check_success, read_json_or_error};

/// HTTP client implementing `ProjectOperations` directly.
///
/// Each method maps to a single API endpoint call. Domain logic is executed
/// server-side; this client only handles HTTP transport.
pub struct RemoteProjectOperations {
    http: HttpClient,
}

impl RemoteProjectOperations {
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        Self {
            http: HttpClient::new(base_url, api_key),
        }
    }

    fn url(&self, path: &str) -> String {
        self.http.url(path)
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        self.http.auth(builder)
    }

    fn client(&self) -> &reqwest::Client {
        self.http.reqwest()
    }
}

#[async_trait]
impl ProjectOperations for RemoteProjectOperations {
    // --- Project CRUD ---

    async fn list_projects(&self) -> Result<Vec<Project>> {
        let resp = self
            .auth(self.client().get(self.url("/api/v1/projects")))
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn create_project(
        &self,
        params: &CreateProjectParams,
        _caller_user_id: Option<i64>,
    ) -> Result<Project> {
        let resp = self
            .auth(
                self.client()
                    .post(self.url("/api/v1/projects"))
                    .json(params),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_project(&self, id: ProjectId) -> Result<Project> {
        let resp = self
            .auth(
                self.client()
                    .get(self.url(&format!("/api/v1/projects/{id}"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        let projects = self.list_projects().await?;
        projects
            .into_iter()
            .find(|p| p.name() == name)
            .ok_or_else(|| DomainError::ProjectNotFound.into())
    }

    async fn delete_project(&self, id: ProjectId, _caller_user_id: Option<i64>) -> Result<()> {
        let resp = self
            .auth(
                self.client()
                    .delete(self.url(&format!("/api/v1/projects/{id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    // --- Member management ---

    async fn list_project_members(&self, project_id: ProjectId) -> Result<Vec<ProjectMember>> {
        let resp = self
            .auth(
                self.client()
                    .get(self.url(&format!("/api/v1/projects/{project_id}/members"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn add_project_member(
        &self,
        project_id: ProjectId,
        params: &AddProjectMemberParams,
        _caller_user_id: Option<i64>,
    ) -> Result<ProjectMember> {
        let resp = self
            .auth(
                self.client()
                    .post(self.url(&format!("/api/v1/projects/{project_id}/members")))
                    .json(&json!({ "user_id": params.user_id, "role": params.role })),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn remove_project_member(
        &self,
        project_id: ProjectId,
        user_id: i64,
        _caller_user_id: Option<i64>,
    ) -> Result<()> {
        let resp = self
            .auth(
                self.client()
                    .delete(self.url(&format!("/api/v1/projects/{project_id}/members/{user_id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    async fn get_project_member(
        &self,
        project_id: ProjectId,
        user_id: i64,
    ) -> Result<ProjectMember> {
        let resp = self
            .auth(
                self.client()
                    .get(self.url(&format!("/api/v1/projects/{project_id}/members/{user_id}"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn update_member_role(
        &self,
        project_id: ProjectId,
        user_id: i64,
        role: Role,
        _caller_user_id: Option<i64>,
    ) -> Result<ProjectMember> {
        let resp = self
            .auth(
                self.client()
                    .put(self.url(&format!("/api/v1/projects/{project_id}/members/{user_id}")))
                    .json(&json!({ "role": role })),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }
}
