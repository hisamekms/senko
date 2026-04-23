use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::application::auth::{Permission, require_project_role};
use crate::application::port::{ProjectOperations, TaskBackend};
use crate::domain::pagination::ListPage;
use crate::domain::project::{
    CreateProjectParams, ListProjectMembersFilter, ListProjectsFilter, Project, ProjectId,
};
use crate::domain::task::ListTasksFilter;
use crate::domain::user::{AddProjectMemberParams, ProjectMember, Role, UserId};

pub struct ProjectService {
    backend: Arc<dyn TaskBackend>,
}

impl ProjectService {
    pub fn new(backend: Arc<dyn TaskBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl ProjectOperations for ProjectService {
    async fn list_projects(&self, filter: &ListProjectsFilter) -> Result<ListPage<Project>> {
        self.backend.list_projects(filter).await
    }

    async fn create_project(
        &self,
        params: &CreateProjectParams,
        caller_user_id: Option<UserId>,
    ) -> Result<Project> {
        params.validate()?;
        let project = self.backend.create_project(params).await?;
        if let Some(uid) = caller_user_id {
            let member_params = AddProjectMemberParams::new(uid, Some(Role::Owner));
            self.backend
                .add_project_member(project.id(), &member_params)
                .await?;
        }
        Ok(project)
    }

    async fn get_project(&self, id: ProjectId) -> Result<Project> {
        self.backend.get_project(id).await
    }

    async fn get_project_by_name(&self, name: &str) -> Result<Project> {
        self.backend.get_project_by_name(name).await
    }

    async fn delete_project(&self, id: ProjectId, caller_user_id: Option<UserId>) -> Result<()> {
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, id, Permission::Admin).await?;
        }
        let project = self.backend.get_project(id).await?;
        let tasks = self
            .backend
            .list_tasks(id, &ListTasksFilter::default())
            .await?
            .items;
        project.validate_deletable(tasks.len() as i64)?;
        self.backend.delete_project(id).await
    }

    // --- Member management ---

    async fn list_project_members(
        &self,
        project_id: ProjectId,
        filter: &ListProjectMembersFilter,
    ) -> Result<ListPage<ProjectMember>> {
        self.backend.list_project_members(project_id, filter).await
    }

    async fn add_project_member(
        &self,
        project_id: ProjectId,
        params: &AddProjectMemberParams,
        caller_user_id: Option<UserId>,
    ) -> Result<ProjectMember> {
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, project_id, Permission::Admin).await?;
        }
        self.backend.add_project_member(project_id, params).await
    }

    async fn remove_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        caller_user_id: Option<UserId>,
    ) -> Result<()> {
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, project_id, Permission::Admin).await?;
        }
        self.backend
            .remove_project_member(project_id, user_id)
            .await
    }

    async fn get_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
    ) -> Result<ProjectMember> {
        self.backend.get_project_member(project_id, user_id).await
    }

    async fn update_member_role(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        role: Role,
        caller_user_id: Option<UserId>,
    ) -> Result<ProjectMember> {
        if let Some(uid) = caller_user_id {
            require_project_role(self.backend.as_ref(), uid, project_id, Permission::Admin).await?;
        }
        self.backend
            .update_member_role(project_id, user_id, role)
            .await
    }
}
