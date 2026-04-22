use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::application::port::{MetadataFieldOperations, TaskBackend};
use crate::domain::error::DomainError;
use crate::domain::metadata_field::{
    CreateMetadataFieldParams, MetadataField, validate_field_name,
};
use crate::domain::project::ProjectId;

pub struct MetadataFieldService {
    backend: Arc<dyn TaskBackend>,
}

impl MetadataFieldService {
    pub fn new(backend: Arc<dyn TaskBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl MetadataFieldOperations for MetadataFieldService {
    async fn create_metadata_field(
        &self,
        project_id: ProjectId,
        params: &CreateMetadataFieldParams,
    ) -> Result<MetadataField> {
        validate_field_name(&params.name)?;
        self.backend.create_metadata_field(project_id, params).await
    }

    async fn list_metadata_fields(&self, project_id: ProjectId) -> Result<Vec<MetadataField>> {
        self.backend.list_metadata_fields(project_id).await
    }

    async fn delete_metadata_field_by_name(&self, project_id: ProjectId, name: &str) -> Result<()> {
        let fields = self.backend.list_metadata_fields(project_id).await?;
        let field = fields
            .into_iter()
            .find(|f| f.name() == name)
            .ok_or(DomainError::MetadataFieldNotFound)?;
        self.backend
            .delete_metadata_field(project_id, field.id())
            .await
    }
}
