use anyhow::Result;
use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::json;

use crate::application::port::ProjectOperations;
use crate::domain::error::DomainError;
use crate::domain::pagination::{Cursor, ListPage};
use crate::domain::project::{
    CreateProjectParams, ListProjectMembersFilter, ListProjectsFilter, Project, ProjectEvent,
    ProjectId, UpdateProjectParams,
};
use crate::domain::user::{AddProjectMemberParams, ProjectMember, Role, UserId};

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
    pub fn new(
        base_url: &str,
        api_key: Option<String>,
        attributes: std::collections::BTreeMap<String, String>,
    ) -> Self {
        Self {
            http: HttpClient::new(base_url, api_key, attributes),
        }
    }

    fn url(&self, path: &str) -> String {
        self.http.url(path)
    }

    /// Attach Bearer auth + W3C trace-propagation headers to the request builder.
    fn prepare(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        self.http.propagate(self.http.auth(builder))
    }

    fn client(&self) -> &reqwest::Client {
        self.http.reqwest()
    }
}

#[async_trait]
impl ProjectOperations for RemoteProjectOperations {
    // --- Project CRUD ---

    async fn list_projects(&self, filter: &ListProjectsFilter) -> Result<ListPage<Project>> {
        let mut url = self.url("/api/v1/projects");
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

    async fn create_project(
        &self,
        params: &CreateProjectParams,
        _caller_user_id: Option<UserId>,
    ) -> Result<(Project, Vec<ProjectEvent>)> {
        let resp = self
            .prepare(
                self.client()
                    .post(self.url("/api/v1/projects"))
                    .json(params),
            )
            .send()
            .await?;
        let project: Project = read_json_or_error(resp).await?;

        crate::emit_business_event!("senko.project.created", senko.project.id = project.id().0,);

        Ok((project, Vec::new()))
    }

    async fn get_project(&self, id: ProjectId) -> Result<Project> {
        let resp = self
            .prepare(
                self.client()
                    .get(self.url(&format!("/api/v1/projects/{id}"))),
            )
            .send()
            .await?;
        read_json_or_error(resp).await
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        // Paginate to find the named project. A server-side filter would be nicer
        // but isn't exposed yet; this keeps behaviour identical to the previous
        // "list all + find" approach.
        let mut filter = ListProjectsFilter::default();
        loop {
            let page = self.list_projects(&filter).await?;
            for p in page.items {
                if p.name() == name {
                    return Ok(p);
                }
            }
            match page.next_cursor {
                Some(cursor) => {
                    filter.after = Some(Cursor::decode::<ProjectId>(&cursor)?);
                }
                None => return Err(DomainError::ProjectNotFound.into()),
            }
        }
    }

    async fn update_project(
        &self,
        id: ProjectId,
        params: &UpdateProjectParams,
        _caller_user_id: Option<UserId>,
    ) -> Result<(Project, Vec<ProjectEvent>)> {
        // Mirror the API's `UpdateProjectBody` wire format: explicit clear flag.
        let body = match &params.description {
            Some(None) => json!({ "clear_description": true }),
            Some(Some(text)) => json!({ "description": text }),
            None => json!({}),
        };
        let resp = self
            .prepare(
                self.client()
                    .put(self.url(&format!("/api/v1/projects/{id}")))
                    .json(&body),
            )
            .send()
            .await?;
        let project: Project = read_json_or_error(resp).await?;

        // Mirror Project::update semantics: a field is "changed" when the
        // caller supplied any value for it (no prev fetch on the relay).
        let mut changed_fields: Vec<String> = Vec::new();
        if params.description.is_some() {
            changed_fields.push("description".into());
        }
        if !changed_fields.is_empty() {
            let changed_fields_json = serde_json::to_string(&changed_fields).unwrap_or_default();
            crate::emit_business_event!(
                "senko.project.updated",
                senko.project.id = id.0,
                changed_fields = changed_fields_json.as_str(),
            );
        }

        Ok((project, Vec::new()))
    }

    async fn delete_project(&self, id: ProjectId, _caller_user_id: Option<UserId>) -> Result<()> {
        let resp = self
            .prepare(
                self.client()
                    .delete(self.url(&format!("/api/v1/projects/{id}"))),
            )
            .send()
            .await?;
        check_success(resp).await
    }

    // --- Member management ---

    async fn list_project_members(
        &self,
        project_id: ProjectId,
        filter: &ListProjectMembersFilter,
    ) -> Result<ListPage<ProjectMember>> {
        let mut url = self.url(&format!("/api/v1/projects/{project_id}/members"));
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

    async fn add_project_member(
        &self,
        project_id: ProjectId,
        params: &AddProjectMemberParams,
        _caller_user_id: Option<UserId>,
    ) -> Result<(ProjectMember, Vec<ProjectEvent>)> {
        let resp = self
            .prepare(
                self.client()
                    .post(self.url(&format!("/api/v1/projects/{project_id}/members")))
                    .json(&json!({ "user_id": params.user_id, "role": params.role })),
            )
            .send()
            .await?;
        let member: ProjectMember = read_json_or_error(resp).await?;

        crate::emit_business_event!(
            "senko.project.member_added",
            senko.project.id = project_id.0,
            senko.user.id = member.user_id().0,
            role = %member.role(),
        );

        Ok((member, Vec::new()))
    }

    async fn remove_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        _caller_user_id: Option<UserId>,
    ) -> Result<Vec<ProjectEvent>> {
        let resp = self
            .prepare(
                self.client()
                    .delete(self.url(&format!("/api/v1/projects/{project_id}/members/{user_id}"))),
            )
            .send()
            .await?;
        check_success(resp).await?;

        crate::emit_business_event!(
            "senko.project.member_removed",
            senko.project.id = project_id.0,
            senko.user.id = user_id.0,
        );

        Ok(Vec::new())
    }

    async fn get_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
    ) -> Result<ProjectMember> {
        let resp = self
            .prepare(
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
        user_id: UserId,
        role: Role,
        _caller_user_id: Option<UserId>,
    ) -> Result<(ProjectMember, Vec<ProjectEvent>)> {
        // Fetch prev member so we can emit `from_role` (Contract #8 requires
        // both from_role / to_role on member_role_changed). Mirrors the
        // prev-fetch pattern in `RemoteTaskOperations` for `prev_status`.
        let prev = self.get_project_member(project_id, user_id).await?;
        let from_role = prev.role();

        let resp = self
            .prepare(
                self.client()
                    .put(self.url(&format!("/api/v1/projects/{project_id}/members/{user_id}")))
                    .json(&json!({ "role": role })),
            )
            .send()
            .await?;
        let member: ProjectMember = read_json_or_error(resp).await?;

        if from_role != role {
            crate::emit_business_event!(
                "senko.project.member_role_changed",
                senko.project.id = project_id.0,
                senko.user.id = user_id.0,
                from_role = %from_role,
                to_role = %role,
            );
        }

        Ok((member, Vec::new()))
    }
}
