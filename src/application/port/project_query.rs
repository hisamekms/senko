use anyhow::Result;
use async_trait::async_trait;

use crate::domain::pagination::ListPage;
use crate::domain::project::{ListProjectsFilter, Project};

#[async_trait]
pub trait ProjectQueryPort: Send + Sync {
    async fn list_projects(&self, filter: &ListProjectsFilter) -> Result<ListPage<Project>>;
}
