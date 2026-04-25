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

/// Emit Contract #8 business events for metadata field mutations. The
/// project_id and per-event payload (`field_name`, `field_type`) supply all
/// callsite attributes; common attributes are auto-attached.
fn emit_metadata_field_events(project_id: ProjectId, events: &[MetadataFieldEvent]) {
    for ev in events {
        match ev {
            MetadataFieldEvent::Defined {
                field_name,
                field_type,
            } => {
                let field_type_str = field_type.to_string();
                crate::emit_business_event!(
                    "senko.metadata_field.defined",
                    senko.project.id = project_id.0,
                    senko.metadata_field.name = field_name.as_str(),
                    "senko.metadata_field.type" = field_type_str.as_str(),
                );
            }
            MetadataFieldEvent::Removed {
                field_name,
                field_type,
            } => {
                let field_type_str = field_type.to_string();
                crate::emit_business_event!(
                    "senko.metadata_field.removed",
                    senko.project.id = project_id.0,
                    senko.metadata_field.name = field_name.as_str(),
                    "senko.metadata_field.type" = field_type_str.as_str(),
                );
            }
        }
    }
}

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
        emit_metadata_field_events(project_id, &events);
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
        let events = vec![MetadataFieldEvent::Removed {
            field_name: captured_name,
            field_type: captured_type,
        }];
        emit_metadata_field_events(project_id, &events);
        Ok(events)
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

    // --- Phase B3: business-event emission wire-up check ------------------

    #[tokio::test(flavor = "current_thread")]
    async fn create_with_events_emits_otel_log_record() {
        use crate::application::telemetry::test_support::{
            build_capture_provider, capture_layer, lookup_attr,
        };
        use opentelemetry::logs::AnyValue;
        use tracing_subscriber::layer::SubscriberExt;

        let (_dir, service, project_id) = new_service().await;
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));

        {
            let _guard = tracing::subscriber::set_default(subscriber);
            service
                .create_with_events(project_id, &params("epic", MetadataFieldType::String))
                .await
                .unwrap();
        }

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        let record = logs
            .iter()
            .find(|d| d.record.event_name() == Some("senko.metadata_field.defined"))
            .expect("senko.metadata_field.defined should be emitted");

        assert_eq!(
            lookup_attr(&record.record, "senko.project.id"),
            Some(AnyValue::Int(project_id.0))
        );
        assert_eq!(
            lookup_attr(&record.record, "senko.metadata_field.name"),
            Some(AnyValue::String("epic".into()))
        );
        assert_eq!(
            lookup_attr(&record.record, "senko.metadata_field.type"),
            Some(AnyValue::String("string".into()))
        );
    }
}
