use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::application::port::{MetadataFieldOperations, TaskBackend};
use crate::domain::error::DomainError;
use crate::domain::metadata_field::{
    CreateMetadataFieldParams, ListMetadataFieldsFilter, MetadataField, MetadataFieldEvent,
    validate_field_name,
};
use crate::domain::pagination::ListPage;
use crate::domain::project::ProjectId;

pub struct MetadataFieldService {
    backend: Arc<dyn TaskBackend>,
}

impl MetadataFieldService {
    pub fn new(backend: Arc<dyn TaskBackend>) -> Self {
        Self { backend }
    }

    /// Create a metadata field and return the persisted entity along with the
    /// `Defined` domain event that B3 / A5 will later wire into hooks / OTel.
    pub async fn create_with_events(
        &self,
        project_id: ProjectId,
        params: &CreateMetadataFieldParams,
    ) -> Result<(MetadataField, Vec<MetadataFieldEvent>)> {
        validate_field_name(&params.name)?;
        let field = self
            .backend
            .create_metadata_field(project_id, params)
            .await?;
        let events = vec![MetadataFieldEvent::Defined {
            field_name: field.name().to_string(),
            field_type: field.field_type(),
        }];
        Ok((field, events))
    }

    /// Delete a metadata field by name and return the `Removed` domain event
    /// (carrying the captured pre-delete name + type).
    pub async fn delete_by_name_with_events(
        &self,
        project_id: ProjectId,
        name: &str,
    ) -> Result<Vec<MetadataFieldEvent>> {
        let fields = self
            .backend
            .list_metadata_fields(project_id, &ListMetadataFieldsFilter::default())
            .await?
            .items;
        let field = fields
            .into_iter()
            .find(|f| f.name() == name)
            .ok_or(DomainError::MetadataFieldNotFound)?;
        let captured_name = field.name().to_string();
        let captured_type = field.field_type();
        self.backend
            .delete_metadata_field(project_id, field.id())
            .await?;
        Ok(vec![MetadataFieldEvent::Removed {
            field_name: captured_name,
            field_type: captured_type,
        }])
    }
}

#[async_trait]
impl MetadataFieldOperations for MetadataFieldService {
    async fn create_metadata_field(
        &self,
        project_id: ProjectId,
        params: &CreateMetadataFieldParams,
    ) -> Result<MetadataField> {
        let (field, _events) = self.create_with_events(project_id, params).await?;
        Ok(field)
    }

    async fn list_metadata_fields(
        &self,
        project_id: ProjectId,
        filter: &ListMetadataFieldsFilter,
    ) -> Result<ListPage<MetadataField>> {
        self.backend.list_metadata_fields(project_id, filter).await
    }

    async fn delete_metadata_field_by_name(&self, project_id: ProjectId, name: &str) -> Result<()> {
        let _events = self.delete_by_name_with_events(project_id, name).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::metadata_field::MetadataFieldType;
    use crate::infra::sqlite::SqliteBackend;
    use tempfile::tempdir;

    async fn new_service() -> (tempfile::TempDir, MetadataFieldService, ProjectId) {
        let dir = tempdir().unwrap();
        let backend = SqliteBackend::new(
            dir.path(),
            Some(&dir.path().join("data.db")),
            None,
            &crate::infra::xdg::XdgDirs::default(),
        )
        .unwrap();
        let backend: Arc<dyn TaskBackend> = Arc::new(backend);
        let service = MetadataFieldService::new(backend);
        // Default project id=1 is seeded by migration v1.
        (dir, service, ProjectId(1))
    }

    fn params(name: &str, field_type: MetadataFieldType) -> CreateMetadataFieldParams {
        CreateMetadataFieldParams {
            name: name.to_string(),
            field_type,
            required_on_complete: false,
            description: None,
        }
    }

    #[tokio::test]
    async fn defined_event_carries_name_and_type() {
        let (_dir, service, project_id) = new_service().await;
        let cases = [
            ("sprint", MetadataFieldType::String),
            ("points", MetadataFieldType::Number),
            ("blocked", MetadataFieldType::Boolean),
        ];
        for (name, field_type) in cases {
            let (field, events) = service
                .create_with_events(project_id, &params(name, field_type))
                .await
                .unwrap();
            assert_eq!(field.name(), name);
            assert_eq!(field.field_type(), field_type);
            assert_eq!(
                events,
                vec![MetadataFieldEvent::Defined {
                    field_name: name.to_string(),
                    field_type,
                }]
            );
        }
    }

    #[tokio::test]
    async fn removed_event_carries_name_and_type() {
        let (_dir, service, project_id) = new_service().await;
        service
            .create_with_events(project_id, &params("sprint", MetadataFieldType::String))
            .await
            .unwrap();

        let events = service
            .delete_by_name_with_events(project_id, "sprint")
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![MetadataFieldEvent::Removed {
                field_name: "sprint".to_string(),
                field_type: MetadataFieldType::String,
            }]
        );
    }

    #[tokio::test]
    async fn delete_by_unknown_name_errors_without_event() {
        let (_dir, service, project_id) = new_service().await;
        let err = service
            .delete_by_name_with_events(project_id, "nope")
            .await
            .unwrap_err();
        assert!(
            err.downcast_ref::<DomainError>()
                .is_some_and(|e| matches!(e, DomainError::MetadataFieldNotFound)),
            "expected MetadataFieldNotFound, got: {err:?}"
        );
    }
}
