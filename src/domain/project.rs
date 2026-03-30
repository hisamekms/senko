use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::user::{AddProjectMemberParams, ProjectMember, Role};

/// The default project (id=1) cannot be deleted.
pub const DEFAULT_PROJECT_ID: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    id: i64,
    name: String,
    description: Option<String>,
    created_at: String,
}

impl Project {
    pub fn new(id: i64, name: String, description: Option<String>, created_at: String) -> Self {
        Self { id, name, description, created_at }
    }

    pub fn id(&self) -> i64 {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectParams {
    pub name: String,
    pub description: Option<String>,
}

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn create_project(&self, params: &CreateProjectParams) -> Result<Project>;
    async fn get_project(&self, id: i64) -> Result<Project>;
    async fn get_project_by_name(&self, name: &str) -> Result<Project>;
    async fn list_projects(&self) -> Result<Vec<Project>>;
    async fn delete_project(&self, id: i64) -> Result<()>;

    // Project membership
    async fn add_project_member(&self, project_id: i64, params: &AddProjectMemberParams) -> Result<ProjectMember>;
    async fn remove_project_member(&self, project_id: i64, user_id: i64) -> Result<()>;
    async fn list_project_members(&self, project_id: i64) -> Result<Vec<ProjectMember>>;
    async fn get_project_member(&self, project_id: i64, user_id: i64) -> Result<ProjectMember>;
    async fn update_member_role(&self, project_id: i64, user_id: i64, role: Role) -> Result<ProjectMember>;
}
