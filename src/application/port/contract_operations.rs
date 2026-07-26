use anyhow::Result;
use async_trait::async_trait;

use crate::domain::contract::{
    Contract, ContractId, ContractNote, CreateContractParams, ListContractNotesFilter,
    ListContractsFilter, UpdateContractArrayParams, UpdateContractParams,
};
use crate::domain::pagination::ListPage;
use crate::domain::project::ProjectId;
use crate::domain::task::TaskId;

/// Application-level port that exposes all contract operations.
///
/// Both local (`LocalContractOperations`) and remote implementations can
/// satisfy this trait, allowing the presentation layer to depend only on the
/// abstraction rather than a concrete service type.
#[async_trait]
pub trait ContractOperations: Send + Sync {
    // --- CRUD ---

    async fn create_contract(
        &self,
        project_id: ProjectId,
        params: &CreateContractParams,
    ) -> Result<Contract>;

    async fn get_contract(&self, project_id: ProjectId, id: ContractId) -> Result<Contract>;

    async fn list_contracts(
        &self,
        project_id: ProjectId,
        filter: &ListContractsFilter,
    ) -> Result<ListPage<Contract>>;

    async fn edit_contract(
        &self,
        project_id: ProjectId,
        id: ContractId,
        params: &UpdateContractParams,
        array_params: &UpdateContractArrayParams,
    ) -> Result<Contract>;

    async fn delete_contract(&self, project_id: ProjectId, id: ContractId) -> Result<()>;

    // --- Definition of Done ---

    async fn check_dod(
        &self,
        project_id: ProjectId,
        contract_id: ContractId,
        index: usize,
        verification_note: Option<String>,
    ) -> Result<Contract>;
    async fn uncheck_dod(
        &self,
        project_id: ProjectId,
        contract_id: ContractId,
        index: usize,
    ) -> Result<Contract>;

    // --- Notes ---

    async fn add_note(
        &self,
        project_id: ProjectId,
        contract_id: ContractId,
        content: String,
        source_task_id: Option<TaskId>,
    ) -> Result<ContractNote>;

    async fn list_notes(
        &self,
        project_id: ProjectId,
        contract_id: ContractId,
        filter: &ListContractNotesFilter,
    ) -> Result<ListPage<ContractNote>>;
}
