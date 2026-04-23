use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::error::DomainError;

/// Newtype wrapper around the project identifier.
///
/// Wraps `i64` with `#[serde(transparent)]` so that the JSON wire format stays a
/// bare integer (e.g. `1`), not `{"0": 1}`. The goal is compile-time safety: a
/// `ProjectId` cannot be accidentally mixed with a `TaskId`, `user_id`, or
/// `contract_id` that also happen to be `i64`.
///
/// Unlike [`crate::domain::task::TaskId`] (which has a distinct `TaskDbId`
/// sealed inside `infra`), `ProjectId` is the DB primary key itself. The
/// infrastructure layer implements `rusqlite` / `sqlx` traits directly on
/// `ProjectId` (see `src/infra/mod.rs`), so no separate sealed newtype is
/// needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(pub i64);

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ProjectId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i64>().map(ProjectId)
    }
}

impl From<i64> for ProjectId {
    fn from(n: i64) -> Self {
        ProjectId(n)
    }
}

impl From<ProjectId> for i64 {
    fn from(id: ProjectId) -> i64 {
        id.0
    }
}

/// The default project cannot be deleted.
pub const DEFAULT_PROJECT_ID: ProjectId = ProjectId(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    id: ProjectId,
    name: String,
    description: Option<String>,
    created_at: String,
}

impl Project {
    pub fn new(
        id: ProjectId,
        name: String,
        description: Option<String>,
        created_at: String,
    ) -> Self {
        Self {
            id,
            name,
            description,
            created_at,
        }
    }

    pub fn id(&self) -> ProjectId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn validate_deletable(&self, task_count: i64) -> Result<(), DomainError> {
        if self.id == DEFAULT_PROJECT_ID {
            return Err(DomainError::CannotDeleteDefaultProject);
        }
        if task_count > 0 {
            return Err(DomainError::CannotDeleteProjectWithTasks { count: task_count });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectParams {
    pub name: String,
    pub description: Option<String>,
}

impl CreateProjectParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        use super::validator::*;
        validate_string_length("name", &self.name, MAX_PROJECT_NAME_LEN)?;
        validate_optional_string_length(
            "description",
            &self.description,
            MAX_PROJECT_DESCRIPTION_LEN,
        )?;
        Ok(())
    }
}

/// Filter / paging inputs for `list_projects`.
#[derive(Clone, Default)]
pub struct ListProjectsFilter {
    pub limit: Option<u32>,
    pub after: Option<ProjectId>,
}

/// Filter / paging inputs for `list_project_members`.
#[derive(Clone, Default)]
pub struct ListProjectMembersFilter {
    pub limit: Option<u32>,
    /// Cursor payload: the `project_members.id` of the last member returned.
    pub after: Option<i64>,
}

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project>;
    async fn get_project(&self, id: ProjectId) -> Result<Project>;
    async fn get_project_by_name(&self, name: &str) -> Result<Project>;
    async fn delete_project(&self, id: ProjectId) -> Result<()>;
}

use super::user::{AddProjectMemberParams, ProjectMember, Role, UserId};

#[async_trait]
pub trait ProjectMemberRepository: Send + Sync {
    async fn add_project_member(
        &self,
        project_id: ProjectId,
        params: &AddProjectMemberParams,
    ) -> Result<ProjectMember>;
    async fn remove_project_member(&self, project_id: ProjectId, user_id: UserId) -> Result<()>;
    async fn list_project_members(
        &self,
        project_id: ProjectId,
        filter: &ListProjectMembersFilter,
    ) -> Result<super::pagination::ListPage<ProjectMember>>;
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
    ) -> Result<ProjectMember>;
}
