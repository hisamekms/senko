use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

use crate::application::HookTrigger;
use crate::application::port::{ContractOperations, HookExecutor, TaskBackend};
use crate::domain::contract::{
    Contract, ContractEvent, ContractId, ContractNote, CreateContractParams,
    ListContractNotesFilter, ListContractsFilter, UpdateContractArrayParams, UpdateContractParams,
};
use crate::domain::error::DomainError;
use crate::domain::pagination::ListPage;
use crate::domain::project::ProjectId;
use crate::infra::config::HookWhen;
use crate::infra::hook::FireOutcome;

pub struct LocalContractOperations {
    backend: Arc<dyn TaskBackend>,
    hooks: Arc<dyn HookExecutor>,
}

impl LocalContractOperations {
    pub fn new(backend: Arc<dyn TaskBackend>, hooks: Arc<dyn HookExecutor>) -> Self {
        Self { backend, hooks }
    }
}

fn now_iso8601() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[async_trait]
impl ContractOperations for LocalContractOperations {
    async fn create_contract(
        &self,
        project_id: ProjectId,
        params: &CreateContractParams,
    ) -> Result<Contract> {
        params.validate()?;

        let trigger = HookTrigger::Contract(ContractEvent::Created);
        if self
            .hooks
            .fire_contract(&trigger, HookWhen::Pre, None)
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "contract_add".into(),
            }
            .into());
        }

        let contract = self.backend.create_contract(project_id, params).await?;

        let _ = self
            .hooks
            .fire_contract(&trigger, HookWhen::Post, Some(&contract))
            .await;

        crate::emit_business_event!(
            "senko.contract.created",
            senko.contract.id = contract.id().0,
            senko.project.id = project_id.0,
        );

        Ok(contract)
    }

    async fn get_contract(&self, _project_id: ProjectId, id: ContractId) -> Result<Contract> {
        self.backend.get_contract(id).await
    }

    async fn list_contracts(
        &self,
        project_id: ProjectId,
        filter: &ListContractsFilter,
    ) -> Result<ListPage<Contract>> {
        self.backend.list_contracts(project_id, filter).await
    }

    async fn edit_contract(
        &self,
        _project_id: ProjectId,
        id: ContractId,
        params: &UpdateContractParams,
        array_params: &UpdateContractArrayParams,
    ) -> Result<Contract> {
        params.validate()?;
        array_params.validate()?;

        let prev = self.backend.get_contract(id).await?;
        let trigger = HookTrigger::Contract(ContractEvent::Updated {
            changed_fields: Vec::new(),
        });
        if self
            .hooks
            .fire_contract(&trigger, HookWhen::Pre, Some(&prev))
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "contract_edit".into(),
            }
            .into());
        }

        // Capture the per-field diff from the domain aggregate methods. The
        // returned (Contract, _) pair is discarded: the persistence path goes
        // through `backend.update_contract`; we only need the
        // `ContractEvent::Updated { changed_fields }` payload to attribute the
        // emit. Two clones (one per method) are intentional — the methods
        // consume `self`, and merging them in-place would couple this site to
        // the domain's internal ordering.
        let (_, scalar_events) = prev.clone().update(params, String::new());
        let (_, array_events) = prev.clone().apply_array_update(array_params, String::new());
        let changed_fields: Vec<String> = scalar_events
            .iter()
            .chain(array_events.iter())
            .flat_map(|ev| match ev {
                ContractEvent::Updated { changed_fields } => changed_fields.clone(),
                _ => Vec::new(),
            })
            .collect();

        let contract = self
            .backend
            .update_contract(id, params, array_params)
            .await?;

        let _ = self
            .hooks
            .fire_contract(&trigger, HookWhen::Post, Some(&contract))
            .await;

        if !changed_fields.is_empty() {
            let changed_fields_json = serde_json::to_string(&changed_fields).unwrap_or_default();
            crate::emit_business_event!(
                "senko.contract.updated",
                senko.contract.id = contract.id().0,
                senko.project.id = contract.project_id().0,
                changed_fields = changed_fields_json.as_str(),
            );
        }

        Ok(contract)
    }

    async fn delete_contract(&self, _project_id: ProjectId, id: ContractId) -> Result<()> {
        let prev = self.backend.get_contract(id).await?;
        let trigger = HookTrigger::Contract(ContractEvent::Deleted);
        if self
            .hooks
            .fire_contract(&trigger, HookWhen::Pre, Some(&prev))
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "contract_delete".into(),
            }
            .into());
        }

        self.backend.delete_contract(id).await?;

        let _ = self
            .hooks
            .fire_contract(&trigger, HookWhen::Post, Some(&prev))
            .await;

        crate::emit_business_event!(
            "senko.contract.deleted",
            senko.contract.id = prev.id().0,
            senko.project.id = prev.project_id().0,
        );

        Ok(())
    }

    async fn check_dod(
        &self,
        _project_id: ProjectId,
        contract_id: ContractId,
        index: usize,
        verification_note: Option<String>,
    ) -> Result<Contract> {
        let prev = self.backend.get_contract(contract_id).await?;
        let trigger = HookTrigger::Contract(ContractEvent::DodChecked { index });
        if self
            .hooks
            .fire_contract(&trigger, HookWhen::Pre, Some(&prev))
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "contract_dod_check".into(),
            }
            .into());
        }

        let contract = self
            .backend
            .check_dod(contract_id, index, verification_note)
            .await?;

        let _ = self
            .hooks
            .fire_contract(&trigger, HookWhen::Post, Some(&contract))
            .await;

        crate::emit_business_event!(
            "senko.contract.dod_checked",
            senko.contract.id = contract.id().0,
            senko.project.id = contract.project_id().0,
            dod_index = index as i64,
        );

        Ok(contract)
    }

    async fn uncheck_dod(
        &self,
        _project_id: ProjectId,
        contract_id: ContractId,
        index: usize,
    ) -> Result<Contract> {
        let prev = self.backend.get_contract(contract_id).await?;
        let trigger = HookTrigger::Contract(ContractEvent::DodUnchecked { index });
        if self
            .hooks
            .fire_contract(&trigger, HookWhen::Pre, Some(&prev))
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "contract_dod_uncheck".into(),
            }
            .into());
        }

        let contract = self.backend.uncheck_dod(contract_id, index).await?;

        let _ = self
            .hooks
            .fire_contract(&trigger, HookWhen::Post, Some(&contract))
            .await;

        crate::emit_business_event!(
            "senko.contract.dod_unchecked",
            senko.contract.id = contract.id().0,
            senko.project.id = contract.project_id().0,
            dod_index = index as i64,
        );

        Ok(contract)
    }

    async fn add_note(
        &self,
        _project_id: ProjectId,
        contract_id: ContractId,
        content: String,
        source_task_id: Option<crate::domain::task::TaskId>,
    ) -> Result<ContractNote> {
        let note = ContractNote::new(content, source_task_id, now_iso8601());
        note.validate()?;

        let prev = self.backend.get_contract(contract_id).await?;
        let trigger = HookTrigger::Contract(ContractEvent::NoteAdded);
        if self
            .hooks
            .fire_contract(&trigger, HookWhen::Pre, Some(&prev))
            .await
            == FireOutcome::Abort
        {
            return Err(DomainError::HookAborted {
                event: "contract_note_add".into(),
            }
            .into());
        }

        let created = self.backend.add_note(contract_id, &note).await?;

        let after = self.backend.get_contract(contract_id).await.ok();
        let _ = self
            .hooks
            .fire_contract(&trigger, HookWhen::Post, after.as_ref())
            .await;

        crate::emit_business_event!(
            "senko.contract.note_added",
            senko.contract.id = contract_id.0,
            senko.project.id = prev.project_id().0,
        );

        Ok(created)
    }

    async fn list_notes(
        &self,
        _project_id: ProjectId,
        contract_id: ContractId,
        filter: &ListContractNotesFilter,
    ) -> Result<ListPage<ContractNote>> {
        // Verify the contract exists so that listing notes for an unknown id
        // fails loudly instead of returning an empty page (matching prior
        // `list_notes` behaviour, which went through the aggregate).
        let _ = self.backend.get_contract(contract_id).await?;
        self.backend.list_contract_notes(contract_id, filter).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::port::{NoOpHookExecutor, TaskBackend};
    use crate::infra::sqlite::SqliteBackend;
    use tempfile::tempdir;

    fn noop_hooks() -> Arc<dyn HookExecutor> {
        Arc::new(NoOpHookExecutor)
    }

    fn dod_input(content: &str) -> crate::domain::task::DodItemInput {
        crate::domain::task::DodItemInput {
            content: content.into(),
            verification_type: crate::domain::task::VerificationType::Manual,
            verification_method: None,
        }
    }

    async fn new_backend() -> (tempfile::TempDir, Arc<dyn TaskBackend>, ProjectId) {
        let dir = tempdir().unwrap();
        let backend = SqliteBackend::new(
            dir.path(),
            Some(&dir.path().join("data.db")),
            None,
            &crate::infra::xdg::XdgDirs::default(),
        )
        .unwrap();
        let backend: Arc<dyn TaskBackend> = Arc::new(backend);
        // Default project id=1 is seeded by migration v1.
        (dir, backend, ProjectId(1))
    }

    fn simple_params() -> CreateContractParams {
        CreateContractParams {
            title: "contract-title".to_string(),
            description: Some("desc".to_string()),
            definition_of_done: vec![dod_input("dod-1"), dod_input("dod-2")],
            tags: vec!["tag-a".to_string()],
            metadata: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_contract() {
        let (_dir, backend, project_id) = new_backend().await;
        let ops = LocalContractOperations::new(backend, noop_hooks());

        let created = ops
            .create_contract(project_id, &simple_params())
            .await
            .unwrap();
        assert_eq!(created.title(), "contract-title");

        let fetched = ops.get_contract(project_id, created.id()).await.unwrap();
        assert_eq!(fetched.id(), created.id());
        assert_eq!(fetched.title(), "contract-title");
        assert_eq!(fetched.tags(), &["tag-a".to_string()]);
        assert_eq!(fetched.definition_of_done().len(), 2);
    }

    #[tokio::test]
    async fn list_contracts_returns_all() {
        let (_dir, backend, project_id) = new_backend().await;
        let ops = LocalContractOperations::new(backend, noop_hooks());

        ops.create_contract(project_id, &simple_params())
            .await
            .unwrap();
        let mut p2 = simple_params();
        p2.title = "another".to_string();
        ops.create_contract(project_id, &p2).await.unwrap();

        let list = ops
            .list_contracts(project_id, &ListContractsFilter::default())
            .await
            .unwrap();
        assert_eq!(list.items.len(), 2);
    }

    #[tokio::test]
    async fn edit_contract_scalar_and_array() {
        let (_dir, backend, project_id) = new_backend().await;
        let ops = LocalContractOperations::new(backend, noop_hooks());
        let c = ops
            .create_contract(project_id, &simple_params())
            .await
            .unwrap();

        let update = UpdateContractParams {
            title: Some("new-title".to_string()),
            ..Default::default()
        };
        let array_update = UpdateContractArrayParams {
            add_tags: vec!["extra".to_string()],
            add_definition_of_done: vec![dod_input("dod-3")],
            ..Default::default()
        };

        let edited = ops
            .edit_contract(project_id, c.id(), &update, &array_update)
            .await
            .unwrap();
        assert_eq!(edited.title(), "new-title");
        assert!(edited.tags().contains(&"extra".to_string()));
        assert_eq!(edited.definition_of_done().len(), 3);
    }

    #[tokio::test]
    async fn check_and_uncheck_dod() {
        let (_dir, backend, project_id) = new_backend().await;
        let ops = LocalContractOperations::new(backend, noop_hooks());
        let c = ops
            .create_contract(project_id, &simple_params())
            .await
            .unwrap();

        let after_check = ops.check_dod(project_id, c.id(), 1, None).await.unwrap();
        assert!(after_check.definition_of_done()[0].checked());
        assert!(!after_check.definition_of_done()[1].checked());

        let after_uncheck = ops.uncheck_dod(project_id, c.id(), 1).await.unwrap();
        assert!(!after_uncheck.definition_of_done()[0].checked());
    }

    #[tokio::test]
    async fn add_and_list_notes() {
        let (_dir, backend, project_id) = new_backend().await;
        let ops = LocalContractOperations::new(backend.clone(), noop_hooks());
        let c = ops
            .create_contract(project_id, &simple_params())
            .await
            .unwrap();

        // Create a real task so source_task_id can reference it (FK constraint).
        let task = backend
            .create_task(
                project_id,
                &crate::domain::task::CreateTaskParams {
                    title: "src".to_string(),
                    background: None,
                    description: None,
                    priority: None,
                    definition_of_done: vec![],
                    in_scope: vec![],
                    out_of_scope: vec![],
                    branch: None,
                    pr_url: None,
                    metadata: None,
                    tags: vec![],
                    dependencies: vec![],
                    assignee_user_id: None,
                    contract_id: None,
                },
            )
            .await
            .unwrap();

        let n1 = ops
            .add_note(
                project_id,
                c.id(),
                "first note".to_string(),
                Some(task.id()),
            )
            .await
            .unwrap();
        assert_eq!(n1.content(), "first note");
        assert_eq!(n1.source_task_id(), Some(task.id()));

        ops.add_note(project_id, c.id(), "second".to_string(), None)
            .await
            .unwrap();

        let notes = ops
            .list_notes(project_id, c.id(), &ListContractNotesFilter::default())
            .await
            .unwrap();
        assert_eq!(notes.items.len(), 2);
        assert_eq!(notes.items[0].content(), "first note");
        assert_eq!(notes.items[0].source_task_id(), Some(task.id()));
        assert_eq!(notes.items[1].content(), "second");
        assert_eq!(notes.items[1].source_task_id(), None);
    }

    #[tokio::test]
    async fn delete_contract_removes_it() {
        let (_dir, backend, project_id) = new_backend().await;
        let ops = LocalContractOperations::new(backend, noop_hooks());
        let c = ops
            .create_contract(project_id, &simple_params())
            .await
            .unwrap();

        ops.delete_contract(project_id, c.id()).await.unwrap();
        assert!(ops.get_contract(project_id, c.id()).await.is_err());
    }

    #[tokio::test]
    async fn create_contract_validates_params() {
        let (_dir, backend, project_id) = new_backend().await;
        let ops = LocalContractOperations::new(backend, noop_hooks());

        let mut params = simple_params();
        params.title = "x".repeat(10_000);
        let err = ops.create_contract(project_id, &params).await;
        assert!(err.is_err());
    }

    // --- Phase B3: business-event emission wire-up check ------------------

    #[tokio::test(flavor = "current_thread")]
    async fn create_contract_emits_otel_log_record() {
        use crate::application::telemetry::test_support::{
            build_capture_provider, capture_layer, lookup_attr,
        };
        use opentelemetry::logs::AnyValue;
        use tracing_subscriber::layer::SubscriberExt;

        let (_dir, backend, project_id) = new_backend().await;
        let ops = LocalContractOperations::new(backend, noop_hooks());
        let (exporter, provider) = build_capture_provider();
        let subscriber = tracing_subscriber::registry().with(capture_layer(&provider));

        let created = {
            let _guard = tracing::subscriber::set_default(subscriber);
            ops.create_contract(project_id, &simple_params())
                .await
                .unwrap()
        };

        provider.force_flush().expect("flush ok");
        let logs = exporter.get_emitted_logs().expect("logs exported");
        let record = logs
            .iter()
            .find(|d| d.record.event_name() == Some("senko.contract.created"))
            .expect("senko.contract.created should be emitted");

        assert_eq!(
            lookup_attr(&record.record, "senko.contract.id"),
            Some(AnyValue::Int(created.id().0))
        );
        assert_eq!(
            lookup_attr(&record.record, "senko.project.id"),
            Some(AnyValue::Int(project_id.0))
        );
    }
}
