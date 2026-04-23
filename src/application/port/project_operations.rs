use anyhow::Result;
use async_trait::async_trait;

use crate::domain::pagination::ListPage;
use crate::domain::project::{
    CreateProjectParams, ListProjectMembersFilter, ListProjectsFilter, Project, ProjectId,
};
use crate::domain::user::{AddProjectMemberParams, ProjectMember, Role, UserId};

/// Application-level port that exposes all project operations.
///
/// Both local (`ProjectService`) and remote implementations can satisfy this
/// trait, allowing the presentation layer to depend only on the abstraction
/// rather than a concrete service type.
#[async_trait]
pub trait ProjectOperations: Send + Sync {
    // --- Project CRUD ---

    async fn list_projects(&self, filter: &ListProjectsFilter) -> Result<ListPage<Project>>;
    async fn create_project(
        &self,
        params: &CreateProjectParams,
        caller_user_id: Option<UserId>,
    ) -> Result<Project>;
    async fn get_project(&self, id: ProjectId) -> Result<Project>;
    async fn get_project_by_name(&self, name: &str) -> Result<Project>;
    async fn delete_project(&self, id: ProjectId, caller_user_id: Option<UserId>) -> Result<()>;

    // --- Member management ---

    async fn list_project_members(
        &self,
        project_id: ProjectId,
        filter: &ListProjectMembersFilter,
    ) -> Result<ListPage<ProjectMember>>;
    async fn add_project_member(
        &self,
        project_id: ProjectId,
        params: &AddProjectMemberParams,
        caller_user_id: Option<UserId>,
    ) -> Result<ProjectMember>;
    async fn remove_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        caller_user_id: Option<UserId>,
    ) -> Result<()>;
    async fn get_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
    ) -> Result<ProjectMember>;
    async fn update_member_role(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        role: Role,
        caller_user_id: Option<UserId>,
    ) -> Result<ProjectMember>;
}
