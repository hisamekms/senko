use std::sync::Arc;

use anyhow::Result;

use crate::domain::repository::TaskBackend;
use crate::domain::project::{CreateProjectParams, Project, DEFAULT_PROJECT_ID};
use crate::domain::task::ListTasksFilter;
use crate::domain::user::{
    AddProjectMemberParams, ProjectMember, Role,
};

pub struct ProjectService {
    backend: Arc<dyn TaskBackend>,
}

impl ProjectService {
    pub fn new(backend: Arc<dyn TaskBackend>) -> Self {
        Self { backend }
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        self.backend.list_projects().await
    }

    pub async fn create_project(&self, params: &CreateProjectParams) -> Result<Project> {
        self.backend.create_project(params).await
    }

    pub async fn get_project(&self, id: i64) -> Result<Project> {
        self.backend.get_project(id).await
    }

    pub async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        self.backend.get_project_by_name(name).await
    }

    pub async fn delete_project(&self, id: i64) -> Result<()> {
        if id == DEFAULT_PROJECT_ID {
            anyhow::bail!("cannot delete the default project");
        }
        let tasks = self.backend.list_tasks(id, &ListTasksFilter::default()).await?;
        if !tasks.is_empty() {
            anyhow::bail!("cannot delete project with {} existing task(s)", tasks.len());
        }
        self.backend.delete_project(id).await
    }

    // --- Member management ---

    pub async fn list_project_members(
        &self,
        project_id: i64,
    ) -> Result<Vec<ProjectMember>> {
        self.backend.list_project_members(project_id).await
    }

    pub async fn add_project_member(
        &self,
        project_id: i64,
        params: &AddProjectMemberParams,
    ) -> Result<ProjectMember> {
        self.backend.add_project_member(project_id, params).await
    }

    pub async fn remove_project_member(
        &self,
        project_id: i64,
        user_id: i64,
    ) -> Result<()> {
        self.backend.remove_project_member(project_id, user_id).await
    }

    pub async fn get_project_member(
        &self,
        project_id: i64,
        user_id: i64,
    ) -> Result<ProjectMember> {
        self.backend.get_project_member(project_id, user_id).await
    }

    pub async fn update_member_role(
        &self,
        project_id: i64,
        user_id: i64,
        role: Role,
    ) -> Result<ProjectMember> {
        self.backend.update_member_role(project_id, user_id, role).await
    }
}
