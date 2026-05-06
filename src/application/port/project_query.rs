use anyhow::Result;
use async_trait::async_trait;

use crate::domain::pagination::ListPage;
use crate::domain::project::{ListProjectsFilter, Project};
use crate::domain::user::UserId;

#[async_trait]
pub trait ProjectQueryPort: Send + Sync {
    /// `caller_user_id`: when `Some(uid)`, the result is restricted to projects
    /// where `uid` is a member; `None` returns all projects (used by master
    /// callers and auth-disabled deployments). The handler decides which to pass.
    async fn list_projects(
        &self,
        filter: &ListProjectsFilter,
        caller_user_id: Option<UserId>,
    ) -> Result<ListPage<Project>>;
}
