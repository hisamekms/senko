use anyhow::Result;
use async_trait::async_trait;

use crate::domain::pagination::ListPage;
use crate::domain::project::{
    CreateProjectParams, ListProjectMembersFilter, ListProjectsFilter, Project, ProjectEvent,
    ProjectId, UpdateProjectParams,
};
use crate::domain::user::{AddProjectMemberParams, ProjectMember, Role, UserId};

/// Application-level port that exposes all project operations.
///
/// Both local (`ProjectService`) and remote implementations can satisfy this
/// trait, allowing the presentation layer to depend only on the abstraction
/// rather than a concrete service type.
///
/// Mutation methods return a tuple of `(result, Vec<ProjectEvent>)`. Local
/// implementations populate the event vec; remote implementations return an
/// empty vec because the events are emitted server-side. Phase B3 will route
/// the returned events into `emit_business_event!`.
#[async_trait]
pub trait ProjectOperations: Send + Sync {
    // --- Project CRUD ---

    async fn list_projects(&self, filter: &ListProjectsFilter) -> Result<ListPage<Project>>;
    async fn create_project(
        &self,
        params: &CreateProjectParams,
        caller_user_id: Option<UserId>,
    ) -> Result<(Project, Vec<ProjectEvent>)>;
    async fn get_project(&self, id: ProjectId) -> Result<Project>;
    async fn get_project_by_name(&self, name: &str) -> Result<Project>;
    async fn update_project(
        &self,
        id: ProjectId,
        params: &UpdateProjectParams,
        caller_user_id: Option<UserId>,
    ) -> Result<(Project, Vec<ProjectEvent>)>;
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
    ) -> Result<(ProjectMember, Vec<ProjectEvent>)>;
    async fn remove_project_member(
        &self,
        project_id: ProjectId,
        user_id: UserId,
        caller_user_id: Option<UserId>,
    ) -> Result<Vec<ProjectEvent>>;
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
    ) -> Result<(ProjectMember, Vec<ProjectEvent>)>;
}
