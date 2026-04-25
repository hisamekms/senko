use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::error::DomainError;
use super::user::{AddProjectMemberParams, ProjectMember, Role, UserId};

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

// --- Domain events ---

/// Domain event emitted by Project / ProjectMember mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectEvent {
    Created,
    Updated {
        changed_fields: Vec<String>,
    },
    MemberAdded {
        user_id: UserId,
        role: Role,
    },
    MemberRemoved {
        user_id: UserId,
    },
    MemberRoleChanged {
        user_id: UserId,
        from_role: Role,
        to_role: Role,
    },
}

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

    /// Apply scalar-field updates. Emits `Updated` with the list of changed field
    /// names (snake_case) when at least one field actually changed value.
    pub fn update(mut self, params: &UpdateProjectParams) -> (Project, Vec<ProjectEvent>) {
        let mut changed_fields: Vec<String> = Vec::new();
        if let Some(ref new_desc) = params.description
            && &self.description != new_desc
        {
            self.description = new_desc.clone();
            changed_fields.push("description".to_string());
        }
        if changed_fields.is_empty() {
            (self, vec![])
        } else {
            (self, vec![ProjectEvent::Updated { changed_fields }])
        }
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

/// Parameters for editing an existing project. `name` is intentionally not
/// editable — it is treated as immutable. `description` uses the
/// `Option<Option<String>>` pattern so the wire protocol can distinguish
/// "not specified" (`None`) from "set to NULL" (`Some(None)`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateProjectParams {
    pub description: Option<Option<String>>,
}

impl UpdateProjectParams {
    pub fn validate(&self) -> Result<(), DomainError> {
        use super::validator::*;
        validate_optional_nullable_string_length(
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
    async fn update_project(&self, id: ProjectId, params: &UpdateProjectParams) -> Result<Project>;
    async fn delete_project(&self, id: ProjectId) -> Result<()>;
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project(description: Option<&str>) -> Project {
        Project::new(
            ProjectId(7),
            "alpha".to_string(),
            description.map(|s| s.to_string()),
            "2026-04-25T00:00:00Z".to_string(),
        )
    }

    #[test]
    fn update_with_description_change_emits_updated_event() {
        let project = sample_project(Some("old"));
        let params = UpdateProjectParams {
            description: Some(Some("new".to_string())),
        };
        let (updated, events) = project.update(&params);
        assert_eq!(updated.description(), Some("new"));
        assert_eq!(
            events,
            vec![ProjectEvent::Updated {
                changed_fields: vec!["description".to_string()],
            }]
        );
    }

    #[test]
    fn update_no_op_emits_no_event() {
        let project = sample_project(Some("same"));
        let params = UpdateProjectParams {
            description: Some(Some("same".to_string())),
        };
        let (updated, events) = project.update(&params);
        assert_eq!(updated.description(), Some("same"));
        assert!(events.is_empty());
    }

    #[test]
    fn update_clear_description_emits_updated_with_description_field() {
        let project = sample_project(Some("had-value"));
        let params = UpdateProjectParams {
            description: Some(None),
        };
        let (updated, events) = project.update(&params);
        assert_eq!(updated.description(), None);
        assert_eq!(
            events,
            vec![ProjectEvent::Updated {
                changed_fields: vec!["description".to_string()],
            }]
        );
    }

    #[test]
    fn update_with_no_specified_fields_is_no_op() {
        let project = sample_project(Some("kept"));
        let params = UpdateProjectParams { description: None };
        let (updated, events) = project.update(&params);
        assert_eq!(updated.description(), Some("kept"));
        assert!(events.is_empty());
    }
}
